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
//! 2. a shift from the GTFS-Realtime trip update that belongs to that
//!    run — the per-stop events where the feed supplies them,
//!    otherwise the trip-level delay. Which update belongs to which
//!    run is a question of its own, answered below;
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
//! # Which trip update belongs to which run
//!
//! A `trip_id` names a trip in the schedule, not one run of it. The
//! same `trip_id` runs again tomorrow, and a trip that a headway block
//! expands runs several times in one day. A GTFS-Realtime
//! `TripDescriptor` says which run it means with `start_date` and
//! `start_time`, and the snapshot reads both: an update reaches a run
//! only where it can be shown to belong to it.
//!
//! [`crate::matching`] carries the decision table and the one
//! implementation of it, which the live destination board reads too.
//! The snapshot names a run to it with
//! [`crate::matching::RunKey::for_instance`]: the start of a run is the
//! `@<HH:MM:SS>` suffix of its [`TripInstance::instance_id`], which
//! [`RailNetwork::query_trip_instances`] writes from the first call of
//! that run.
//!
//! The ambiguous case — a headway trip and an update with no
//! `start_time` — is the one that has to invent something either way.
//! Applying the delay to every sibling states that four trains are all
//! four minutes late when the operator said that one of them is; the
//! snapshot applies it to none of them instead and leaves a
//! `realtime-update-ambiguous` diagnostic naming the run it left on the
//! schedule.
//!
//! A cancellation is the exception that proves the rule. It outlives a
//! stale feed, because the operator's own statement that a train does
//! not run does not expire the way a delay does — but only where the
//! update provably targets the run, by naming its service date and, for
//! a run from a headway block, its start time. A cancellation that
//! names no date is a statement about a `trip_id`, and a stale feed
//! cannot say which day it was made on: yesterday's cancellation must
//! not suppress today's train.
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

use crate::matching::{RunKey, TripUpdateIndex};
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

/// The default ageing threshold, in seconds.
///
/// Below it the realtime layer is current. Above it, and below the
/// staleness threshold, the layer is *ageing*: the operator's
/// predictions still apply, and the map says the data is no longer
/// new. It is the amber step of the lamp grammar that
/// `docs/LIVE-MAP-POC.md` asks for in phase 4.
///
/// Sixty seconds is half of [`DEFAULT_STALENESS_SECS`], and it is the
/// same minute this project already treats as operationally
/// meaningful: the board lights a lamp when a run is 60 s off
/// schedule. The map borrows a figure that already means something
/// here rather than inventing a second one. Like the staleness
/// threshold it stays a placeholder until phase 0 measures how often
/// the feed timestamp actually advances; a caller that has measured
/// the feed sets its own value with
/// [`NetworkSnapshotBuilder::ageing_secs`].
const DEFAULT_AGEING_SECS: u32 = 60;

/// The number of seconds in one day.
///
/// A run that continues past midnight keeps `24:xx` times on the
/// service day it started on, so reading that day means reading the
/// clock 24 hours later on it.
const SECS_PER_DAY: u32 = 24 * 3600;

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
    /// The run stands at a station, and the realtime layer carried a
    /// time or a delay for that call. This is the strongest claim the
    /// data supports.
    AtStation,
    /// The run lies between two stations, both bracketing times are
    /// published ones, and the realtime layer carried a time or a
    /// delay for at least one of them.
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
///
/// The three drawn states are the lamp grammar of the board: current,
/// ageing, and no longer usable. [`FreshnessState::is_current`] is the
/// one question the placement code asks — whether the operator's
/// predictions still apply.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FreshnessState {
    /// The realtime feed is newer than the ageing threshold.
    Live,
    /// The realtime feed is older than the ageing threshold and newer
    /// than the staleness threshold. The predictions still apply; the
    /// data is no longer new, and the map says so.
    Ageing,
    /// The realtime feed is older than the staleness threshold, or it
    /// carries no timestamp at all. Positions come from the schedule.
    Stale,
    /// The caller supplied no realtime layer.
    Unavailable,
}

impl FreshnessState {
    /// Report whether the realtime layer still applies.
    ///
    /// A live layer and an ageing one both shift positions; a stale
    /// one and an absent one do not, and every train falls back to
    /// [`PositionQuality::ScheduleOnly`].
    pub fn is_current(self) -> bool {
        matches!(self, FreshnessState::Live | FreshnessState::Ageing)
    }
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
    /// The threshold above which the layer counts as ageing, in
    /// seconds.
    pub ageing_secs: u32,
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
    /// The public alert messages, exactly as
    /// [`crate::NetworkStatus::messages`] carries them.
    ///
    /// A disrupted line names its own stations and direction in
    /// [`MapLine::state`], but the legacy payload attaches no message
    /// text to a segment: the messages belong to the network. So the
    /// snapshot carries them as network notices, and a renderer shows
    /// them beside the lines it marks rather than claiming one message
    /// belongs to one line.
    pub notices: Vec<String>,
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
    ageing_secs: u32,
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
            ageing_secs: DEFAULT_AGEING_SECS,
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

    /// Set the ageing threshold, in seconds. The default is 60.
    ///
    /// A realtime feed older than this and newer than the staleness
    /// threshold is [`FreshnessState::Ageing`]: its predictions still
    /// apply, and the page reports the age. The staleness test runs
    /// first, so an ageing threshold at or above the staleness
    /// threshold simply never fires; the snapshot stays deterministic
    /// either way.
    pub fn ageing_secs(mut self, seconds: u32) -> Self {
        self.ageing_secs = seconds;
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
    ///
    /// The builder reads two service days, exactly as
    /// [`RailNetwork::departure_board`] does and for the same reason: a
    /// run that continues past midnight keeps `24:xx` times on the day
    /// it started, so shortly after midnight the trains on the network
    /// belong to the day before, where the same clock reading stands
    /// 24 hours later. A run reaches the map from one of the two days
    /// and never from both — its adjusted times cannot bracket a clock
    /// reading and that same reading a day later — and an
    /// `instance_id` carries the service date it came from, so the two
    /// days stay distinct and the order stays stable.
    pub fn build(&self, date: ServiceDate, clock: GtfsTime) -> NetworkSnapshot {
        let mut diagnostics = Vec::new();
        let freshness = self.freshness(date, clock, &mut diagnostics);
        let live = freshness.state.is_current();
        let updates = TripUpdateIndex::from_feed(self.realtime, &mut diagnostics);

        let mut trains: Vec<MapTrain> = Vec::new();
        let mut bands: Vec<MapBand> = Vec::new();
        for (day, reading) in [
            (date, clock),
            (date.previous_day(), clock.plus_seconds(SECS_PER_DAY)),
        ] {
            let (day_trains, day_bands) =
                self.service_day(day, reading, live, &updates, &mut diagnostics);
            trains.extend(day_trains);
            bands.extend(day_bands);
        }
        trains.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
        bands.sort_by(|a, b| a.band_id.cmp(&b.band_id));

        normalize_diagnostics(&mut diagnostics);
        NetworkSnapshot {
            lines: self.lines(),
            stations: self.stations(),
            edges: self.edges(),
            trains,
            bands,
            freshness,
            notices: self.notices(),
            diagnostics,
        }
    }

    /// Query one service day and place the runs it carries at the
    /// clock reading of that day.
    fn service_day(
        &self,
        date: ServiceDate,
        clock: GtfsTime,
        live: bool,
        updates: &TripUpdateIndex<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> (Vec<MapTrain>, Vec<MapBand>) {
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

        let trains = result
            .trips
            .iter()
            .filter_map(|trip| self.place(trip, clock, live, updates, diagnostics))
            .collect();
        let bands = result
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
        (trains, bands)
    }

    /// Collect the public alert messages of the legacy feed.
    ///
    /// The text is the operator's, unchanged; a renderer escapes it.
    fn notices(&self) -> Vec<String> {
        self.alerts
            .map(|alerts| alerts.messages.iter().map(|m| m.content.clone()).collect())
            .unwrap_or_default()
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
    ///
    /// The two thresholds are read in order — stale first, then
    /// ageing — so the layer degrades in one direction only and a
    /// caller cannot produce a state that contradicts itself.
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
            ageing_secs: self.ageing_secs,
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
            Some(age) if age > u64::from(self.ageing_secs) => {
                diagnostics.push(Diagnostic::info(
                    "realtime-ageing",
                    format!(
                        "the realtime feed is {age} s old, above the ageing threshold of \
                         {} s and below the staleness threshold of {} s, so its \
                         predictions still apply and the page reports the age",
                        self.ageing_secs, self.staleness_secs
                    ),
                ));
                FreshnessState::Ageing
            }
            Some(_) => FreshnessState::Live,
        };
        freshness
    }

    // ------------------------------------------------------------------
    // Positions
    // ------------------------------------------------------------------

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
        updates: &TripUpdateIndex<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<MapTrain> {
        let matched = updates.lookup(RunKey::for_instance(trip));
        if matched.ambiguous {
            diagnostics.push(
                Diagnostic::info(
                    "realtime-update-ambiguous",
                    format!(
                        "a trip update names \"{}\" but no start time, and the headway block \
                         expands that trip into several runs, so the update cannot be shown to \
                         belong to this one; the map draws this run from the schedule",
                        trip.source_trip_id
                    ),
                )
                .about(trip.instance_id.clone()),
            );
        }
        let update = matched.update;

        // A cancellation is the operator's own statement, so it holds
        // even when the feed has aged past the staleness threshold —
        // but only where the update provably targets this run, by
        // naming its service date and, for a run from a headway block,
        // its start time. Drawing a canceled run again would invent a
        // train; suppressing a run on a stale statement that names no
        // day would let yesterday's cancellation delete today's train.
        if update.is_some_and(|u| u.canceled) {
            if live || matched.targeted {
                diagnostics.push(
                    Diagnostic::info(
                        "train-canceled",
                        "the trip update cancels this run, so the map draws no train for it",
                    )
                    .about(trip.instance_id.clone()),
                );
                return None;
            }
            diagnostics.push(
                Diagnostic::info(
                    "train-cancellation-not-attributed",
                    "the trip update cancels the trip but names no start date, and the realtime \
                     feed is stale, so the cancellation cannot be shown to be about this run \
                     rather than an earlier one; the map draws the scheduled run",
                )
                .about(trip.instance_id.clone()),
            );
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
        if notes.skipped > 0 {
            diagnostics.push(
                Diagnostic::info(
                    "train-call-skipped",
                    format!(
                        "the trip update marks {} call(s) of this run skipped, so the map \
                         draws the run passing those stations rather than standing at them",
                        notes.skipped
                    ),
                )
                .about(trip.instance_id.clone()),
            );
        }

        // Every call the snapshot has a time for. A skipped call keeps
        // its place here, because the run still runs over the edges
        // that reach it.
        let known: Vec<usize> = adjusted
            .iter()
            .enumerate()
            .filter_map(|(index, call)| call.map(|_| index))
            .collect();
        // The calls the run actually serves. The operator's skip takes
        // a station out of the trajectory: the run passes it without
        // dwelling, so nothing on the map may say that it stands there,
        // and a run whose first or last call is skipped begins and ends
        // at the calls it does serve.
        let stops: Vec<usize> = known
            .iter()
            .copied()
            .filter(|&index| !adjusted[index].is_some_and(|call| call.skipped))
            .collect();
        let (&first, &last) = (stops.first()?, stops.last()?);
        let now = i64::from(clock.seconds());
        if now < adjusted[first]?.arrival || now > adjusted[last]?.departure {
            // The run is not on the network at this clock reading.
            return None;
        }

        let destination = trip
            .headsign
            .clone()
            .or_else(|| {
                trip.terminus()
                    .map(|id| self.network.station(id).name.clone())
            })
            .unwrap_or_default();

        // Standing at a station.
        for &index in &stops {
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
                    quality: quality(true, call.realtime, computed(call.quality)),
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
                quality: quality(false, back.realtime || front.realtime, interpolated),
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
    /// `true` when the realtime layer carried a time or a delay that
    /// applies to this call. An update that said nothing about it —
    /// an update with no delay and no stop events at all, or one that
    /// names other stops only — leaves this `false`, and a position
    /// this call brackets stays schedule-only.
    realtime: bool,
    /// `true` when the trip update marks this call skipped. The run
    /// passes the station without dwelling: its arrival and its
    /// departure are the same instant, and it never stands there.
    skipped: bool,
}

/// What the shift noticed about the trip update.
#[derive(Default)]
struct ShiftNotes {
    /// A stop time update carried a predicted time but no delay.
    time_without_delay: bool,
    /// How many calls the trip update marks skipped.
    skipped: usize,
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
/// A stop time update that marks the call skipped is the operator
/// saying the run does not serve the station. It carries no prediction
/// to apply, and the call keeps its scheduled arrival as an instant the
/// run passes through: the caller draws the run running past rather
/// than dwelling.
///
/// The function returns `None` for a call with no time at all.
fn adjust(
    call: &ScheduledCall,
    update: Option<&TripUpdate>,
    notes: &mut ShiftNotes,
) -> Option<AdjustedCall> {
    let arrival = call.arrival_or_departure()?;
    let departure = call.departure_or_arrival()?;

    let announced = update.and_then(|u| {
        u.stop_updates
            .iter()
            .find(|su| su.stop_id.as_deref() == Some(call.platform_stop_id.as_str()))
    });
    let skipped = announced.is_some_and(|su| su.skipped);
    if skipped {
        notes.skipped += 1;
    }
    let stop_update = announced.filter(|su| !su.skipped);
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

    // What the realtime layer actually said about this call. An update
    // that carried neither a time nor a delay here shifted nothing, and
    // a position it brackets keeps the schedule-only treatment rather
    // than claiming a provenance the operator did not publish.
    let realtime = trip_delay.is_some()
        || stop_update.is_some_and(|su| {
            [su.arrival, su.departure]
                .iter()
                .flatten()
                .any(|event| event.time.is_some() || event.delay_secs.is_some())
        });

    let arrival_secs = i64::from(arrival.seconds()) + i64::from(arrival_delay.unwrap_or(0));
    let departure_secs = i64::from(departure.seconds()) + i64::from(departure_delay.unwrap_or(0));
    Some(AdjustedCall {
        arrival: arrival_secs,
        // A skipped call has no dwell: the run reaches the station and
        // carries straight on to the next one.
        departure: if skipped {
            arrival_secs
        } else {
            departure_secs
        },
        delay_secs: departure_delay.or(arrival_delay),
        quality: call.time_quality,
        realtime,
        skipped,
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
/// Without a realtime shift on a bracketing call the position is
/// schedule-only, whatever else is true of it: that is the treatment a
/// stale feed, an unmatched run, and a trip update that said nothing
/// about these calls all fall back to.
fn quality(at_station: bool, realtime: bool, interpolated: bool) -> PositionQuality {
    if !realtime {
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
    fn an_ageing_layer_still_applies_and_a_stale_one_does_not() {
        assert!(FreshnessState::Live.is_current());
        assert!(FreshnessState::Ageing.is_current());
        assert!(!FreshnessState::Stale.is_current());
        assert!(!FreshnessState::Unavailable.is_current());
    }

    #[test]
    fn a_computed_time_is_not_a_published_time() {
        assert!(computed(TimeQuality::Interpolated));
        assert!(computed(TimeQuality::Approximate));
        assert!(!computed(TimeQuality::Exact));
        assert!(!computed(TimeQuality::Missing));
    }
}
