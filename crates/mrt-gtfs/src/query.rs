//! The public scheduled-trip query API.
//!
//! [`RailNetwork::query_trip_instances`] answers one question: which
//! trains actually run on a service date, and what does each of them
//! do at every station it calls at? The answer is a list of
//! [`TripInstance`] values with complete [`ScheduledCall`] lists, plus
//! the headway-based [`FrequencyBand`] entries that the selected
//! [`FrequencyPolicy`] left unexpanded, plus the diagnostics that
//! explain everything the query could not represent.
//!
//! The API is deliberately renderer-independent. It knows nothing
//! about timetables, diagrams, HTML, or files. The `mrt-publication`
//! crate turns its output into view models.
//!
//! # What the query guarantees
//!
//! - Times stay on the GTFS service day. `25:35:00` remains
//!   `25:35:00`; nothing wraps to `01:35:00` before display.
//! - Calls follow `stop_sequence`, not file order.
//! - Every call carries the platform that the trip really uses.
//! - A time that the library computed is marked
//!   [`TimeQuality::Interpolated`]. A time that the feed itself marks
//!   as approximate (`timepoint=0`) is marked
//!   [`TimeQuality::Approximate`].
//! - Headway-based service with `exact_times=0` never turns into
//!   exact-looking departures unless the caller asks for it with
//!   [`FrequencyPolicy::ExpandApproximate`], and then every instance
//!   carries [`TimeExactness::Approximate`].
//!
//! # Example
//!
//! ```no_run
//! use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork, TripInstanceQuery};
//!
//! let feed = GtfsFeed::from_zip_path("data/singapore-gtfs.zip").unwrap();
//! let network = RailNetwork::from_feed(&feed).unwrap();
//!
//! let query = TripInstanceQuery::new("20260810".parse().unwrap())
//!     .window(GtfsTime::from_hms(5, 0, 0), GtfsTime::from_hms(10, 0, 0));
//! let result = network.query_trip_instances(&query).unwrap();
//! println!("{} trains run in the window.", result.trips.len());
//! ```

use serde::{Deserialize, Serialize};

use crate::date::ServiceDate;
use crate::diag::Diagnostic;
use crate::error::GtfsError;
use crate::model::Frequency;
use crate::network::{LineId, PatternId, RailNetwork, StationId, StopCall, TripSchedule};
use crate::time::GtfsTime;

/// How a query treats headway-based service from `frequencies.txt`.
///
/// GTFS `exact_times=1` describes a schedule that repeats exactly, so
/// expansion into single trips is always correct. `exact_times=0`
/// describes "a train about every N minutes"; the individual times do
/// not exist. This enumeration decides what happens to that service.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FrequencyPolicy {
    /// Return non-exact service as [`FrequencyBand`] entries and never
    /// invent single departures. This is the default.
    #[default]
    Bands,
    /// Expand non-exact service into trip instances that carry
    /// [`TimeExactness::Approximate`]. Renderers must mark them.
    ExpandApproximate,
    /// Fail when non-exact service affects the requested output.
    RejectNonExact,
}

/// How a query fills in stop times that the feed leaves empty.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissingTimePolicy {
    /// Leave a missing time missing. Calls keep
    /// [`TimeQuality::Missing`].
    None,
    /// Interpolate only between two known times. This is the default.
    /// A gap at the start or at the end of a trip stays missing.
    #[default]
    InterpolateBounded,
    /// Interpolate between known times and also extend the first and
    /// the last known times outwards at the neighbouring rate.
    ///
    /// This invents times outside the range that the feed supplies.
    /// Ask for it explicitly.
    InterpolateUnbounded,
}

/// Whether the times of a trip instance are exact.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimeExactness {
    /// The feed supplies an exact schedule for this instance.
    #[default]
    Exact,
    /// The instance comes from a headway template. The times are
    /// representative, not scheduled. Renderers must mark them.
    Approximate,
}

impl TimeExactness {
    /// Report whether the instance is exact.
    pub const fn is_exact(self) -> bool {
        matches!(self, TimeExactness::Exact)
    }
}

/// Where the time of one call comes from.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimeQuality {
    /// The feed supplies the time and does not mark it as approximate.
    #[default]
    Exact,
    /// The feed supplies the time but marks the call `timepoint=0`.
    Approximate,
    /// The library computed the time between two known times.
    Interpolated,
    /// No time is available.
    Missing,
}

/// One scheduled call of one trip instance at one station.
#[derive(Clone, Debug, Serialize)]
pub struct ScheduledCall {
    /// The station of the call.
    pub station: StationId,
    /// The `stop_id` of the platform that the trip uses.
    pub platform_stop_id: String,
    /// The passenger-facing platform label, when the feed supplies one.
    pub platform_code: Option<String>,
    /// The arrival time on the service day.
    pub arrival: Option<GtfsTime>,
    /// The departure time on the service day.
    pub departure: Option<GtfsTime>,
    /// The per-call destination text, when the feed supplies one.
    pub stop_headsign: Option<String>,
    /// The GTFS pickup rule. `1` means that boarding is not possible.
    pub pickup_type: Option<u8>,
    /// The GTFS drop-off rule. `1` means that alighting is not
    /// possible.
    pub drop_off_type: Option<u8>,
    /// Where the times of this call come from.
    pub time_quality: TimeQuality,
}

impl ScheduledCall {
    /// Get the best departure time: the departure, or else the arrival.
    pub fn departure_or_arrival(&self) -> Option<GtfsTime> {
        self.departure.or(self.arrival)
    }

    /// Get the best arrival time: the arrival, or else the departure.
    pub fn arrival_or_departure(&self) -> Option<GtfsTime> {
        self.arrival.or(self.departure)
    }

    /// Report whether passengers may board here.
    pub fn allows_pickup(&self) -> bool {
        self.pickup_type != Some(1)
    }

    /// Report whether passengers may alight here.
    pub fn allows_drop_off(&self) -> bool {
        self.drop_off_type != Some(1)
    }

    /// Report whether the vehicle passes without serving the station.
    ///
    /// GTFS has no explicit "pass through" flag. A call that permits
    /// neither boarding nor alighting is the closest equivalent, and
    /// a diagram draws it without a stop marker.
    pub fn is_pass_through(&self) -> bool {
        !self.allows_pickup() && !self.allows_drop_off()
    }
}

/// One run of one train on one service date.
#[derive(Clone, Debug, Serialize)]
pub struct TripInstance {
    /// A stable identifier for this run.
    ///
    /// A fixed trip yields `<date>:<trip_id>`. A frequency-based trip
    /// yields `<date>:<trip_id>@<HH:MM:SS>`, where the time is the
    /// start of that instance. The value is deterministic, so JSON
    /// snapshots stay stable.
    pub instance_id: String,
    /// The GTFS `trip_id` that this run comes from. This is an
    /// internal key, not a passenger-facing train number.
    pub source_trip_id: String,
    /// The service date that the run belongs to.
    pub service_date: ServiceDate,
    /// The GTFS `service_id` of the run.
    pub service_id: String,
    /// The line of the run.
    pub line: LineId,
    /// The stop pattern of the run.
    pub pattern: PatternId,
    /// The GTFS direction. `0` and `1` are opposite directions and
    /// carry no compass or up/down meaning.
    pub direction: Option<u8>,
    /// The trip-level destination text, when the feed supplies one.
    pub headsign: Option<String>,
    /// The public trip name, when the feed supplies one.
    pub short_name: Option<String>,
    /// The vehicle block, when the feed supplies one.
    pub block_id: Option<String>,
    /// Whether the times of this run are exact.
    pub exactness: TimeExactness,
    /// Every call of the run, in `stop_sequence` order.
    pub calls: Vec<ScheduledCall>,
}

impl TripInstance {
    /// Get the first time of the run.
    pub fn first_time(&self) -> Option<GtfsTime> {
        self.calls.iter().find_map(|c| c.arrival_or_departure())
    }

    /// Get the last time of the run.
    pub fn last_time(&self) -> Option<GtfsTime> {
        self.calls
            .iter()
            .rev()
            .find_map(|c| c.departure_or_arrival())
    }

    /// Get the last station of the run.
    pub fn terminus(&self) -> Option<StationId> {
        self.calls.last().map(|c| c.station)
    }

    /// Report whether any call of the run carries a computed time.
    pub fn has_interpolated_times(&self) -> bool {
        self.calls
            .iter()
            .any(|c| c.time_quality == TimeQuality::Interpolated)
    }

    /// Report whether any call of the run has no time at all.
    pub fn has_missing_times(&self) -> bool {
        self.calls
            .iter()
            .any(|c| c.time_quality == TimeQuality::Missing)
    }
}

/// A block of headway-based service that the query did not expand.
///
/// A band says "a train about every `headway_secs` seconds between
/// `start` and `end`". It carries the template calls so that a
/// renderer can draw an envelope, but the template times are offsets
/// of one representative run, not a schedule.
#[derive(Clone, Debug, Serialize)]
pub struct FrequencyBand {
    /// A stable identifier: `<date>:<trip_id>~<start>`.
    pub band_id: String,
    /// The GTFS `trip_id` of the template trip.
    pub source_trip_id: String,
    /// The service date that the band belongs to.
    pub service_date: ServiceDate,
    /// The line of the band.
    pub line: LineId,
    /// The stop pattern of the band.
    pub pattern: PatternId,
    /// The GTFS direction of the template trip.
    pub direction: Option<u8>,
    /// The trip-level destination text, when the feed supplies one.
    pub headsign: Option<String>,
    /// The first departure of the block.
    pub start: GtfsTime,
    /// The end of the block. No run starts at or after this time.
    pub end: GtfsTime,
    /// The time between two runs, in seconds.
    pub headway_secs: u32,
    /// The template calls, at the times of the first run of the block.
    pub template: Vec<ScheduledCall>,
}

impl FrequencyBand {
    /// Get the headway in whole minutes, rounded to the nearest
    /// minute, for display.
    pub fn headway_minutes(&self) -> u32 {
        (self.headway_secs + 30) / 60
    }
}

/// A request for the trips that run on one service date.
///
/// The time window is half-open: `[from, until)`. Adjacent windows
/// therefore never report the same departure twice.
///
/// A run enters the result when its own time span overlaps the
/// window, so a diagram window shows a train that entered the corridor
/// before the window began.
#[derive(Clone, Debug)]
pub struct TripInstanceQuery {
    /// The service date to query.
    pub service_date: ServiceDate,
    /// Keep only the trips of this line.
    pub line: Option<LineId>,
    /// Keep only the trips of this stop pattern.
    pub pattern: Option<PatternId>,
    /// Keep only the trips that call at this station.
    pub station: Option<StationId>,
    /// The start of the window, inclusive.
    pub from: GtfsTime,
    /// The end of the window, exclusive.
    pub until: GtfsTime,
    /// How to treat headway-based service.
    pub frequency_policy: FrequencyPolicy,
    /// How to fill in missing stop times.
    pub missing_time_policy: MissingTimePolicy,
}

impl TripInstanceQuery {
    /// Make a query for a whole service day: `00:00:00` to `28:00:00`.
    ///
    /// The upper bound covers trips that continue past midnight, which
    /// GTFS writes as `24:00:00` and later.
    pub fn new(service_date: ServiceDate) -> Self {
        TripInstanceQuery {
            service_date,
            line: None,
            pattern: None,
            station: None,
            from: GtfsTime::from_seconds(0),
            until: GtfsTime::from_hms(28, 0, 0),
            frequency_policy: FrequencyPolicy::default(),
            missing_time_policy: MissingTimePolicy::default(),
        }
    }

    /// Set the half-open time window.
    pub fn window(mut self, from: GtfsTime, until: GtfsTime) -> Self {
        self.from = from;
        self.until = until;
        self
    }

    /// Keep only the trips of one line.
    pub fn line(mut self, line: LineId) -> Self {
        self.line = Some(line);
        self
    }

    /// Keep only the trips of one stop pattern.
    pub fn pattern(mut self, pattern: PatternId) -> Self {
        self.pattern = Some(pattern);
        self
    }

    /// Keep only the trips that call at one station.
    pub fn station(mut self, station: StationId) -> Self {
        self.station = Some(station);
        self
    }

    /// Set the frequency policy.
    pub fn frequency_policy(mut self, policy: FrequencyPolicy) -> Self {
        self.frequency_policy = policy;
        self
    }

    /// Set the missing-time policy.
    pub fn missing_time_policy(mut self, policy: MissingTimePolicy) -> Self {
        self.missing_time_policy = policy;
        self
    }
}

/// The answer to a [`TripInstanceQuery`].
#[derive(Clone, Debug, Default, Serialize)]
pub struct TripQueryResult {
    /// The runs, sorted by first time, then line, then instance
    /// identifier.
    pub trips: Vec<TripInstance>,
    /// The headway blocks that the policy left unexpanded.
    pub frequency_bands: Vec<FrequencyBand>,
    /// Everything the query could not represent.
    pub diagnostics: Vec<Diagnostic>,
}

impl TripQueryResult {
    /// Report whether any run or band carries approximate times.
    pub fn has_approximate_service(&self) -> bool {
        !self.frequency_bands.is_empty()
            || self
                .trips
                .iter()
                .any(|t| t.exactness == TimeExactness::Approximate)
    }
}

/// The source of the weights that the time interpolation uses.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum WeightSource {
    /// `stop_times.shape_dist_traveled`.
    ShapeDistance,
    /// Great-circle distance between the station positions.
    StationDistance,
    /// The position of the call in the trip.
    CallIndex,
}

impl WeightSource {
    const fn code(self) -> &'static str {
        match self {
            WeightSource::ShapeDistance => "shape_dist_traveled",
            WeightSource::StationDistance => "station distance",
            WeightSource::CallIndex => "call index",
        }
    }
}

impl RailNetwork {
    /// Answer a [`TripInstanceQuery`].
    ///
    /// The function returns an error only when the query cannot be
    /// answered under the selected policy, for example when
    /// [`FrequencyPolicy::RejectNonExact`] meets a non-exact frequency
    /// block. Everything else that goes wrong becomes a
    /// [`Diagnostic`].
    pub fn query_trip_instances(
        &self,
        query: &TripInstanceQuery,
    ) -> Result<TripQueryResult, GtfsError> {
        let mut result = TripQueryResult::default();
        for trip in &self.trips {
            if !self.services.active(trip.service, query.service_date) {
                continue;
            }
            if query.line.is_some_and(|line| trip.line != line) {
                continue;
            }
            if query.pattern.is_some_and(|pattern| trip.pattern != pattern) {
                continue;
            }
            if let Some(station) = query.station {
                if !self.pattern(trip.pattern).stations.contains(&station) {
                    continue;
                }
            }
            self.collect_trip(trip, query, &mut result)?;
        }

        result.trips.sort_by(|a, b| {
            (a.first_time(), a.line, a.instance_id.as_str()).cmp(&(
                b.first_time(),
                b.line,
                b.instance_id.as_str(),
            ))
        });
        result.frequency_bands.sort_by(|a, b| {
            (a.start, a.line, a.band_id.as_str()).cmp(&(b.start, b.line, b.band_id.as_str()))
        });
        crate::diag::normalize(&mut result.diagnostics);
        Ok(result)
    }

    /// Turn one stored trip into instances, bands, and diagnostics.
    fn collect_trip(
        &self,
        trip: &TripSchedule,
        query: &TripInstanceQuery,
        out: &mut TripQueryResult,
    ) -> Result<(), GtfsError> {
        let stations = &self.pattern(trip.pattern).stations;
        let Some(calls) = self.resolve_calls(trip, stations, query, &mut out.diagnostics) else {
            return Ok(());
        };

        if trip.frequencies.is_empty() {
            let instance = self.make_instance(
                trip,
                query.service_date,
                format!("{}:{}", query.service_date, trip.trip_id),
                TimeExactness::Exact,
                calls,
            );
            if overlaps(&instance, query) {
                out.trips.push(instance);
            }
            return Ok(());
        }

        // A frequency-based trip is a template. Its stop times are
        // offsets from the first departure of each run.
        let Some(anchor) = calls.iter().find_map(|c| c.arrival_or_departure()) else {
            out.diagnostics.push(
                Diagnostic::warning(
                    "frequency-template-without-times",
                    "the template trip of a frequency block has no usable time",
                )
                .about(trip.trip_id.clone()),
            );
            return Ok(());
        };

        for block in &trip.frequencies {
            if let Some(diagnostic) = invalid_block(trip, block) {
                out.diagnostics.push(diagnostic);
                continue;
            }
            let expand =
                block.is_exact() || query.frequency_policy == FrequencyPolicy::ExpandApproximate;
            if !block.is_exact() {
                match query.frequency_policy {
                    FrequencyPolicy::RejectNonExact => {
                        return Err(GtfsError::PolicyViolation(format!(
                            "trip \"{}\" runs on a non-exact headway ({}-{}), \
                             and the frequency policy rejects it",
                            trip.trip_id, block.start_time, block.end_time
                        )));
                    }
                    FrequencyPolicy::ExpandApproximate => out.diagnostics.push(
                        Diagnostic::info(
                            "frequency-expanded-approximate",
                            format!(
                                "expanded the non-exact headway block {}-{} into approximate runs",
                                block.start_time, block.end_time
                            ),
                        )
                        .about(trip.trip_id.clone()),
                    ),
                    FrequencyPolicy::Bands => {}
                }
            }

            if expand {
                let exactness = if block.is_exact() {
                    TimeExactness::Exact
                } else {
                    TimeExactness::Approximate
                };
                let mut start = block.start_time.seconds();
                while start < block.end_time.seconds() {
                    let shifted = shift_calls(&calls, anchor.seconds(), start);
                    let instance = self.make_instance(
                        trip,
                        query.service_date,
                        format!(
                            "{}:{}@{}",
                            query.service_date,
                            trip.trip_id,
                            GtfsTime::from_seconds(start)
                        ),
                        exactness,
                        shifted,
                    );
                    if overlaps(&instance, query) {
                        out.trips.push(instance);
                    }
                    start += block.headway_secs;
                }
            } else {
                // Bands: report the block, do not invent runs.
                if block.start_time < query.until && block.end_time > query.from {
                    out.frequency_bands.push(FrequencyBand {
                        band_id: format!(
                            "{}:{}~{}",
                            query.service_date, trip.trip_id, block.start_time
                        ),
                        source_trip_id: trip.trip_id.clone(),
                        service_date: query.service_date,
                        line: trip.line,
                        pattern: trip.pattern,
                        direction: trip.direction,
                        headsign: trip.headsign.clone(),
                        start: block.start_time,
                        end: block.end_time,
                        headway_secs: block.headway_secs,
                        template: shift_calls(&calls, anchor.seconds(), block.start_time.seconds()),
                    });
                }
            }
        }
        Ok(())
    }

    fn make_instance(
        &self,
        trip: &TripSchedule,
        service_date: ServiceDate,
        instance_id: String,
        exactness: TimeExactness,
        calls: Vec<ScheduledCall>,
    ) -> TripInstance {
        TripInstance {
            instance_id,
            source_trip_id: trip.trip_id.clone(),
            service_date,
            service_id: trip.service_id.clone(),
            line: trip.line,
            pattern: trip.pattern,
            direction: trip.direction,
            headsign: trip.headsign.clone(),
            short_name: trip.short_name.clone(),
            block_id: trip.block_id.clone(),
            exactness,
            calls,
        }
    }

    /// Build the calls of one trip, filling in missing times under the
    /// query policy.
    ///
    /// Returns `None` when the trip carries no usable time at all.
    fn resolve_calls(
        &self,
        trip: &TripSchedule,
        stations: &[StationId],
        query: &TripInstanceQuery,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<Vec<ScheduledCall>> {
        let raw = &trip.calls;
        if raw.iter().all(|c| c.arrival_or_departure().is_none()) {
            diagnostics.push(
                Diagnostic::warning(
                    "trip-without-times",
                    "the trip carries no arrival and no departure time, so it cannot be drawn",
                )
                .about(trip.trip_id.clone()),
            );
            return None;
        }

        let (weights, source) = self.interpolation_weights(raw, stations);
        let filled = fill_missing_times(raw, &weights, query.missing_time_policy);

        let interpolated = filled
            .iter()
            .filter(|slot| slot.quality == TimeQuality::Interpolated)
            .count();
        if interpolated > 0 {
            diagnostics.push(
                Diagnostic::info(
                    "time-interpolated",
                    format!(
                        "computed {interpolated} missing stop time(s) from the {} of the trip",
                        source.code()
                    ),
                )
                .about(trip.trip_id.clone()),
            );
        }
        let missing = filled
            .iter()
            .filter(|slot| slot.quality == TimeQuality::Missing)
            .count();
        if missing > 0 {
            diagnostics.push(
                Diagnostic::warning(
                    "time-missing",
                    format!(
                        "{missing} call(s) have no time and lie outside the known times \
                         of the trip, so the run is drawn only in part"
                    ),
                )
                .about(trip.trip_id.clone()),
            );
        }

        Some(
            raw.iter()
                .zip(stations.iter())
                .zip(filled.iter())
                .map(|((call, &station), slot)| ScheduledCall {
                    station,
                    platform_stop_id: call.stop_id.clone(),
                    platform_code: call.platform_code.clone(),
                    arrival: slot.arrival,
                    departure: slot.departure,
                    stop_headsign: call.stop_headsign.clone(),
                    pickup_type: call.pickup_type,
                    drop_off_type: call.drop_off_type,
                    time_quality: slot.quality,
                })
                .collect(),
        )
    }

    /// Pick the best available weights for time interpolation.
    ///
    /// The order follows the specification: `shape_dist_traveled`
    /// first, then the great-circle distance between the stations,
    /// then the position of the call in the trip.
    fn interpolation_weights(
        &self,
        calls: &[StopCall],
        stations: &[StationId],
    ) -> (Vec<f64>, WeightSource) {
        let shape: Option<Vec<f64>> = calls.iter().map(|c| c.shape_dist_traveled).collect();
        if let Some(values) = shape {
            if values.windows(2).all(|w| w[1] >= w[0]) && values.first() < values.last() {
                return (values, WeightSource::ShapeDistance);
            }
        }

        let positions: Option<Vec<(f64, f64)>> = stations
            .iter()
            .map(|&id| {
                let station = self.station(id);
                station.lat.zip(station.lon)
            })
            .collect();
        if let Some(positions) = positions {
            let mut cumulative = Vec::with_capacity(positions.len());
            let mut total = 0.0;
            cumulative.push(0.0);
            for pair in positions.windows(2) {
                total += great_circle_metres(pair[0], pair[1]);
                cumulative.push(total);
            }
            if total > 0.0 {
                return (cumulative, WeightSource::StationDistance);
            }
        }

        (
            (0..calls.len()).map(|i| i as f64).collect(),
            WeightSource::CallIndex,
        )
    }

    /// Get the cumulative great-circle distance along a sequence of
    /// stations, in metres.
    ///
    /// The result is `None` when a station has no position. A diagram
    /// with distance-proportional station spacing needs this value.
    pub fn cumulative_station_distance(&self, stations: &[StationId]) -> Option<Vec<f64>> {
        let positions: Option<Vec<(f64, f64)>> = stations
            .iter()
            .map(|&id| {
                let station = self.station(id);
                station.lat.zip(station.lon)
            })
            .collect();
        let positions = positions?;
        let mut cumulative = Vec::with_capacity(positions.len());
        let mut total = 0.0;
        cumulative.push(0.0);
        for pair in positions.windows(2) {
            total += great_circle_metres(pair[0], pair[1]);
            cumulative.push(total);
        }
        Some(cumulative)
    }
}

/// One filled-in time slot of a call.
#[derive(Copy, Clone, Debug)]
struct TimeSlot {
    arrival: Option<GtfsTime>,
    departure: Option<GtfsTime>,
    quality: TimeQuality,
}

/// Fill in the missing times of a trip.
///
/// The function interpolates a missing time between the two nearest
/// known times, weighted by `weights`. Under
/// [`MissingTimePolicy::InterpolateBounded`] a gap before the first or
/// after the last known time stays missing.
fn fill_missing_times(
    calls: &[StopCall],
    weights: &[f64],
    policy: MissingTimePolicy,
) -> Vec<TimeSlot> {
    let mut slots: Vec<TimeSlot> = calls
        .iter()
        .map(|call| TimeSlot {
            arrival: call.arrival,
            departure: call.departure,
            quality: match call.arrival_or_departure() {
                None => TimeQuality::Missing,
                Some(_) if call.timepoint == Some(0) => TimeQuality::Approximate,
                Some(_) => TimeQuality::Exact,
            },
        })
        .collect();
    if policy == MissingTimePolicy::None {
        return slots;
    }

    let known: Vec<usize> = slots
        .iter()
        .enumerate()
        .filter(|(_, slot)| slot.quality != TimeQuality::Missing)
        .map(|(i, _)| i)
        .collect();
    if known.len() < 2 {
        return slots;
    }

    // Interior gaps: interpolate between the bounding known calls.
    for pair in known.windows(2) {
        let (before, after) = (pair[0], pair[1]);
        if after == before + 1 {
            continue;
        }
        let start = slots[before].departure.or(slots[before].arrival).unwrap();
        let end = slots[after].arrival.or(slots[after].departure).unwrap();
        for (offset, slot) in slots[(before + 1)..after].iter_mut().enumerate() {
            let index = before + 1 + offset;
            let fraction = fraction_between(weights, before, after, index);
            let seconds =
                start.seconds() as f64 + (end.seconds() as f64 - start.seconds() as f64) * fraction;
            let time = GtfsTime::from_seconds(seconds.round().max(0.0) as u32);
            *slot = TimeSlot {
                arrival: Some(time),
                departure: Some(time),
                quality: TimeQuality::Interpolated,
            };
        }
    }

    if policy == MissingTimePolicy::InterpolateUnbounded {
        extrapolate_edges(&mut slots, weights, &known);
    }
    slots
}

/// Extend the first and the last known time outwards.
///
/// The rate comes from the nearest known pair. This invents times, so
/// only [`MissingTimePolicy::InterpolateUnbounded`] calls it.
fn extrapolate_edges(slots: &mut [TimeSlot], weights: &[f64], known: &[usize]) {
    let (first, second) = (known[0], known[1]);
    if first > 0 {
        let t0 = slots[first].arrival_seconds();
        let t1 = slots[second].arrival_seconds();
        for index in (0..first).rev() {
            let fraction = fraction_between(weights, first, second, index);
            let seconds = t0 + (t1 - t0) * fraction;
            let time = GtfsTime::from_seconds(seconds.round().max(0.0) as u32);
            slots[index] = TimeSlot {
                arrival: Some(time),
                departure: Some(time),
                quality: TimeQuality::Interpolated,
            };
        }
    }
    let (last, before_last) = (known[known.len() - 1], known[known.len() - 2]);
    if last + 1 < slots.len() {
        let t0 = slots[before_last].arrival_seconds();
        let t1 = slots[last].arrival_seconds();
        let tail = last + 1;
        for (offset, slot) in slots[tail..].iter_mut().enumerate() {
            let fraction = fraction_between(weights, before_last, last, tail + offset);
            let seconds = t0 + (t1 - t0) * fraction;
            let time = GtfsTime::from_seconds(seconds.round().max(0.0) as u32);
            *slot = TimeSlot {
                arrival: Some(time),
                departure: Some(time),
                quality: TimeQuality::Interpolated,
            };
        }
    }
}

impl TimeSlot {
    fn arrival_seconds(&self) -> f64 {
        self.arrival
            .or(self.departure)
            .map(|t| t.seconds() as f64)
            .unwrap_or(0.0)
    }
}

/// Get the position of `index` between `before` and `after` on the
/// weight axis, as a fraction.
///
/// The function falls back to the index proportion when the weights
/// are degenerate, so a feed with equal distances still interpolates.
fn fraction_between(weights: &[f64], before: usize, after: usize, index: usize) -> f64 {
    let span = weights[after] - weights[before];
    if span.abs() > f64::EPSILON {
        (weights[index] - weights[before]) / span
    } else {
        (index as f64 - before as f64) / (after as f64 - before as f64)
    }
}

/// Shift the template calls of a frequency block to a new start time.
fn shift_calls(calls: &[ScheduledCall], anchor_secs: u32, start_secs: u32) -> Vec<ScheduledCall> {
    let shift = |time: Option<GtfsTime>| -> Option<GtfsTime> {
        time.map(|t| {
            GtfsTime::from_seconds(
                (i64::from(t.seconds()) - i64::from(anchor_secs) + i64::from(start_secs)).max(0)
                    as u32,
            )
        })
    };
    calls
        .iter()
        .map(|call| ScheduledCall {
            arrival: shift(call.arrival),
            departure: shift(call.departure),
            ..call.clone()
        })
        .collect()
}

/// Report whether the run overlaps the half-open query window.
fn overlaps(instance: &TripInstance, query: &TripInstanceQuery) -> bool {
    let (Some(first), Some(last)) = (instance.first_time(), instance.last_time()) else {
        return false;
    };
    first < query.until && last >= query.from
}

/// Reject a frequency block that cannot produce runs.
fn invalid_block(trip: &TripSchedule, block: &Frequency) -> Option<Diagnostic> {
    if block.headway_secs == 0 {
        return Some(
            Diagnostic::error(
                "frequency-zero-headway",
                format!(
                    "the headway of the block {}-{} is zero, so it describes no service",
                    block.start_time, block.end_time
                ),
            )
            .about(trip.trip_id.clone()),
        );
    }
    if block.end_time <= block.start_time {
        return Some(
            Diagnostic::error(
                "frequency-empty-block",
                format!(
                    "the block {}-{} ends at or before its start, so it describes no service",
                    block.start_time, block.end_time
                ),
            )
            .about(trip.trip_id.clone()),
        );
    }
    None
}

/// The mean radius of the Earth, in metres.
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Get the great-circle distance between two positions, in metres.
///
/// The function uses the haversine formula. Station spacing on an
/// urban rail network is a few kilometres, where the formula is
/// accurate well beyond the needs of a diagram axis.
fn great_circle_metres(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lon1) = (a.0.to_radians(), a.1.to_radians());
    let (lat2, lon2) = (b.0.to_radians(), b.1.to_radians());
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().clamp(-1.0, 1.0).asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(arrival: Option<&str>, departure: Option<&str>) -> StopCall {
        StopCall {
            arrival: arrival.map(|s| s.parse().unwrap()),
            departure: departure.map(|s| s.parse().unwrap()),
            stop_id: "S".to_string(),
            platform_code: None,
            stop_headsign: None,
            pickup_type: None,
            drop_off_type: None,
            timepoint: None,
            shape_dist_traveled: None,
        }
    }

    #[test]
    fn bounded_interpolation_fills_only_interior_gaps() {
        let calls = [
            call(None, None),
            call(Some("06:00:00"), Some("06:00:00")),
            call(None, None),
            call(None, None),
            call(Some("06:30:00"), Some("06:30:00")),
            call(None, None),
        ];
        let weights: Vec<f64> = (0..6).map(|i| i as f64).collect();
        let slots = fill_missing_times(&calls, &weights, MissingTimePolicy::InterpolateBounded);

        assert_eq!(slots[0].quality, TimeQuality::Missing);
        assert_eq!(slots[5].quality, TimeQuality::Missing);
        assert_eq!(slots[2].quality, TimeQuality::Interpolated);
        assert_eq!(slots[2].departure.unwrap().to_string(), "06:10:00");
        assert_eq!(slots[3].departure.unwrap().to_string(), "06:20:00");
    }

    #[test]
    fn unbounded_interpolation_extends_the_edges() {
        let calls = [
            call(None, None),
            call(Some("06:00:00"), Some("06:00:00")),
            call(Some("06:20:00"), Some("06:20:00")),
            call(None, None),
        ];
        let weights: Vec<f64> = (0..4).map(|i| i as f64).collect();
        let slots = fill_missing_times(&calls, &weights, MissingTimePolicy::InterpolateUnbounded);
        assert_eq!(slots[0].departure.unwrap().to_string(), "05:40:00");
        assert_eq!(slots[3].departure.unwrap().to_string(), "06:40:00");
    }

    #[test]
    fn the_none_policy_leaves_gaps_alone() {
        let calls = [
            call(Some("06:00:00"), Some("06:00:00")),
            call(None, None),
            call(Some("06:30:00"), Some("06:30:00")),
        ];
        let weights: Vec<f64> = (0..3).map(|i| i as f64).collect();
        let slots = fill_missing_times(&calls, &weights, MissingTimePolicy::None);
        assert_eq!(slots[1].quality, TimeQuality::Missing);
    }

    #[test]
    fn distance_weights_bend_the_interpolation() {
        // The middle station sits close to the first one, so its
        // interpolated time is close to the first time too.
        let calls = [
            call(Some("06:00:00"), Some("06:00:00")),
            call(None, None),
            call(Some("06:40:00"), Some("06:40:00")),
        ];
        let weights = vec![0.0, 1.0, 5.0];
        let slots = fill_missing_times(&calls, &weights, MissingTimePolicy::InterpolateBounded);
        assert_eq!(slots[1].departure.unwrap().to_string(), "06:08:00");
    }

    #[test]
    fn a_timepoint_zero_call_is_approximate() {
        let mut c = call(Some("06:00:00"), Some("06:00:00"));
        c.timepoint = Some(0);
        let slots = fill_missing_times(&[c], &[0.0], MissingTimePolicy::InterpolateBounded);
        assert_eq!(slots[0].quality, TimeQuality::Approximate);
    }

    #[test]
    fn great_circle_distance_matches_a_known_pair() {
        // Jurong East to Choa Chu Kang, about 5.8 km apart.
        let metres = great_circle_metres((1.3331, 103.7422), (1.3854, 103.7443));
        assert!((metres - 5_820.0).abs() < 60.0, "got {metres} m");
        assert_eq!(great_circle_metres((1.0, 1.0), (1.0, 1.0)), 0.0);
    }

    #[test]
    fn headway_minutes_round_to_the_nearest_minute() {
        let band = FrequencyBand {
            band_id: String::new(),
            source_trip_id: String::new(),
            service_date: "20260810".parse().unwrap(),
            line: LineId(0),
            pattern: PatternId(0),
            direction: None,
            headsign: None,
            start: GtfsTime::from_seconds(0),
            end: GtfsTime::from_seconds(600),
            headway_secs: 250,
            template: Vec::new(),
        };
        assert_eq!(band.headway_minutes(), 4);
    }
}
