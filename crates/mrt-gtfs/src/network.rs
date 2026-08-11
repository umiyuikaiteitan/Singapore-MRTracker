//! A linked rail network model.
//!
//! [`RailNetwork`] turns raw GTFS tables into a model with stations,
//! lines, and stop patterns. Identifier types such as [`StationId`] are
//! plain indexes. They make lookups fast and keep the model easy to
//! port to other languages.

use std::collections::HashMap;

use serde::Serialize;

use crate::alias;
use crate::date::ServiceDate;
use crate::error::GtfsError;
use crate::feed::GtfsFeed;
use crate::filter::RailFilter;
use crate::model::{Frequency, EXCEPTION_SERVICE_ADDED, EXCEPTION_SERVICE_REMOVED};
use crate::time::GtfsTime;

/// The index of a [`Line`] in a [`RailNetwork`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct LineId(pub usize);

/// The index of a [`Station`] in a [`RailNetwork`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct StationId(pub usize);

/// The index of a [`StopPattern`] in a [`RailNetwork`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PatternId(pub usize);

/// A rail line, built from one GTFS route.
#[derive(Debug, Clone, Serialize)]
pub struct Line {
    /// The GTFS route identifier.
    pub route_id: String,
    /// The best available display name, for example `NSL`.
    pub name: String,
    /// The long name, for example `North South Line`.
    pub long_name: Option<String>,
    /// The GTFS route type.
    pub route_type: u16,
    /// The line color as a six-digit hexadecimal value.
    pub color: Option<String>,
    /// The text color as a six-digit hexadecimal value.
    pub text_color: Option<String>,
}

/// A rail station, built from one GTFS station or one standalone stop.
#[derive(Debug, Clone, Serialize)]
pub struct Station {
    /// The GTFS identifier. For a grouped station this is the parent
    /// `stop_id`. For a standalone stop this is the stop `stop_id`.
    pub gtfs_id: String,
    /// The public name, for example `Jurong East`.
    pub name: String,
    /// The public station codes, for example `NS1` and `EW24`.
    pub codes: Vec<String>,
    /// The position of the station, in WGS 84 degrees.
    pub lat: Option<f64>,
    /// The position of the station, in WGS 84 degrees.
    pub lon: Option<f64>,
    /// The `stop_id` values of the platforms of this station.
    pub platform_stop_ids: Vec<String>,
    /// The lines that serve this station.
    pub lines: Vec<LineId>,
}

impl Station {
    /// Report whether more than one line entry serves this station.
    ///
    /// Some feeds split one line into several route entries with the
    /// same display name. This method then reports `true` for
    /// stations on one physical line. Use
    /// [`RailNetwork::is_interchange`] for a check that compares the
    /// line names.
    pub fn is_interchange(&self) -> bool {
        self.lines.len() > 1
    }
}

/// An ordered sequence of stations that trips of one line follow.
#[derive(Debug, Clone, Serialize)]
pub struct StopPattern {
    /// The line of this pattern.
    pub line: LineId,
    /// The GTFS direction of this pattern. `0` and `1` are opposite
    /// directions.
    pub direction: Option<u8>,
    /// The stations of this pattern, in travel order.
    pub stations: Vec<StationId>,
}

/// A transfer rule between two stations.
#[derive(Debug, Clone, Serialize)]
pub struct StationTransfer {
    /// The origin station.
    pub from: StationId,
    /// The destination station.
    pub to: StationId,
    /// The minimum transfer time, in seconds.
    pub min_transfer_secs: Option<u32>,
}

/// One scheduled call of a trip at a station.
#[derive(Debug, Clone)]
pub(crate) struct StopCall {
    pub arrival: Option<GtfsTime>,
    pub departure: Option<GtfsTime>,
}

impl StopCall {
    /// Get the best departure time: the departure, or else the arrival.
    pub(crate) fn departure_or_arrival(&self) -> Option<GtfsTime> {
        self.departure.or(self.arrival)
    }
}

/// The schedule of one trip, aligned with its stop pattern.
#[derive(Debug, Clone)]
pub(crate) struct TripSchedule {
    pub trip_id: String,
    pub line: LineId,
    pub pattern: PatternId,
    pub service: usize,
    pub headsign: Option<String>,
    /// One call per station of the pattern, in the same order.
    pub calls: Vec<StopCall>,
    /// The frequency blocks of the trip. Empty for a fixed-schedule
    /// trip.
    pub frequencies: Vec<Frequency>,
}

/// A weekly service rule from `calendar.txt`.
#[derive(Debug, Clone)]
struct WeeklyRule {
    days: [bool; 7],
    start: ServiceDate,
    end: ServiceDate,
}

/// The service calendar of the network.
#[derive(Debug, Clone, Default)]
pub(crate) struct ServiceCalendar {
    ids: Vec<String>,
    index: HashMap<String, usize>,
    weekly: Vec<Option<WeeklyRule>>,
    exceptions: HashMap<(usize, ServiceDate), u8>,
}

impl ServiceCalendar {
    /// Get or make the index for a service identifier.
    fn intern(&mut self, service_id: &str) -> usize {
        if let Some(&idx) = self.index.get(service_id) {
            return idx;
        }
        let idx = self.ids.len();
        self.ids.push(service_id.to_string());
        self.index.insert(service_id.to_string(), idx);
        self.weekly.push(None);
        idx
    }

    /// Report whether the service operates on the date.
    pub(crate) fn active(&self, service: usize, date: ServiceDate) -> bool {
        match self.exceptions.get(&(service, date)) {
            Some(&EXCEPTION_SERVICE_ADDED) => return true,
            Some(&EXCEPTION_SERVICE_REMOVED) => return false,
            _ => {}
        }
        match &self.weekly[service] {
            Some(rule) => {
                rule.start <= date && date <= rule.end && rule.days[date.weekday().index()]
            }
            None => false,
        }
    }

    fn lookup(&self, service_id: &str) -> Option<usize> {
        self.index.get(service_id).copied()
    }
}

/// A linked rail network model.
///
/// # Examples
///
/// ```no_run
/// use mrt_gtfs::{GtfsFeed, RailNetwork};
///
/// let feed = GtfsFeed::from_zip_path("data/singapore-gtfs.zip").unwrap();
/// let network = RailNetwork::from_feed(&feed).unwrap();
/// for id in network.interchanges() {
///     println!("{} is an interchange.", network.station(id).name);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct RailNetwork {
    lines: Vec<Line>,
    stations: Vec<Station>,
    patterns: Vec<StopPattern>,
    transfers: Vec<StationTransfer>,
    pub(crate) trips: Vec<TripSchedule>,
    pub(crate) services: ServiceCalendar,
    station_index: HashMap<String, StationId>,
    code_index: HashMap<String, StationId>,
    alias_index: HashMap<String, StationId>,
    stop_to_station: HashMap<String, StationId>,
    line_index: HashMap<String, LineId>,
}

impl RailNetwork {
    /// Build a network from a feed with the default [`RailFilter`].
    pub fn from_feed(feed: &GtfsFeed) -> Result<Self, GtfsError> {
        Self::from_feed_with(feed, &RailFilter::default())
    }

    /// Build a network from a feed with a custom [`RailFilter`].
    pub fn from_feed_with(feed: &GtfsFeed, filter: &RailFilter) -> Result<Self, GtfsError> {
        let feed = filter.apply(feed);
        Self::build(&feed)
    }

    fn build(feed: &GtfsFeed) -> Result<Self, GtfsError> {
        // Step 1: build the service calendar.
        let mut services = ServiceCalendar::default();
        for row in &feed.calendar {
            let idx = services.intern(&row.service_id);
            services.weekly[idx] = Some(WeeklyRule {
                days: row.weekday_flags(),
                start: row.start_date,
                end: row.end_date,
            });
        }
        for row in &feed.calendar_dates {
            let idx = services.intern(&row.service_id);
            services
                .exceptions
                .insert((idx, row.date), row.exception_type);
        }

        // Step 2: build the stations. A GTFS station record groups its
        // platforms. A stop without a parent becomes its own station.
        let mut stations: Vec<Station> = Vec::new();
        let mut station_index: HashMap<String, StationId> = HashMap::new();
        for stop in feed.stops.iter().filter(|s| s.is_station()) {
            let id = StationId(stations.len());
            station_index.insert(stop.stop_id.clone(), id);
            stations.push(Station {
                gtfs_id: stop.stop_id.clone(),
                name: stop
                    .stop_name
                    .clone()
                    .unwrap_or_else(|| stop.stop_id.clone()),
                codes: stop.stop_code.iter().cloned().collect(),
                lat: stop.stop_lat,
                lon: stop.stop_lon,
                platform_stop_ids: Vec::new(),
                lines: Vec::new(),
            });
        }
        let mut stop_to_station: HashMap<String, StationId> = HashMap::new();
        for stop in feed.stops.iter().filter(|s| s.is_boarding_location()) {
            let station_id = match stop.parent_station_id().and_then(|p| station_index.get(p)) {
                Some(&id) => id,
                None => {
                    let id = StationId(stations.len());
                    station_index.insert(stop.stop_id.clone(), id);
                    stations.push(Station {
                        gtfs_id: stop.stop_id.clone(),
                        name: stop
                            .stop_name
                            .clone()
                            .unwrap_or_else(|| stop.stop_id.clone()),
                        codes: Vec::new(),
                        lat: None,
                        lon: None,
                        platform_stop_ids: Vec::new(),
                        lines: Vec::new(),
                    });
                    id
                }
            };
            let station = &mut stations[station_id.0];
            station.platform_stop_ids.push(stop.stop_id.clone());
            if let Some(code) = &stop.stop_code {
                if !code.is_empty() && !station.codes.contains(code) {
                    station.codes.push(code.clone());
                }
            }
            if station.lat.is_none() {
                station.lat = stop.stop_lat;
                station.lon = stop.stop_lon;
            }
            stop_to_station.insert(stop.stop_id.clone(), station_id);
        }
        let code_index: HashMap<String, StationId> = stations
            .iter()
            .enumerate()
            .flat_map(|(i, s)| {
                s.codes
                    .iter()
                    .map(move |c| (c.to_ascii_uppercase(), StationId(i)))
            })
            .collect();
        let alias_index = build_alias_index(&stations);

        // Step 3: build the lines.
        let mut lines: Vec<Line> = Vec::new();
        let mut line_index: HashMap<String, LineId> = HashMap::new();
        for route in &feed.routes {
            let name = route
                .route_short_name
                .clone()
                .filter(|s| !s.is_empty())
                .or_else(|| route.route_long_name.clone().filter(|s| !s.is_empty()))
                .unwrap_or_else(|| route.route_id.clone());
            line_index.insert(route.route_id.clone(), LineId(lines.len()));
            lines.push(Line {
                route_id: route.route_id.clone(),
                name,
                long_name: route.route_long_name.clone().filter(|s| !s.is_empty()),
                route_type: route.route_type,
                color: route.route_color.clone().filter(|s| !s.is_empty()),
                text_color: route.route_text_color.clone().filter(|s| !s.is_empty()),
            });
        }

        // Step 4: group the stop times by trip.
        let mut calls_by_trip: HashMap<&str, Vec<&crate::model::StopTime>> = HashMap::new();
        for st in &feed.stop_times {
            calls_by_trip.entry(&st.trip_id).or_default().push(st);
        }
        for calls in calls_by_trip.values_mut() {
            calls.sort_by_key(|st| st.stop_sequence);
        }
        let mut freq_by_trip: HashMap<&str, Vec<Frequency>> = HashMap::new();
        for f in &feed.frequencies {
            freq_by_trip
                .entry(f.trip_id.as_str())
                .or_default()
                .push(f.clone());
        }

        // Step 5: build the trips and their shared stop patterns.
        let mut patterns: Vec<StopPattern> = Vec::new();
        let mut pattern_index: HashMap<(LineId, Option<u8>, Vec<StationId>), PatternId> =
            HashMap::new();
        let mut trips: Vec<TripSchedule> = Vec::new();
        for trip in &feed.trips {
            let Some(&line) = line_index.get(trip.route_id.as_str()) else {
                return Err(GtfsError::UnknownId {
                    kind: "route",
                    id: trip.route_id.clone(),
                });
            };
            let Some(stop_times) = calls_by_trip.get(trip.trip_id.as_str()) else {
                // A trip without stop times cannot carry passengers.
                continue;
            };
            if stop_times.len() < 2 {
                continue;
            }
            let mut station_seq = Vec::with_capacity(stop_times.len());
            let mut calls = Vec::with_capacity(stop_times.len());
            for st in stop_times {
                let Some(&station) = stop_to_station.get(st.stop_id.as_str()) else {
                    return Err(GtfsError::UnknownId {
                        kind: "stop",
                        id: st.stop_id.clone(),
                    });
                };
                station_seq.push(station);
                calls.push(StopCall {
                    arrival: st.arrival_time,
                    departure: st.departure_time,
                });
            }
            let key = (line, trip.direction_id, station_seq.clone());
            let pattern = *pattern_index.entry(key).or_insert_with(|| {
                let id = PatternId(patterns.len());
                patterns.push(StopPattern {
                    line,
                    direction: trip.direction_id,
                    stations: station_seq,
                });
                id
            });
            let service = services.intern(&trip.service_id);
            trips.push(TripSchedule {
                trip_id: trip.trip_id.clone(),
                line,
                pattern,
                service,
                headsign: trip.trip_headsign.clone().filter(|s| !s.is_empty()),
                calls,
                frequencies: freq_by_trip
                    .get(trip.trip_id.as_str())
                    .cloned()
                    .unwrap_or_default(),
            });
        }

        // Step 6: record the lines that serve each station.
        for pattern in &patterns {
            for &station in &pattern.stations {
                let lines_of_station = &mut stations[station.0].lines;
                if !lines_of_station.contains(&pattern.line) {
                    lines_of_station.push(pattern.line);
                }
            }
        }
        for station in &mut stations {
            station.lines.sort();
        }

        // Step 7: build the station-level transfers.
        let mut transfers: Vec<StationTransfer> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for t in &feed.transfers {
            // Type 3 means that no transfer is possible.
            if t.transfer_type == Some(3) {
                continue;
            }
            let from = stop_to_station
                .get(t.from_stop_id.as_str())
                .or_else(|| station_index.get(t.from_stop_id.as_str()));
            let to = stop_to_station
                .get(t.to_stop_id.as_str())
                .or_else(|| station_index.get(t.to_stop_id.as_str()));
            let (Some(&from), Some(&to)) = (from, to) else {
                continue;
            };
            if from != to && seen.insert((from, to)) {
                transfers.push(StationTransfer {
                    from,
                    to,
                    min_transfer_secs: t.min_transfer_time,
                });
            }
        }

        Ok(RailNetwork {
            lines,
            stations,
            patterns,
            transfers,
            trips,
            services,
            station_index,
            code_index,
            alias_index,
            stop_to_station,
            line_index,
        })
    }

    /// Get all lines.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// Get one line.
    pub fn line(&self, id: LineId) -> &Line {
        &self.lines[id.0]
    }

    /// Get all stations.
    pub fn stations(&self) -> &[Station] {
        &self.stations
    }

    /// Get one station.
    pub fn station(&self, id: StationId) -> &Station {
        &self.stations[id.0]
    }

    /// Get all stop patterns.
    pub fn patterns(&self) -> &[StopPattern] {
        &self.patterns
    }

    /// Get one stop pattern.
    pub fn pattern(&self, id: PatternId) -> &StopPattern {
        &self.patterns[id.0]
    }

    /// Get the stop patterns of one line.
    pub fn patterns_for_line(&self, line: LineId) -> impl Iterator<Item = &StopPattern> {
        self.patterns.iter().filter(move |p| p.line == line)
    }

    /// Get all station-level transfers.
    pub fn transfers(&self) -> &[StationTransfer] {
        &self.transfers
    }

    /// Get the number of trips in the network.
    pub fn trip_count(&self) -> usize {
        self.trips.len()
    }

    /// Find a line by its GTFS route identifier.
    pub fn line_by_route_id(&self, route_id: &str) -> Option<LineId> {
        self.line_index.get(route_id).copied()
    }

    /// Find a station by its GTFS identifier.
    pub fn station_by_gtfs_id(&self, gtfs_id: &str) -> Option<StationId> {
        self.station_index.get(gtfs_id).copied()
    }

    /// Find a station by its public code, for example `NS1`.
    ///
    /// The search ignores case.
    pub fn station_by_code(&self, code: &str) -> Option<StationId> {
        self.code_index.get(&code.to_ascii_uppercase()).copied()
    }

    /// Find a station by its public name, for example `Jurong East`.
    ///
    /// The search ignores case.
    pub fn station_by_name(&self, name: &str) -> Option<StationId> {
        self.stations
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
            .map(StationId)
    }

    /// Find a station by any spelling of any of its codes.
    ///
    /// The alias runs through [`alias::normalize`], so `NS1`, `ns-1`,
    /// and `NS 1` all reach the same station, and every code of an
    /// interchange reaches it too. Station names are not aliases:
    /// several stations share a name, so a name in a link would name
    /// an arbitrary one. Use [`RailNetwork::station_by_name`] where
    /// the ambiguity is acceptable.
    ///
    /// ```no_run
    /// # use mrt_gtfs::{GtfsFeed, RailNetwork};
    /// # let feed = GtfsFeed::from_zip_path("data/singapore-gtfs.zip").unwrap();
    /// # let network = RailNetwork::from_feed(&feed).unwrap();
    /// let station = network.station_by_alias("ns-1").unwrap();
    /// assert_eq!(network.station(station).name, "Jurong East");
    /// ```
    pub fn station_by_alias(&self, alias: &str) -> Option<StationId> {
        self.alias_index.get(&alias::normalize(alias)).copied()
    }

    /// Get the station that contains the given stop.
    pub fn station_for_stop(&self, stop_id: &str) -> Option<StationId> {
        self.stop_to_station.get(stop_id).copied()
    }

    /// Report whether the station is an interchange.
    ///
    /// A station is an interchange when lines with more than one
    /// distinct display name serve it. The name comparison absorbs
    /// feeds that split one line into several route entries. The
    /// official LTA feed does this for the Circle Line.
    pub fn is_interchange(&self, station: StationId) -> bool {
        let mut names: Vec<&str> = self.stations[station.0]
            .lines
            .iter()
            .map(|&id| self.lines[id.0].name.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        names.len() > 1
    }

    /// Get the identifiers of all interchange stations.
    ///
    /// See [`RailNetwork::is_interchange`] for the definition.
    pub fn interchanges(&self) -> impl Iterator<Item = StationId> + '_ {
        (0..self.stations.len())
            .map(StationId)
            .filter(|&id| self.is_interchange(id))
    }

    /// Report whether a service operates on a date.
    ///
    /// The function returns `false` for an unknown service identifier.
    pub fn service_active(&self, service_id: &str, date: ServiceDate) -> bool {
        match self.services.lookup(service_id) {
            Some(idx) => self.services.active(idx, date),
            None => false,
        }
    }
}

/// Build the alias table: a normalized station code to its station.
///
/// Codes alone make the table. A station name is not an alias: the
/// feed carries names that two stations share, for example `Bukit
/// Panjang` on the Downtown Line and on the Bukit Panjang LRT.
fn build_alias_index(stations: &[Station]) -> HashMap<String, StationId> {
    let mut index: HashMap<String, StationId> = HashMap::new();
    for (i, station) in stations.iter().enumerate() {
        for code in &station.codes {
            let key = alias::normalize(code);
            if !key.is_empty() {
                index.insert(key, StationId(i));
            }
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station(name: &str, codes: &[&str]) -> Station {
        Station {
            gtfs_id: name.to_string(),
            name: name.to_string(),
            codes: codes.iter().map(|c| c.to_string()).collect(),
            lat: None,
            lon: None,
            platform_stop_ids: Vec::new(),
            lines: Vec::new(),
        }
    }

    #[test]
    fn the_alias_index_holds_every_code() {
        let stations = [station("Jurong East", &["NS1", "EW24"])];
        let index = build_alias_index(&stations);
        assert_eq!(index.get("ns1"), Some(&StationId(0)));
        assert_eq!(index.get("ew24"), Some(&StationId(0)));
    }

    #[test]
    fn a_station_name_is_not_an_alias() {
        // Two stations share the name, so neither owns it.
        let stations = [
            station("Bukit Panjang", &["DT1"]),
            station("Bukit Panjang", &["BP6"]),
        ];
        let index = build_alias_index(&stations);
        assert_eq!(index.get("bukitpanjang"), None);
        assert_eq!(index.get("dt1"), Some(&StationId(0)));
        assert_eq!(index.get("bp6"), Some(&StationId(1)));
    }

    #[test]
    fn a_station_without_a_code_adds_no_alias() {
        let stations = [
            station("Woodlands Depot", &[]),
            station("Sengkang", &["NE16"]),
        ];
        let index = build_alias_index(&stations);
        assert_eq!(index.len(), 1);
        assert_eq!(index.get("ne16"), Some(&StationId(1)));
    }
}
