//! # mrt-live
//!
//! Merge the static rail network with live DataMall data into
//! render-ready view models.
//!
//! This crate is the composition layer of the Singapore-MRTracker
//! project. It takes:
//!
//! - the static network model from `mrt-gtfs`,
//! - the live status data from `mrt-datamall`,
//! - the decoded GTFS-Realtime data from `mrt-gtfs-rt`,
//!
//! and produces flat structures that a renderer shows directly: a
//! per-line network status, a live destination board, and a snapshot
//! of the whole network for a live map. It also reads the schematic
//! layout that the map is drawn on and binds it to the network. All
//! view models serialize with `serde`, so a web map, a destination
//! board, or an LED panel driver can consume them as JSON.
//!
//! This crate does no input/output. The application fetches the data
//! and passes it in. This keeps render loops testable and fast.
//!
//! # Example
//!
//! ```no_run
//! use mrt_datamall::DataMallClient;
//! use mrt_gtfs::{GtfsFeed, RailNetwork, ZipSource};
//! use mrt_live::{LiveBoardBuilder, NetworkStatus};
//!
//! // Fetch the official GTFS Schedule feed for trains.
//! let client = DataMallClient::from_env().unwrap();
//! let bytes = client.fetch_gtfs_schedule().unwrap();
//! let mut source = ZipSource::from_reader(std::io::Cursor::new(bytes)).unwrap();
//! let network = RailNetwork::from_feed(&GtfsFeed::load(&mut source).unwrap()).unwrap();
//!
//! // Merge the live layers.
//! let alerts = client.train_service_alerts().unwrap();
//! let status = NetworkStatus::from_alerts(&alerts);
//! let station = network.station_by_code("NS1").unwrap();
//! let board = LiveBoardBuilder::new(&network)
//!     .with_alerts(&alerts)
//!     .build(station, "20260810".parse().unwrap(), "08:00:00".parse().unwrap(), 1800);
//! for row in &board.rows {
//!     println!("{} {} in {}s", row.line_code, row.destination, row.departs_in_secs);
//! }
//! ```

#![warn(missing_docs)]

pub mod clock;
mod layout;
mod map;

pub use layout::{
    BoundLayout, BoundStation, Layout, LayoutBranch, LayoutError, LayoutLine, LayoutPoint,
    LayoutStation, UncoveredStation, UnmatchedReason, UnmatchedStation,
};
pub use map::{
    Freshness, FreshnessState, MapBand, MapEdge, MapLine, MapStation, MapTrain, NetworkSnapshot,
    NetworkSnapshotBuilder, PositionQuality, TrainLocation,
};

use serde::Serialize;

use mrt_datamall::{CrowdLevel, PlatformCrowd, ServiceStatus, TrainLine, TrainServiceAlerts};
use mrt_gtfs::{GtfsTime, Line, RailNetwork, ServiceDate, StationId};
use mrt_gtfs_rt::{Alert, RailRtFeed};

// ----------------------------------------------------------------------
// Line matching
// ----------------------------------------------------------------------

/// Match a GTFS line to a Singapore [`TrainLine`].
///
/// GTFS feeds name their routes in different ways: `NSL`, `NS`,
/// `North South Line`, and so on. This function tries the short name
/// as a code first. Then it searches the names for the known line
/// names.
pub fn match_train_line(line: &Line) -> Option<TrainLine> {
    if let Ok(parsed) = line.name.parse::<TrainLine>() {
        return Some(parsed);
    }
    let mut text = line.name.to_ascii_uppercase();
    if let Some(long) = &line.long_name {
        text.push(' ');
        text.push_str(&long.to_ascii_uppercase());
    }
    // The official LTA feed hyphenates the names, for example
    // "North-South Line". Normalize the hyphens away.
    let text = text.replace('-', " ");
    const NAME_MAP: [(&str, TrainLine); 10] = [
        ("NORTH SOUTH", TrainLine::NSL),
        ("EAST WEST", TrainLine::EWL),
        ("NORTH EAST", TrainLine::NEL),
        ("CIRCLE", TrainLine::CCL),
        ("DOWNTOWN", TrainLine::DTL),
        ("THOMSON", TrainLine::TEL),
        ("BUKIT PANJANG", TrainLine::BPL),
        ("SENGKANG", TrainLine::SLRT),
        ("PUNGGOL", TrainLine::PLRT),
        ("CHANGI", TrainLine::CGL),
    ];
    NAME_MAP
        .iter()
        .find(|(needle, _)| text.contains(needle))
        .map(|&(_, line)| line)
}

// ----------------------------------------------------------------------
// Network status
// ----------------------------------------------------------------------

/// The live state of one line.
#[derive(Debug, Clone, Serialize)]
pub enum LineState {
    /// The line operates as normal.
    Normal,
    /// A part of the line is disrupted.
    Disrupted {
        /// The affected station codes, for example `NE1`.
        stations: Vec<String>,
        /// The affected direction, as the alert reports it.
        direction: String,
        /// The station codes with free public bus service.
        free_public_bus: Vec<String>,
    },
}

/// The live status of one line.
#[derive(Debug, Clone, Serialize)]
pub struct LineStatus {
    /// The line.
    pub line: TrainLine,
    /// The state of the line.
    pub state: LineState,
}

impl LineStatus {
    /// Report whether the line is disrupted.
    pub fn is_disrupted(&self) -> bool {
        matches!(self.state, LineState::Disrupted { .. })
    }
}

/// The live status of the whole rail network.
///
/// Build it from the legacy train service alerts with
/// [`NetworkStatus::from_alerts`]. The structure always contains one
/// entry per known line, so a status board can render a stable list.
#[derive(Debug, Clone, Serialize)]
pub struct NetworkStatus {
    /// The overall service status.
    pub overall: ServiceStatus,
    /// The status of every known line.
    pub lines: Vec<LineStatus>,
    /// The public alert messages.
    pub messages: Vec<String>,
}

impl NetworkStatus {
    /// Build the network status from the legacy train service alerts.
    pub fn from_alerts(alerts: &TrainServiceAlerts) -> Self {
        let mut lines: Vec<LineStatus> = TrainLine::ALL
            .iter()
            .map(|&line| LineStatus {
                line,
                state: LineState::Normal,
            })
            .collect();
        for segment in &alerts.affected_segments {
            let Some(train_line) = segment.train_line() else {
                continue;
            };
            if let Some(status) = lines.iter_mut().find(|s| s.line == train_line) {
                status.state = LineState::Disrupted {
                    stations: segment.station_codes(),
                    direction: segment.direction.clone(),
                    free_public_bus: segment.free_public_bus_codes(),
                };
            }
        }
        NetworkStatus {
            overall: alerts.status,
            lines,
            messages: alerts.messages.iter().map(|m| m.content.clone()).collect(),
        }
    }

    /// Get the status of one line.
    pub fn line(&self, line: TrainLine) -> &LineStatus {
        self.lines
            .iter()
            .find(|s| s.line == line)
            .expect("NetworkStatus contains every known line")
    }
}

// ----------------------------------------------------------------------
// Live destination board
// ----------------------------------------------------------------------

/// One row of a live destination board.
#[derive(Debug, Clone, Serialize)]
pub struct LiveBoardRow {
    /// The display code of the line, for example `NSL`.
    pub line_code: String,
    /// The line color as a six-digit hexadecimal value.
    pub line_color: Option<String>,
    /// The destination text.
    pub destination: String,
    /// The wait time until the predicted departure, in seconds.
    ///
    /// With a realtime layer attached this is the scheduled time plus
    /// the delay that applies here. Without one, and for a canceled
    /// departure, it is the scheduled wait.
    pub departs_in_secs: u32,
    /// The predicted departure time on the 24-hour clock, as
    /// `HH:MM:SS`. It carries the same shift as
    /// [`LiveBoardRow::departs_in_secs`].
    pub clock_time: String,
    /// `true` if the time comes from a headway and is approximate.
    pub approximate: bool,
    /// The live delay in seconds, if a trip update reports one. The
    /// board has already applied it to
    /// [`LiveBoardRow::departs_in_secs`] and
    /// [`LiveBoardRow::clock_time`]; the figure is here for a row that
    /// wants to name it.
    pub delay_secs: Option<i32>,
    /// `true` if a trip update or a service alert cancels this trip.
    pub canceled: bool,
    /// `true` if an active service alert disturbs this departure
    /// without a delay figure: reduced service, significant delays,
    /// a detour, or a modified service.
    pub alerted: bool,
    /// The live platform crowd level at the station, if available.
    pub crowd: Option<CrowdLevel>,
}

/// What the realtime layer says about one departure at one station.
#[derive(Debug, Clone, Copy, Default)]
struct TripStatus {
    /// The delay that applies to the scheduled time, in seconds.
    delay_secs: Option<i32>,
    /// `true` when a trip update cancels the whole trip.
    canceled: bool,
    /// `true` when a trip update marks the call at this station
    /// skipped: the train will not call here.
    skipped: bool,
}

/// The seconds in one day, for wrapping a shifted clock time.
const SECS_PER_DAY: i64 = 24 * 3600;

/// The largest realtime shift the board widens its schedule query by.
///
/// Three hours covers any plausible delay on a metro. A feed that
/// reports more still shifts the rows it names; it only stops widening
/// the query.
const MAX_SHIFT_SECS: u32 = 3 * 3600;

/// The clock time of a departure with the realtime shift applied.
///
/// The result stays on the 24-hour clock, the way
/// [`mrt_gtfs::BoardEntry::clock_time`] does: a trip that runs past
/// midnight, and a shift that crosses it, both wrap.
fn shifted_clock(scheduled: GtfsTime, shift_secs: i64) -> GtfsTime {
    let seconds = i64::from(scheduled.seconds()) + shift_secs;
    GtfsTime::from_seconds(seconds.rem_euclid(SECS_PER_DAY) as u32)
}

/// A live destination board for one station.
#[derive(Debug, Clone, Serialize)]
pub struct LiveBoard {
    /// The station name.
    pub station_name: String,
    /// The station codes, for example `NS1` and `EW24`.
    pub station_codes: Vec<String>,
    /// The board rows, in predicted wait-time order.
    pub rows: Vec<LiveBoardRow>,
    /// Alert messages for the lines that serve this station.
    pub notices: Vec<String>,
}

/// A builder that merges live data layers into a [`LiveBoard`].
///
/// Every layer is optional. Without live layers the board shows the
/// static schedule.
///
/// With a realtime layer the board predicts: every row carries its
/// scheduled time plus the delay the feed reports for it, and the
/// predicted time decides the order of the rows, the window they must
/// fall in, and which of them survive [`LiveBoardBuilder::max_rows`].
pub struct LiveBoardBuilder<'a> {
    network: &'a RailNetwork,
    alerts: Option<&'a TrainServiceAlerts>,
    crowd: &'a [PlatformCrowd],
    realtime: Option<&'a RailRtFeed>,
    rt_alerts: &'a [Alert],
    now_unix: Option<u64>,
    max_rows: usize,
}

impl<'a> LiveBoardBuilder<'a> {
    /// Make a builder for the given network.
    pub fn new(network: &'a RailNetwork) -> Self {
        LiveBoardBuilder {
            network,
            alerts: None,
            crowd: &[],
            realtime: None,
            rt_alerts: &[],
            now_unix: None,
            max_rows: 10,
        }
    }

    /// Add the legacy train service alerts.
    pub fn with_alerts(mut self, alerts: &'a TrainServiceAlerts) -> Self {
        self.alerts = Some(alerts);
        self
    }

    /// Add GTFS-Realtime service alerts, active at the given POSIX
    /// time.
    ///
    /// The time selects the alerts whose active periods apply now.
    /// An active alert that names a trip, a route, or a platform of
    /// the station reaches the board: a no-service alert cancels the
    /// affected departures, a disturbance marks them
    /// ([`LiveBoardRow::alerted`]), and the alert text joins the
    /// notices.
    pub fn with_rt_alerts(mut self, alerts: &'a [Alert], now_unix: u64) -> Self {
        self.rt_alerts = alerts;
        self.now_unix = Some(now_unix);
        self
    }

    /// Add live platform crowd records.
    pub fn with_crowd(mut self, crowd: &'a [PlatformCrowd]) -> Self {
        self.crowd = crowd;
        self
    }

    /// Add a decoded GTFS-Realtime feed with trip updates.
    pub fn with_realtime(mut self, realtime: &'a RailRtFeed) -> Self {
        self.realtime = Some(realtime);
        self
    }

    /// Set the maximum number of board rows. The default is 10.
    pub fn max_rows(mut self, max_rows: usize) -> Self {
        self.max_rows = max_rows;
        self
    }

    /// Build the board for one station.
    ///
    /// The board lists the departures predicted between `clock` and
    /// `clock + lookahead_secs` on the given date. A realtime layer
    /// moves a departure across either edge of that window: a late
    /// train scheduled before `clock` still shows, and an early train
    /// scheduled after the window comes in. The rows are in predicted
    /// order and the last of them fall away at
    /// [`LiveBoardBuilder::max_rows`].
    ///
    /// A row a trip update marks skipped at this station never
    /// appears: the operator says that train will not call here. A
    /// canceled row stays, so the board can read `CANC`, and keeps its
    /// scheduled time for the order and the window.
    pub fn build(
        &self,
        station: StationId,
        date: ServiceDate,
        clock: GtfsTime,
        lookahead_secs: u32,
    ) -> LiveBoard {
        let station_data = self.network.station(station);
        let crowd = self.station_crowd(station_data.codes.iter().map(String::as_str));

        // The realtime layer moves departures across both edges of the
        // window, so the schedule query runs wider than the window the
        // caller asked for and the predicted time decides afterwards.
        // Without a realtime layer both margins are zero and the query
        // is the plain scheduled one.
        let (back, forward) = self.window_margins();
        let back = back.min(clock.seconds());
        let from = GtfsTime::from_seconds(clock.seconds() - back);
        let span = lookahead_secs.saturating_add(back).saturating_add(forward);

        let mut rows: Vec<(i64, LiveBoardRow)> = Vec::new();
        for entry in self.network.departure_board(station, date, from, span) {
            let status = self.trip_status(&entry.departure.trip_id, station);
            // The operator says this run does not call here at all, so
            // the row leaves the board rather than merely losing its
            // delay.
            if status.skipped {
                continue;
            }
            let line = self.network.line(entry.departure.line);
            let (alert_canceled, alerted) =
                self.alert_status(&entry.departure.trip_id, line, station);
            let canceled = status.canceled || alert_canceled;
            // Nothing predicts a train that will not run: a canceled
            // row keeps the scheduled time it was booked for.
            let shift = if canceled {
                0
            } else {
                i64::from(status.delay_secs.unwrap_or(0))
            };
            let wait = i64::from(entry.wait_secs) - i64::from(back) + shift;
            if wait < 0 || wait > i64::from(lookahead_secs) {
                continue;
            }
            let destination = entry
                .departure
                .headsign
                .clone()
                .unwrap_or_else(|| self.network.station(entry.departure.terminus).name.clone());
            rows.push((
                wait,
                LiveBoardRow {
                    line_code: line.name.clone(),
                    line_color: line.color.clone(),
                    destination,
                    departs_in_secs: wait as u32,
                    clock_time: shifted_clock(entry.departure.time, shift).to_string(),
                    approximate: !entry.departure.exact,
                    delay_secs: status.delay_secs,
                    canceled,
                    alerted,
                    crowd,
                },
            ));
        }

        // The schedule query returns its entries in scheduled order,
        // built deterministically. This sort is stable, so rows that
        // share a predicted time keep that scheduled order and the
        // board stays deterministic.
        rows.sort_by_key(|(wait, _)| *wait);
        rows.truncate(self.max_rows);

        LiveBoard {
            station_name: station_data.name.clone(),
            station_codes: station_data.codes.clone(),
            rows: rows.into_iter().map(|(_, row)| row).collect(),
            notices: self.notices(station),
        }
    }

    /// How far the realtime layer can move a scheduled departure, in
    /// seconds before and after the window the caller asked for.
    ///
    /// The margins come from the delays the feed actually carries, so
    /// the widened schedule query stays as narrow as the data allows
    /// and stays deterministic. [`MAX_SHIFT_SECS`] caps them: a longer
    /// delay still applies to the rows it names, but does not widen
    /// the query further.
    fn window_margins(&self) -> (u32, u32) {
        let Some(feed) = self.realtime else {
            return (0, 0);
        };
        let mut latest: i32 = 0;
        let mut earliest: i32 = 0;
        for update in &feed.trip_updates {
            let mut note = |delay: Option<i32>| {
                if let Some(delay) = delay {
                    latest = latest.max(delay);
                    earliest = earliest.min(delay);
                }
            };
            note(update.delay_secs);
            for stop_update in &update.stop_updates {
                note(stop_update.arrival.and_then(|event| event.delay_secs));
                note(stop_update.departure.and_then(|event| event.delay_secs));
            }
        }
        (
            latest.unsigned_abs().min(MAX_SHIFT_SECS),
            earliest.unsigned_abs().min(MAX_SHIFT_SECS),
        )
    }

    /// Get the worst live crowd level among the station codes.
    fn station_crowd<'c>(&self, codes: impl Iterator<Item = &'c str>) -> Option<CrowdLevel> {
        let mut worst: Option<CrowdLevel> = None;
        for code in codes {
            for record in self.crowd {
                if record.station.eq_ignore_ascii_case(code) {
                    let level = record.crowd_level;
                    if crowd_severity(level) > worst.map(crowd_severity).unwrap_or(0) {
                        worst = Some(level);
                    }
                }
            }
        }
        worst
    }

    /// Get what the realtime layer says about one trip at one station.
    ///
    /// The per-stop event of the call wins where the feed supplies
    /// one for a platform of the station: its departure delay first,
    /// then its arrival delay. The trip-level delay fills in for a
    /// call the feed says nothing about. This is the rule the map view
    /// model applies to the same feed.
    fn trip_status(&self, trip_id: &str, station: StationId) -> TripStatus {
        let Some(feed) = self.realtime else {
            return TripStatus::default();
        };
        let Some(update) = feed
            .trip_updates
            .iter()
            .find(|u| u.trip_id.as_deref() == Some(trip_id))
        else {
            return TripStatus::default();
        };
        if update.canceled {
            return TripStatus {
                canceled: true,
                ..TripStatus::default()
            };
        }
        let platforms = &self.network.station(station).platform_stop_ids;
        let announced = update.stop_updates.iter().find(|su| {
            su.stop_id
                .as_deref()
                .is_some_and(|id| platforms.iter().any(|p| p == id))
        });
        // A skipped call carries no prediction at all: the operator
        // says the train passes this station by.
        if announced.is_some_and(|su| su.skipped) {
            return TripStatus {
                skipped: true,
                ..TripStatus::default()
            };
        }
        let delay_secs = announced
            .and_then(|su| {
                su.departure
                    .and_then(|event| event.delay_secs)
                    .or_else(|| su.arrival.and_then(|event| event.delay_secs))
            })
            .or(update.delay_secs);
        TripStatus {
            delay_secs,
            ..TripStatus::default()
        }
    }

    /// The effect of the active service alerts on one departure.
    ///
    /// An alert reaches the departure when it names the trip, the
    /// route of the line, or a platform of the station. The first
    /// flag reports a canceling no-service alert, the second a
    /// disturbance without a delay figure.
    fn alert_status(&self, trip_id: &str, line: &Line, station: StationId) -> (bool, bool) {
        let Some(now) = self.now_unix else {
            return (false, false);
        };
        let platforms = &self.network.station(station).platform_stop_ids;
        let mut canceled = false;
        let mut alerted = false;
        for alert in self.rt_alerts {
            if !alert.is_active(now) {
                continue;
            }
            let informs = alert.informed.iter().any(|entity| {
                entity.trip_id.as_deref() == Some(trip_id)
                    || entity.route_id.as_deref() == Some(line.route_id.as_str())
                    || entity
                        .stop_id
                        .as_deref()
                        .is_some_and(|id| platforms.iter().any(|p| p == id))
            });
            if !informs {
                continue;
            }
            canceled = canceled || alert.effect.stops_service();
            alerted = alerted || alert.effect.disturbs_service();
        }
        (canceled, alerted)
    }

    /// Collect the notices for the station: the legacy alert
    /// messages, then the texts of the active service alerts.
    fn notices(&self, station: StationId) -> Vec<String> {
        let mut notices = self.legacy_notices(station);
        for text in self.rt_alert_notices(station) {
            if !notices.contains(&text) {
                notices.push(text);
            }
        }
        notices
    }

    /// Collect the texts of the active service alerts that name a
    /// line or a platform of the station.
    fn rt_alert_notices(&self, station: StationId) -> Vec<String> {
        let Some(now) = self.now_unix else {
            return Vec::new();
        };
        let station_data = self.network.station(station);
        let routes: Vec<&str> = station_data
            .lines
            .iter()
            .map(|&id| self.network.line(id).route_id.as_str())
            .collect();
        let mut texts = Vec::new();
        for alert in self.rt_alerts {
            if !alert.is_active(now) {
                continue;
            }
            let informs = alert.informed.iter().any(|entity| {
                entity
                    .route_id
                    .as_deref()
                    .is_some_and(|route| routes.contains(&route))
                    || entity
                        .stop_id
                        .as_deref()
                        .is_some_and(|id| station_data.platform_stop_ids.iter().any(|p| p == id))
            });
            if !informs {
                continue;
            }
            if let Some(text) = alert.text() {
                if !texts.iter().any(|t| t == text) {
                    texts.push(text.to_string());
                }
            }
        }
        texts
    }

    /// Collect the legacy alert messages for the lines that serve the
    /// station.
    fn legacy_notices(&self, station: StationId) -> Vec<String> {
        let Some(alerts) = self.alerts else {
            return Vec::new();
        };
        if alerts.status != ServiceStatus::Disrupted {
            return Vec::new();
        }
        let station_data = self.network.station(station);
        let serving: Vec<TrainLine> = station_data
            .lines
            .iter()
            .filter_map(|&id| match_train_line(self.network.line(id)))
            .collect();
        let relevant = alerts.affected_segments.iter().any(|segment| {
            let line_match = segment
                .train_line()
                .is_some_and(|line| serving.contains(&line));
            let station_match = segment.station_codes().iter().any(|code| {
                station_data
                    .codes
                    .iter()
                    .any(|c| c.eq_ignore_ascii_case(code))
            });
            line_match || station_match
        });
        if relevant {
            alerts.messages.iter().map(|m| m.content.clone()).collect()
        } else {
            Vec::new()
        }
    }
}

/// Rank a crowd level for comparisons. Higher means more crowded.
fn crowd_severity(level: CrowdLevel) -> u8 {
    match level {
        CrowdLevel::Unknown => 0,
        CrowdLevel::Low => 1,
        CrowdLevel::Moderate => 2,
        CrowdLevel::High => 3,
    }
}
