//! The live map view model.
//!
//! A [`NetworkSnapshot`] is the whole rail network at one instant: the
//! lines, the edges between neighbouring stations, the runs placed on
//! those edges, the headway bands that carry no individual runs, and a
//! freshness record for the realtime layer.
//!
//! # There are no vehicle positions
//!
//! LTA DataMall publishes no `VehiclePositions` feed, so every position
//! on the map is derived. It comes from three steps:
//!
//! 1. the scheduled trajectory of a run, from
//!    [`RailNetwork::query_trip_instances`];
//! 2. a shift from the GTFS-Realtime trip update for the same
//!    `trip_id` — the per-stop events where the feed supplies them,
//!    otherwise the trip-level delay;
//! 3. linear interpolation between the adjusted departure at the
//!    station behind and the adjusted arrival at the station ahead.
//!
//! The third step invents the shape of the motion: a real train
//! accelerates and brakes, the interpolation travels at a constant
//! fraction per second. So every position carries a
//! [`PositionQuality`], the way a stop time carries a
//! [`TimeQuality`], and a renderer draws an estimate as an estimate.
//!
//! Nothing here is a measurement of where a train is. "Live" means the
//! schedule, corrected by the most recent prediction the operator
//! published, evaluated at the clock the caller passes in.
//!
//! # No input, no output, no clock
//!
//! The builder reads no clock and touches no network. The caller
//! fetches the feeds and passes the service date, the clock, and the
//! realtime `now_unix` in, exactly as [`crate::LiveBoardBuilder`] does.
//! The same inputs always produce the same snapshot, byte for byte.

use serde::Serialize;

use mrt_datamall::TrainServiceAlerts;
use mrt_gtfs::{
    normalize_diagnostics, Diagnostic, FrequencyPolicy, GtfsTime, Line, LineId, MissingTimePolicy,
    PatternId, RailNetwork, ScheduledCall, ServiceDate, StationId, TimeExactness, TimeQuality,
    TripInstance, TripInstanceQuery, TripQueryResult,
};
use mrt_gtfs_rt::{RailRtFeed, TripUpdate};

use crate::{match_train_line, LineState};

/// The default half-width of the query window, in seconds.
///
/// A run enters the snapshot when its own span overlaps
/// `clock ± window`. One hour covers a metro run from end to end.
const DEFAULT_WINDOW_SECS: u32 = 3600;

/// The default staleness threshold, in seconds.
///
/// The real cadence of the LTA realtime feed is unmeasured, which
/// `docs/LIVE-MAP-POC.md` records as an open question of phase 0. Two
/// minutes is a placeholder; a caller that has measured the feed sets
/// its own value with [`NetworkSnapshotBuilder::staleness_secs`].
const DEFAULT_STALENESS_SECS: u32 = 120;

// ----------------------------------------------------------------------
// Provenance
// ----------------------------------------------------------------------

/// Where the position of one train comes from.
///
/// The enumeration mirrors [`TimeQuality`]: a small, closed set of
/// provenance markers, never a confidence score. A renderer switches on
/// it to choose the treatment of a train.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PositionQuality {
    /// The run stands at a station, and a realtime trip update
    /// confirms the timing. This is the strongest claim the data
    /// supports.
    AtStation,
    /// The run lies between two stations, and both bracketing times
    /// come from the feed and carry a realtime shift.
    InterpolatedRealtime,
    /// The run lies between two stations, and at least one bracketing
    /// time was computed by `mrt-gtfs` or is marked approximate by the
    /// feed. The schedule itself is an estimate here.
    InterpolatedSchedule,
    /// No realtime update applies: the feed carries none for this run,
    /// there is no realtime layer at all, or the layer is stale. The
    /// position comes from the schedule alone.
    ScheduleOnly,
}

/// How current the realtime layer of a snapshot is.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FreshnessState {
    /// The realtime feed is newer than the staleness threshold.
    Live,
    /// The realtime feed is older than the staleness threshold, or it
    /// carries no timestamp at all. Positions come from the schedule.
    Stale,
    /// The caller supplied no realtime layer.
    Unavailable,
}

/// The freshness of the realtime layer.
#[derive(Debug, Clone, Serialize)]
pub struct Freshness {
    /// The service date of the snapshot.
    pub service_date: ServiceDate,
    /// The clock that the snapshot was evaluated at, on the service
    /// day.
    pub clock: GtfsTime,
    /// The timestamp of the realtime feed, in POSIX seconds.
    pub feed_timestamp: Option<u64>,
    /// The POSIX time the caller passed with the realtime layer.
    pub now_unix: Option<u64>,
    /// The age of the realtime feed, in seconds.
    pub age_secs: Option<u64>,
    /// The threshold above which the layer counts as stale, in
    /// seconds.
    pub staleness_secs: u32,
    /// The resulting state.
    pub state: FreshnessState,
}

// ----------------------------------------------------------------------
// The network
// ----------------------------------------------------------------------

/// One line of the network.
#[derive(Debug, Clone, Serialize)]
pub struct MapLine {
    /// The line.
    pub line: LineId,
    /// The GTFS route identifier.
    pub route_id: String,
    /// The display name, for example `NSL`.
    pub name: String,
    /// The long name, for example `North South Line`.
    pub long_name: Option<String>,
    /// The line colour as a six-digit hexadecimal value, as the feed
    /// supplies it. A renderer filters it before it reaches a
    /// stylesheet.
    pub color: Option<String>,
    /// The live state of the line.
    pub state: LineState,
}

/// One station of the network.
///
/// The table gives the [`StationId`] values of the edges and the trains
/// a meaning, so a renderer never reaches back into the
/// [`RailNetwork`]. It carries no position: the map is schematic, and
/// the layout supplies the geometry.
#[derive(Debug, Clone, Serialize)]
pub struct MapStation {
    /// The station.
    pub station: StationId,
    /// The public name, for example `Jurong East`.
    pub name: String,
    /// The public station codes, for example `NS1` and `EW24`.
    pub codes: Vec<String>,
}

/// One edge of the network: the section between two neighbouring
/// stations of one stop pattern.
#[derive(Debug, Clone, Serialize)]
pub struct MapEdge {
    /// The stop pattern that the edge belongs to.
    pub pattern: PatternId,
    /// The index of the edge in the pattern. The edge runs from
    /// station `index` to station `index + 1`.
    pub index: usize,
    /// The line of the pattern.
    pub line: LineId,
    /// The station behind.
    pub from: StationId,
    /// The station ahead.
    pub to: StationId,
}

/// Where one train sits on the network.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TrainLocation {
    /// The run stands at a station.
    AtStation {
        /// The station.
        station: StationId,
        /// The stop pattern of the run.
        pattern: PatternId,
        /// The index of the station in the pattern. A loop pattern
        /// visits a station twice, and the index tells the two calls
        /// apart.
        index: usize,
    },
    /// The run lies between two stations.
    OnEdge {
        /// The stop pattern of the run.
        pattern: PatternId,
        /// The index of the edge in the pattern.
        index: usize,
        /// The station behind.
        from: StationId,
        /// The station ahead.
        to: StationId,
    },
}

/// One train on the map.
#[derive(Debug, Clone, Serialize)]
pub struct MapTrain {
    /// The stable identifier of the run, from
    /// [`TripInstance::instance_id`].
    pub instance_id: String,
    /// The GTFS `trip_id` that the run comes from. This is an internal
    /// key, not a passenger-facing train number.
    pub source_trip_id: String,
    /// The line of the run.
    pub line: LineId,
    /// Where the run sits.
    pub location: TrainLocation,
    /// How far along the edge the run has travelled, from 0 to 1. The
    /// value is 0 for a run standing at a station.
    pub progress: f64,
    /// The adjusted scheduled time from the departure behind to the
    /// arrival ahead, in seconds, for a run that lies on an edge.
    ///
    /// It is the bound on local motion. A renderer that advances a
    /// train between two polls knows from `progress` and this value
    /// when the run is due at the station ahead, and must stop there
    /// rather than extrapolate past it. A run standing at a station
    /// carries `None`.
    pub edge_secs: Option<u32>,
    /// The destination text.
    pub destination: String,
    /// Where the position comes from.
    pub quality: PositionQuality,
    /// The live delay in seconds, where a trip update reports one. A
    /// positive value means late.
    pub delay_secs: Option<i32>,
    /// `true` when a scheduled time that brackets the position was
    /// computed by `mrt-gtfs` or marked approximate by the feed, so a
    /// tooltip can say that the schedule itself is an estimate.
    pub schedule_interpolated: bool,
}

/// One headway block that the snapshot did not expand into trains.
///
/// A block with `exact_times=0` says "a train about every N minutes".
/// The individual runs do not exist, so the map draws none of them and
/// the line carries the band label instead.
#[derive(Debug, Clone, Serialize)]
pub struct MapBand {
    /// The stable identifier of the block.
    pub band_id: String,
    /// The GTFS `trip_id` of the template trip.
    pub source_trip_id: String,
    /// The line of the block.
    pub line: LineId,
    /// The stop pattern of the block.
    pub pattern: PatternId,
    /// The destination text, when the feed supplies one.
    pub destination: Option<String>,
    /// The first departure of the block.
    pub start: GtfsTime,
    /// The end of the block. No run starts at or after this time.
    pub end: GtfsTime,
    /// The time between two runs, in seconds.
    pub headway_secs: u32,
    /// The same headway in whole minutes, for a label.
    pub headway_minutes: u32,
}

/// The whole network at one instant.
#[derive(Debug, Clone, Serialize)]
pub struct NetworkSnapshot {
    /// Every line, in identifier order.
    pub lines: Vec<MapLine>,
    /// Every station, in identifier order.
    pub stations: Vec<MapStation>,
    /// Every edge, in pattern order and then pattern index order.
    pub edges: Vec<MapEdge>,
    /// The placed runs, in `instance_id` order.
    pub trains: Vec<MapTrain>,
    /// The headway blocks that carry no individual runs, in `band_id`
    /// order.
    pub bands: Vec<MapBand>,
    /// The freshness of the realtime layer.
    pub freshness: Freshness,
    /// Everything the snapshot could not represent: every run that
    /// could not be placed says why.
    pub diagnostics: Vec<Diagnostic>,
}

// ----------------------------------------------------------------------
// The builder
// ----------------------------------------------------------------------

/// A builder that merges the network, the realtime feed, and the
/// alerts into a [`NetworkSnapshot`].
///
/// Every live layer is optional. Without them the snapshot is the
/// schedule, drawn at the clock the caller passes in, and every train
/// carries [`PositionQuality::ScheduleOnly`].
///
/// # Example
///
/// ```no_run
/// use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork};
/// use mrt_gtfs_rt::RailRtFeed;
/// use mrt_live::NetworkSnapshotBuilder;
///
/// let network = RailNetwork::from_feed(&GtfsFeed::from_dir("feed").unwrap()).unwrap();
/// let realtime = RailRtFeed::decode(&std::fs::read("trip-updates.pb").unwrap()).unwrap();
///
/// let snapshot = NetworkSnapshotBuilder::new(&network)
///     .with_realtime(&realtime, 1_754_000_000)
///     .build("20260810".parse().unwrap(), GtfsTime::from_hms(8, 0, 0));
/// println!("{} trains", snapshot.trains.len());
/// ```
pub struct NetworkSnapshotBuilder<'a> {
    network: &'a RailNetwork,
    realtime: Option<&'a RailRtFeed>,
    now_unix: Option<u64>,
    alerts: Option<&'a TrainServiceAlerts>,
    staleness_secs: u32,
    window_secs: u32,
    missing_time_policy: MissingTimePolicy,
}

impl<'a> NetworkSnapshotBuilder<'a> {
    /// Make a builder for the given network.
    pub fn new(network: &'a RailNetwork) -> Self {
        NetworkSnapshotBuilder {
            network,
            realtime: None,
            now_unix: None,
            alerts: None,
            staleness_secs: DEFAULT_STALENESS_SECS,
            window_secs: DEFAULT_WINDOW_SECS,
            missing_time_policy: MissingTimePolicy::default(),
        }
    }

    /// Add a decoded GTFS-Realtime feed with trip updates.
    ///
    /// `now_unix` is the POSIX time to measure the age of the feed
    /// against. The builder reads no clock, so the caller supplies it.
    pub fn with_realtime(mut self, realtime: &'a RailRtFeed, now_unix: u64) -> Self {
        self.realtime = Some(realtime);
        self.now_unix = Some(now_unix);
        self
    }

    /// Add the legacy train service alerts.
    ///
    /// They set the state of the affected lines, exactly as
    /// [`crate::NetworkStatus::from_alerts`] does.
    pub fn with_alerts(mut self, alerts: &'a TrainServiceAlerts) -> Self {
        self.alerts = Some(alerts);
        self
    }

    /// Set the staleness threshold, in seconds. The default is 120.
    ///
    /// A realtime feed older than this degrades the whole snapshot to
    /// the schedule-only treatment.
    pub fn staleness_secs(mut self, seconds: u32) -> Self {
        self.staleness_secs = seconds;
        self
    }

    /// Set the half-width of the query window, in seconds. The default
    /// is 3600.
    ///
    /// A run reaches the snapshot when its own span overlaps
    /// `clock ± window`. The value must exceed the length of the
    /// longest run, or a train that entered the line before the window
    /// began is missing from the map.
    pub fn window_secs(mut self, seconds: u32) -> Self {
        self.window_secs = seconds;
        self
    }

    /// Set how the query fills in stop times that the feed leaves
    /// empty. The default is
    /// [`MissingTimePolicy::InterpolateBounded`], the default of the
    /// query itself.
    ///
    /// Under [`MissingTimePolicy::None`] a call without a time keeps
    /// [`TimeQuality::Missing`], and a run whose bracketing calls are
    /// missing is not placed and leaves a diagnostic.
    pub fn missing_time_policy(mut self, policy: MissingTimePolicy) -> Self {
        self.missing_time_policy = policy;
        self
    }

    /// Build the snapshot for one service date and one clock reading.
    pub fn build(&self, date: ServiceDate, clock: GtfsTime) -> NetworkSnapshot {
        let mut diagnostics = Vec::new();
        let freshness = self.freshness(date, clock, &mut diagnostics);
        let live = freshness.state == FreshnessState::Live;

        let query = TripInstanceQuery::new(date)
            .window(
                GtfsTime::from_seconds(clock.seconds().saturating_sub(self.window_secs)),
                GtfsTime::from_seconds(clock.seconds().saturating_add(self.window_secs)),
            )
            .frequency_policy(FrequencyPolicy::Bands)
            .missing_time_policy(self.missing_time_policy);
        let result = match self.network.query_trip_instances(&query) {
            Ok(result) => result,
            Err(error) => {
                diagnostics.push(Diagnostic::error("map-query-failed", error.to_string()));
                TripQueryResult::default()
            }
        };
        diagnostics.extend(result.diagnostics.iter().cloned());

        let mut trains: Vec<MapTrain> = result
            .trips
            .iter()
            .filter_map(|trip| self.place(trip, clock, live, &mut diagnostics))
            .collect();
        trains.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));

        let mut bands: Vec<MapBand> = result
            .frequency_bands
            .iter()
            .map(|band| MapBand {
                band_id: band.band_id.clone(),
                source_trip_id: band.source_trip_id.clone(),
                line: band.line,
                pattern: band.pattern,
                destination: band.headsign.clone(),
                start: band.start,
                end: band.end,
                headway_secs: band.headway_secs,
                headway_minutes: band.headway_minutes(),
            })
            .collect();
        bands.sort_by(|a, b| a.band_id.cmp(&b.band_id));

        normalize_diagnostics(&mut diagnostics);
        NetworkSnapshot {
            lines: self.lines(),
            stations: self.stations(),
            edges: self.edges(),
            trains,
            bands,
            freshness,
            diagnostics,
        }
    }

    // ------------------------------------------------------------------
    // The static layers
    // ------------------------------------------------------------------

    /// Build the line table, in identifier order.
    fn lines(&self) -> Vec<MapLine> {
        self.network
            .lines()
            .iter()
            .enumerate()
            .map(|(index, line)| MapLine {
                line: LineId(index),
                route_id: line.route_id.clone(),
                name: line.name.clone(),
                long_name: line.long_name.clone(),
                color: line.color.clone(),
                state: self.line_state(line),
            })
            .collect()
    }

    /// Build the station table, in identifier order.
    fn stations(&self) -> Vec<MapStation> {
        self.network
            .stations()
            .iter()
            .enumerate()
            .map(|(index, station)| MapStation {
                station: StationId(index),
                name: station.name.clone(),
                codes: station.codes.clone(),
            })
            .collect()
    }

    /// Build the edge table, in pattern order and then pattern index
    /// order.
    fn edges(&self) -> Vec<MapEdge> {
        let mut edges = Vec::new();
        for (index, pattern) in self.network.patterns().iter().enumerate() {
            for (position, pair) in pattern.stations.windows(2).enumerate() {
                edges.push(MapEdge {
                    pattern: PatternId(index),
                    index: position,
                    line: pattern.line,
                    from: pair[0],
                    to: pair[1],
                });
            }
        }
        edges
    }

    /// Get the state of one line from the legacy alerts.
    fn line_state(&self, line: &Line) -> LineState {
        let Some(alerts) = self.alerts else {
            return LineState::Normal;
        };
        let Some(train_line) = match_train_line(line) else {
            return LineState::Normal;
        };
        for segment in &alerts.affected_segments {
            if segment.train_line() == Some(train_line) {
                return LineState::Disrupted {
                    stations: segment.station_codes(),
                    direction: segment.direction.clone(),
                    free_public_bus: segment.free_public_bus_codes(),
                };
            }
        }
        LineState::Normal
    }

    // ------------------------------------------------------------------
    // Freshness
    // ------------------------------------------------------------------

    /// Judge the age of the realtime layer.
    ///
    /// A feed without a timestamp counts as stale: the snapshot cannot
    /// tell how old it is, and the honest answer is the schedule.
    fn freshness(
        &self,
        date: ServiceDate,
        clock: GtfsTime,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Freshness {
        let mut freshness = Freshness {
            service_date: date,
            clock,
            feed_timestamp: None,
            now_unix: self.now_unix,
            age_secs: None,
            staleness_secs: self.staleness_secs,
            state: FreshnessState::Unavailable,
        };
        let Some(feed) = self.realtime else {
            return freshness;
        };
        freshness.feed_timestamp = feed.feed_timestamp;
        if feed.trip_updates.is_empty() {
            diagnostics.push(Diagnostic::info(
                "realtime-without-trip-updates",
                "the realtime feed carries no trip update, so every run is schedule-only",
            ));
        }
        let age = match (feed.feed_timestamp, self.now_unix) {
            (Some(timestamp), Some(now)) => Some(now.saturating_sub(timestamp)),
            _ => None,
        };
        freshness.age_secs = age;
        freshness.state = match age {
            None => {
                diagnostics.push(Diagnostic::warning(
                    "realtime-without-timestamp",
                    "the realtime feed carries no timestamp, so its age is unknown \
                     and the snapshot falls back to the schedule",
                ));
                FreshnessState::Stale
            }
            Some(age) if age > u64::from(self.staleness_secs) => {
                diagnostics.push(Diagnostic::warning(
                    "realtime-stale",
                    format!(
                        "the realtime feed is {age} s old, above the threshold of {} s, \
                         so the snapshot falls back to the schedule",
                        self.staleness_secs
                    ),
                ));
                FreshnessState::Stale
            }
            Some(_) => FreshnessState::Live,
        };
        freshness
    }

    // ------------------------------------------------------------------
    // Positions
    // ------------------------------------------------------------------

    /// Get the trip update for one run, if the feed carries one.
    fn trip_update(&self, trip_id: &str) -> Option<&TripUpdate> {
        self.realtime?
            .trip_updates
            .iter()
            .find(|update| update.trip_id.as_deref() == Some(trip_id))
    }

    /// Place one run on the network, or say why it cannot be placed.
    ///
    /// The function returns `None` for a run that is simply not
    /// running at `clock`. Everything else that keeps a run off the map
    /// leaves a diagnostic behind.
    fn place(
        &self,
        trip: &TripInstance,
        clock: GtfsTime,
        live: bool,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<MapTrain> {
        let update = self.trip_update(&trip.source_trip_id);

        // A cancellation is the operator's own statement, so it holds
        // even when the feed has aged past the staleness threshold.
        // Drawing the run again would invent a train.
        if update.is_some_and(|u| u.canceled) {
            diagnostics.push(
                Diagnostic::info(
                    "train-canceled",
                    "the trip update cancels this run, so the map draws no train for it",
                )
                .about(trip.instance_id.clone()),
            );
            return None;
        }

        // A run from a non-exact headway block has no individual
        // position. The query returns such service as a band, so this
        // guard only catches a caller that expanded it anyway.
        if trip.exactness != TimeExactness::Exact {
            diagnostics.push(
                Diagnostic::warning(
                    "train-approximate-times",
                    "the run comes from a non-exact headway block, so it has no position",
                )
                .about(trip.instance_id.clone()),
            );
            return None;
        }

        let update = if live { update } else { None };
        let mut notes = ShiftNotes::default();
        let adjusted: Vec<Option<AdjustedCall>> = trip
            .calls
            .iter()
            .map(|call| adjust(call, update, &mut notes))
            .collect();
        if notes.time_without_delay {
            diagnostics.push(
                Diagnostic::info(
                    "stop-update-without-delay",
                    "a stop time update carries a predicted time but no delay, and the \
                     snapshot cannot convert a POSIX time to a service day without a \
                     time zone, so the trip-level delay applies instead",
                )
                .about(trip.source_trip_id.clone()),
            );
        }

        let known: Vec<usize> = adjusted
            .iter()
            .enumerate()
            .filter_map(|(index, call)| call.map(|_| index))
            .collect();
        let (&first, &last) = (known.first()?, known.last()?);
        let now = i64::from(clock.seconds());
        if now < adjusted[first]?.arrival || now > adjusted[last]?.departure {
            // The run is not on the network at this clock reading.
            return None;
        }

        let has_update = update.is_some();
        let destination = trip
            .headsign
            .clone()
            .or_else(|| {
                trip.terminus()
                    .map(|id| self.network.station(id).name.clone())
            })
            .unwrap_or_default();

        // Standing at a station.
        for &index in &known {
            let call = adjusted[index]?;
            if call.arrival <= now && now <= call.departure {
                return Some(MapTrain {
                    instance_id: trip.instance_id.clone(),
                    source_trip_id: trip.source_trip_id.clone(),
                    line: trip.line,
                    location: TrainLocation::AtStation {
                        station: trip.calls[index].station,
                        pattern: trip.pattern,
                        index,
                    },
                    progress: 0.0,
                    edge_secs: None,
                    destination,
                    quality: quality(true, has_update, computed(call.quality)),
                    delay_secs: call.delay_secs,
                    schedule_interpolated: computed(call.quality),
                });
            }
        }

        // Between two stations.
        for pair in known.windows(2) {
            let (behind, ahead) = (pair[0], pair[1]);
            let (back, front) = (adjusted[behind]?, adjusted[ahead]?);
            if back.departure > now || now >= front.arrival {
                continue;
            }
            if ahead != behind + 1 {
                diagnostics.push(
                    Diagnostic::warning(
                        "train-between-missing-calls",
                        "the run lies between two calls that have no time, so the \
                         snapshot cannot say which edge it is on",
                    )
                    .about(trip.instance_id.clone()),
                );
                return None;
            }
            let span = front.arrival - back.departure;
            let progress = if span > 0 {
                ((now - back.departure) as f64 / span as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let interpolated = computed(back.quality) || computed(front.quality);
            return Some(MapTrain {
                instance_id: trip.instance_id.clone(),
                source_trip_id: trip.source_trip_id.clone(),
                line: trip.line,
                location: TrainLocation::OnEdge {
                    pattern: trip.pattern,
                    index: behind,
                    from: trip.calls[behind].station,
                    to: trip.calls[ahead].station,
                },
                progress,
                edge_secs: Some(span.max(0) as u32),
                destination,
                quality: quality(false, has_update, interpolated),
                delay_secs: back.delay_secs,
                schedule_interpolated: interpolated,
            });
        }

        // The run spans the clock reading, but no call and no pair of
        // calls brackets it. Realtime shifts that reorder the calls do
        // this. Say so rather than dropping the run in silence.
        diagnostics.push(
            Diagnostic::warning(
                "train-not-placed",
                "the adjusted times of the run do not bracket the clock, so the \
                 snapshot cannot place it",
            )
            .about(trip.instance_id.clone()),
        );
        None
    }
}

// ----------------------------------------------------------------------
// The shift
// ----------------------------------------------------------------------

/// One call of a run with the realtime shift applied.
///
/// The times count seconds after midnight of the service day and stay
/// on it: a call at `25:35:00` keeps that value.
#[derive(Copy, Clone, Debug)]
struct AdjustedCall {
    /// The adjusted arrival.
    arrival: i64,
    /// The adjusted departure.
    departure: i64,
    /// The delay applied here, in seconds.
    delay_secs: Option<i32>,
    /// Where the scheduled time of the call came from.
    quality: TimeQuality,
}

/// What the shift noticed about the trip update.
#[derive(Default)]
struct ShiftNotes {
    /// A stop time update carried a predicted time but no delay.
    time_without_delay: bool,
}

/// Apply the realtime shift to one scheduled call.
///
/// The per-stop event wins where the feed supplies one for the platform
/// of the call, otherwise the trip-level delay applies. A stop time
/// update that carries only an absolute predicted time cannot be used:
/// converting a POSIX time to a service day needs a time zone, which
/// this crate does not carry. Such an update leaves a note instead of a
/// guess.
///
/// The function returns `None` for a call with no time at all.
fn adjust(
    call: &ScheduledCall,
    update: Option<&TripUpdate>,
    notes: &mut ShiftNotes,
) -> Option<AdjustedCall> {
    let arrival = call.arrival_or_departure()?;
    let departure = call.departure_or_arrival()?;

    let stop_update = update.and_then(|u| {
        u.stop_updates
            .iter()
            .find(|su| su.stop_id.as_deref() == Some(call.platform_stop_id.as_str()))
            .filter(|su| !su.skipped)
    });
    if let Some(su) = stop_update {
        let events = [su.arrival, su.departure];
        if events.iter().flatten().all(|e| e.delay_secs.is_none())
            && events.iter().flatten().any(|e| e.time.is_some())
        {
            notes.time_without_delay = true;
        }
    }
    let trip_delay = update.and_then(|u| u.delay_secs);
    let arrival_delay = stop_update
        .and_then(|su| {
            su.arrival
                .and_then(|e| e.delay_secs)
                .or_else(|| su.departure.and_then(|e| e.delay_secs))
        })
        .or(trip_delay);
    let departure_delay = stop_update
        .and_then(|su| {
            su.departure
                .and_then(|e| e.delay_secs)
                .or_else(|| su.arrival.and_then(|e| e.delay_secs))
        })
        .or(trip_delay);

    Some(AdjustedCall {
        arrival: i64::from(arrival.seconds()) + i64::from(arrival_delay.unwrap_or(0)),
        departure: i64::from(departure.seconds()) + i64::from(departure_delay.unwrap_or(0)),
        delay_secs: departure_delay.or(arrival_delay),
        quality: call.time_quality,
    })
}

/// Report whether the library or the feed marked the scheduled time as
/// something other than a plain published time.
fn computed(quality: TimeQuality) -> bool {
    matches!(
        quality,
        TimeQuality::Interpolated | TimeQuality::Approximate
    )
}

/// Pick the provenance marker of one position.
///
/// Without a realtime update the position is schedule-only, whatever
/// else is true of it: that is the treatment a stale feed and an
/// unmatched run both fall back to.
fn quality(at_station: bool, has_update: bool, interpolated: bool) -> PositionQuality {
    if !has_update {
        PositionQuality::ScheduleOnly
    } else if at_station {
        PositionQuality::AtStation
    } else if interpolated {
        PositionQuality::InterpolatedSchedule
    } else {
        PositionQuality::InterpolatedRealtime
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_without_an_update_is_schedule_only() {
        assert_eq!(quality(true, false, false), PositionQuality::ScheduleOnly);
        assert_eq!(quality(false, false, true), PositionQuality::ScheduleOnly);
    }

    #[test]
    fn realtime_positions_carry_their_provenance() {
        assert_eq!(quality(true, true, false), PositionQuality::AtStation);
        assert_eq!(
            quality(false, true, false),
            PositionQuality::InterpolatedRealtime
        );
        assert_eq!(
            quality(false, true, true),
            PositionQuality::InterpolatedSchedule
        );
    }

    #[test]
    fn a_computed_time_is_not_a_published_time() {
        assert!(computed(TimeQuality::Interpolated));
        assert!(computed(TimeQuality::Approximate));
        assert!(!computed(TimeQuality::Exact));
        assert!(!computed(TimeQuality::Missing));
    }
}
