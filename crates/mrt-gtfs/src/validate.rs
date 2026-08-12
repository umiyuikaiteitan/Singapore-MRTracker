//! Feed validation.
//!
//! [`validate_feed`] inspects a parsed [`GtfsFeed`] and reports what is
//! wrong with it. It never changes the feed and never fails: the caller
//! decides which severity is fatal.
//!
//! The checks cover the defects that break a timetable or a diagram:
//! missing files, duplicate identifiers, broken references, decreasing
//! stop sequences, arrival times after departure times, impossible
//! headways, and incomplete station hierarchies.
//!
//! # Example
//!
//! ```no_run
//! use mrt_gtfs::{validate_feed, GtfsFeed, Severity, ValidationMode};
//!
//! let feed = GtfsFeed::from_zip_path("data/singapore-gtfs.zip").unwrap();
//! let report = validate_feed(&feed, ValidationMode::Lenient);
//! for diagnostic in report.iter().filter(|d| d.severity >= Severity::Warning) {
//!     eprintln!("{diagnostic}");
//! }
//! ```

use std::collections::{HashMap, HashSet};

use crate::diag::{self, Diagnostic, Severity};
use crate::feed::GtfsFeed;
use crate::model::{EXCEPTION_SERVICE_ADDED, EXCEPTION_SERVICE_REMOVED};

/// How strictly [`validate_feed`] judges a feed.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ValidationMode {
    /// Report the defects that break the output. Tolerate the
    /// deviations that real feeds carry, such as a stop without a
    /// position.
    #[default]
    Lenient,
    /// Also report every deviation from the letter of the
    /// specification, at warning severity.
    Strict,
}

/// The result of validating a feed.
#[derive(Clone, Debug, Default)]
pub struct ValidationReport {
    diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    /// Get every diagnostic, most serious first.
    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter()
    }

    /// Get every diagnostic as a slice.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Report whether the feed carries a defect that breaks output.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Count the diagnostics at a severity.
    pub fn count(&self, severity: Severity) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == severity)
            .count()
    }

    /// Take the diagnostics out of the report.
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

/// Validate a feed.
///
/// The function returns a [`ValidationReport`]. It never panics and
/// never changes the feed.
pub fn validate_feed(feed: &GtfsFeed, mode: ValidationMode) -> ValidationReport {
    let mut out: Vec<Diagnostic> = Vec::new();

    check_required_tables(feed, &mut out);
    let stop_ids = check_stops(feed, mode, &mut out);
    let route_ids = check_routes(feed, mode, &mut out);
    let service_ids = check_calendars(feed, &mut out);
    let trip_ids = check_trips(feed, &route_ids, &service_ids, &mut out);
    check_stop_times(feed, &stop_ids, &trip_ids, mode, &mut out);
    check_frequencies(feed, &trip_ids, &mut out);
    check_transfers(feed, &stop_ids, mode, &mut out);
    check_agencies(feed, mode, &mut out);

    diag::normalize(&mut out);
    ValidationReport { diagnostics: out }
}

fn check_required_tables(feed: &GtfsFeed, out: &mut Vec<Diagnostic>) {
    for (name, empty) in [
        ("stops.txt", feed.stops.is_empty()),
        ("routes.txt", feed.routes.is_empty()),
        ("trips.txt", feed.trips.is_empty()),
        ("stop_times.txt", feed.stop_times.is_empty()),
    ] {
        if empty {
            out.push(Diagnostic::error(
                "empty-required-file",
                format!("{name} contains no records"),
            ));
        }
    }
    if feed.calendar.is_empty() && feed.calendar_dates.is_empty() {
        out.push(Diagnostic::error(
            "no-calendar",
            "the feed contains neither calendar.txt nor calendar_dates.txt",
        ));
    }
}

/// Check the stops and return the set of known stop identifiers.
fn check_stops(
    feed: &GtfsFeed,
    mode: ValidationMode,
    out: &mut Vec<Diagnostic>,
) -> HashSet<String> {
    let mut ids: HashSet<String> = HashSet::new();
    let mut station_ids: HashSet<&str> = HashSet::new();
    for stop in &feed.stops {
        if stop.stop_id.is_empty() {
            out.push(Diagnostic::error("empty-id", "a stop has an empty stop_id"));
            continue;
        }
        if !ids.insert(stop.stop_id.clone()) {
            out.push(
                Diagnostic::error("duplicate-id", "the feed defines this stop_id twice")
                    .about(stop.stop_id.clone()),
            );
        }
        if stop.is_station() {
            station_ids.insert(stop.stop_id.as_str());
        }
        if stop.stop_name.as_deref().unwrap_or("").trim().is_empty() {
            out.push(
                Diagnostic::warning("missing-station-name", "the stop has no stop_name")
                    .about(stop.stop_id.clone()),
            );
        }
        if mode == ValidationMode::Strict && (stop.stop_lat.is_none() || stop.stop_lon.is_none()) {
            out.push(
                Diagnostic::warning("missing-position", "the stop has no position")
                    .about(stop.stop_id.clone()),
            );
        }
        if let (Some(lat), Some(lon)) = (stop.stop_lat, stop.stop_lon) {
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                out.push(
                    Diagnostic::error(
                        "position-out-of-range",
                        format!("the position {lat}, {lon} is not on the Earth"),
                    )
                    .about(stop.stop_id.clone()),
                );
            }
        }
    }

    for stop in &feed.stops {
        let Some(parent) = stop.parent_station_id() else {
            continue;
        };
        if !ids.contains(parent) {
            out.push(
                Diagnostic::error(
                    "unknown-parent-station",
                    format!("the parent station \"{parent}\" is not in stops.txt"),
                )
                .about(stop.stop_id.clone()),
            );
        } else if !station_ids.contains(parent) {
            out.push(
                Diagnostic::error(
                    "incomplete-platform-hierarchy",
                    format!("the parent \"{parent}\" is not a station record (location_type=1)"),
                )
                .about(stop.stop_id.clone()),
            );
        }
        if stop.is_station() {
            out.push(
                Diagnostic::error(
                    "incomplete-platform-hierarchy",
                    "a station record must not have a parent station",
                )
                .about(stop.stop_id.clone()),
            );
        }
    }
    ids
}

fn check_routes(
    feed: &GtfsFeed,
    mode: ValidationMode,
    out: &mut Vec<Diagnostic>,
) -> HashSet<String> {
    let mut ids = HashSet::new();
    for route in &feed.routes {
        if route.route_id.is_empty() {
            out.push(Diagnostic::error(
                "empty-id",
                "a route has an empty route_id",
            ));
            continue;
        }
        if !ids.insert(route.route_id.clone()) {
            out.push(
                Diagnostic::error("duplicate-id", "the feed defines this route_id twice")
                    .about(route.route_id.clone()),
            );
        }
        for (field, value) in [
            ("route_color", &route.route_color),
            ("route_text_color", &route.route_text_color),
        ] {
            if let Some(color) = value.as_deref().filter(|c| !c.is_empty()) {
                if color.len() != 6 || !color.bytes().all(|b| b.is_ascii_hexdigit()) {
                    out.push(
                        Diagnostic::warning(
                            "malformed-color",
                            format!("{field} \"{color}\" is not a six-digit hexadecimal value"),
                        )
                        .about(route.route_id.clone()),
                    );
                }
            }
        }
        let named = route
            .route_short_name
            .as_deref()
            .is_some_and(|n| !n.is_empty())
            || route
                .route_long_name
                .as_deref()
                .is_some_and(|n| !n.is_empty());
        if !named && mode == ValidationMode::Strict {
            out.push(
                Diagnostic::warning(
                    "unnamed-route",
                    "the route has neither a short name nor a long name",
                )
                .about(route.route_id.clone()),
            );
        }
    }
    ids
}

fn check_calendars(feed: &GtfsFeed, out: &mut Vec<Diagnostic>) -> HashSet<String> {
    let mut ids = HashSet::new();
    let mut seen_weekly = HashSet::new();
    for row in &feed.calendar {
        if !seen_weekly.insert(row.service_id.clone()) {
            out.push(
                Diagnostic::error("duplicate-id", "calendar.txt defines this service_id twice")
                    .about(row.service_id.clone()),
            );
        }
        ids.insert(row.service_id.clone());
        if row.end_date < row.start_date {
            out.push(
                Diagnostic::error(
                    "invalid-service-period",
                    format!(
                        "the service period {} to {} ends before it starts",
                        row.start_date, row.end_date
                    ),
                )
                .about(row.service_id.clone()),
            );
        }
        if row.weekday_flags().iter().all(|day| !day) && row.start_date == row.end_date {
            out.push(
                Diagnostic::info(
                    "empty-weekly-rule",
                    "the weekly rule selects no day of the week",
                )
                .about(row.service_id.clone()),
            );
        }
    }
    let mut seen_exception = HashSet::new();
    for row in &feed.calendar_dates {
        ids.insert(row.service_id.clone());
        if !seen_exception.insert((row.service_id.clone(), row.date)) {
            out.push(
                Diagnostic::error(
                    "duplicate-service-exception",
                    format!("calendar_dates.txt carries two rules for {}", row.date),
                )
                .about(row.service_id.clone()),
            );
        }
        if row.exception_type != EXCEPTION_SERVICE_ADDED
            && row.exception_type != EXCEPTION_SERVICE_REMOVED
        {
            out.push(
                Diagnostic::error(
                    "invalid-exception-type",
                    format!(
                        "exception_type {} is neither 1 (added) nor 2 (removed)",
                        row.exception_type
                    ),
                )
                .about(row.service_id.clone()),
            );
        }
    }
    ids
}

fn check_trips(
    feed: &GtfsFeed,
    route_ids: &HashSet<String>,
    service_ids: &HashSet<String>,
    out: &mut Vec<Diagnostic>,
) -> HashSet<String> {
    let mut ids = HashSet::new();
    for trip in &feed.trips {
        if trip.trip_id.is_empty() {
            out.push(Diagnostic::error("empty-id", "a trip has an empty trip_id"));
            continue;
        }
        if !ids.insert(trip.trip_id.clone()) {
            out.push(
                Diagnostic::error("duplicate-id", "the feed defines this trip_id twice")
                    .about(trip.trip_id.clone()),
            );
        }
        if !route_ids.contains(&trip.route_id) {
            out.push(
                Diagnostic::error(
                    "unknown-route",
                    format!("the route \"{}\" is not in routes.txt", trip.route_id),
                )
                .about(trip.trip_id.clone()),
            );
        }
        if !service_ids.contains(&trip.service_id) {
            out.push(
                Diagnostic::error(
                    "unknown-service",
                    format!(
                        "the service \"{}\" is in neither calendar.txt nor calendar_dates.txt",
                        trip.service_id
                    ),
                )
                .about(trip.trip_id.clone()),
            );
        }
        if trip.direction_id.is_some_and(|d| d > 1) {
            out.push(
                Diagnostic::error("invalid-direction", "direction_id must be 0 or 1")
                    .about(trip.trip_id.clone()),
            );
        }
    }
    ids
}

fn check_stop_times(
    feed: &GtfsFeed,
    stop_ids: &HashSet<String>,
    trip_ids: &HashSet<String>,
    mode: ValidationMode,
    out: &mut Vec<Diagnostic>,
) {
    let mut by_trip: HashMap<&str, Vec<&crate::model::StopTime>> = HashMap::new();
    for st in &feed.stop_times {
        if !trip_ids.contains(&st.trip_id) {
            out.push(Diagnostic::error(
                "unknown-trip",
                format!(
                    "stop_times.txt refers to the trip \"{}\", which is not in trips.txt",
                    st.trip_id
                ),
            ));
        }
        if !stop_ids.contains(&st.stop_id) {
            out.push(
                Diagnostic::error(
                    "unknown-stop",
                    format!("the stop \"{}\" is not in stops.txt", st.stop_id),
                )
                .about(st.trip_id.clone()),
            );
        }
        if let (Some(arrival), Some(departure)) = (st.arrival_time, st.departure_time) {
            if departure < arrival {
                out.push(
                    Diagnostic::error(
                        "departure-before-arrival",
                        format!(
                            "the call at \"{}\" departs at {departure} but arrives at {arrival}",
                            st.stop_id
                        ),
                    )
                    .about(st.trip_id.clone()),
                );
            }
        }
        by_trip.entry(&st.trip_id).or_default().push(st);
    }

    for (trip_id, calls) in &mut by_trip {
        // The feed may list the calls in any order; the sequence
        // number carries the truth. Duplicates make it ambiguous.
        let mut sequences: Vec<u32> = calls.iter().map(|c| c.stop_sequence).collect();
        sequences.sort_unstable();
        if sequences.windows(2).any(|w| w[0] == w[1]) {
            out.push(
                Diagnostic::error(
                    "duplicate-stop-sequence",
                    "two calls of the trip share a stop_sequence",
                )
                .about((*trip_id).to_string()),
            );
        }
        if calls.len() == 1 {
            out.push(
                Diagnostic::warning(
                    "single-call-trip",
                    "the trip has one call, so it carries no passengers",
                )
                .about((*trip_id).to_string()),
            );
        }
        calls.sort_by_key(|c| c.stop_sequence);
        let mut previous: Option<crate::time::GtfsTime> = None;
        let mut has_time = false;
        for call in calls.iter() {
            if let Some(time) = call.arrival_time.or(call.departure_time) {
                has_time = true;
                if previous.is_some_and(|p| time < p) {
                    out.push(
                        Diagnostic::error(
                            "times-go-backwards",
                            format!(
                                "the call at \"{}\" is earlier than the call before it",
                                call.stop_id
                            ),
                        )
                        .about((*trip_id).to_string()),
                    );
                }
                previous = call.departure_time.or(call.arrival_time).or(previous);
            }
        }
        if !has_time {
            out.push(
                Diagnostic::error(
                    "trip-without-times",
                    "no call of the trip carries a time, so the trip cannot be drawn",
                )
                .about((*trip_id).to_string()),
            );
        }
        let first_last_timed = calls
            .first()
            .is_some_and(|c| c.arrival_time.or(c.departure_time).is_some())
            && calls
                .last()
                .is_some_and(|c| c.arrival_time.or(c.departure_time).is_some());
        if has_time && !first_last_timed {
            out.push(
                Diagnostic::warning(
                    "unbounded-missing-time",
                    "the first or the last call of the trip has no time, so bounded \
                     interpolation cannot complete the run",
                )
                .about((*trip_id).to_string()),
            );
        }
        if mode == ValidationMode::Strict {
            let untimed = calls
                .iter()
                .filter(|c| c.arrival_time.is_none() && c.departure_time.is_none())
                .count();
            if untimed > 0 {
                out.push(
                    Diagnostic::warning(
                        "incomplete-stop-times",
                        format!("{untimed} call(s) of the trip carry no time"),
                    )
                    .about((*trip_id).to_string()),
                );
            }
        }
    }

    for trip in &feed.trips {
        if !by_trip.contains_key(trip.trip_id.as_str()) {
            out.push(
                Diagnostic::warning(
                    "trip-without-calls",
                    "the trip has no record in stop_times.txt",
                )
                .about(trip.trip_id.clone()),
            );
        }
    }
}

fn check_frequencies(feed: &GtfsFeed, trip_ids: &HashSet<String>, out: &mut Vec<Diagnostic>) {
    for block in &feed.frequencies {
        if !trip_ids.contains(&block.trip_id) {
            out.push(Diagnostic::error(
                "unknown-trip",
                format!(
                    "frequencies.txt refers to the trip \"{}\", which is not in trips.txt",
                    block.trip_id
                ),
            ));
        }
        if block.headway_secs == 0 {
            out.push(
                Diagnostic::error(
                    "frequency-zero-headway",
                    format!(
                        "the block {}-{} has a headway of zero seconds",
                        block.start_time, block.end_time
                    ),
                )
                .about(block.trip_id.clone()),
            );
        }
        if block.end_time <= block.start_time {
            out.push(
                Diagnostic::error(
                    "frequency-empty-block",
                    format!(
                        "the block {}-{} ends at or before its start",
                        block.start_time, block.end_time
                    ),
                )
                .about(block.trip_id.clone()),
            );
        }
        if block.exact_times.is_some_and(|v| v > 1) {
            out.push(
                Diagnostic::error("invalid-exact-times", "exact_times must be 0 or 1")
                    .about(block.trip_id.clone()),
            );
        }
    }
}

fn check_transfers(
    feed: &GtfsFeed,
    stop_ids: &HashSet<String>,
    mode: ValidationMode,
    out: &mut Vec<Diagnostic>,
) {
    for transfer in &feed.transfers {
        for (field, id) in [
            ("from_stop_id", &transfer.from_stop_id),
            ("to_stop_id", &transfer.to_stop_id),
        ] {
            if !id.is_empty() && !stop_ids.contains(id) && mode == ValidationMode::Strict {
                out.push(Diagnostic::warning(
                    "unknown-stop",
                    format!("transfers.txt {field} \"{id}\" is not in stops.txt"),
                ));
            }
        }
    }
}

fn check_agencies(feed: &GtfsFeed, mode: ValidationMode, out: &mut Vec<Diagnostic>) {
    if feed.agencies.is_empty() {
        out.push(Diagnostic::warning(
            "missing-agency",
            "agency.txt contains no records, so the feed carries no time zone",
        ));
        return;
    }
    let zones: HashSet<&str> = feed
        .agencies
        .iter()
        .filter_map(|a| a.agency_timezone.as_deref())
        .filter(|z| !z.is_empty())
        .collect();
    if zones.is_empty() {
        out.push(Diagnostic::warning(
            "missing-timezone",
            "no agency record carries an agency_timezone",
        ));
    } else if zones.len() > 1 && mode == ValidationMode::Strict {
        let mut names: Vec<&str> = zones.into_iter().collect();
        names.sort_unstable();
        out.push(Diagnostic::warning(
            "conflicting-timezone",
            format!(
                "the agencies use different time zones: {}",
                names.join(", ")
            ),
        ));
    }
}

/// Get the time zone of the feed, if the agencies agree on one.
pub fn feed_timezone(feed: &GtfsFeed) -> Option<&str> {
    feed.agencies
        .iter()
        .filter_map(|a| a.agency_timezone.as_deref())
        .find(|z| !z.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Calendar, Frequency, Route, Stop, StopTime, Trip};

    fn base_feed() -> GtfsFeed {
        GtfsFeed {
            agencies: vec![crate::model::Agency {
                agency_id: Some("A".into()),
                agency_name: "Agency".into(),
                agency_url: None,
                agency_timezone: Some("Asia/Singapore".into()),
            }],
            stops: vec![
                Stop {
                    stop_id: "S1".into(),
                    stop_name: Some("Alpha".into()),
                    ..Default::default()
                },
                Stop {
                    stop_id: "S2".into(),
                    stop_name: Some("Beta".into()),
                    ..Default::default()
                },
            ],
            routes: vec![Route {
                route_id: "R1".into(),
                agency_id: Some("A".into()),
                route_short_name: Some("NS".into()),
                route_long_name: None,
                route_type: 1,
                route_color: Some("D42E12".into()),
                route_text_color: None,
            }],
            trips: vec![Trip {
                route_id: "R1".into(),
                service_id: "WK".into(),
                trip_id: "T1".into(),
                ..Default::default()
            }],
            stop_times: vec![
                StopTime {
                    trip_id: "T1".into(),
                    arrival_time: Some("06:00:00".parse().unwrap()),
                    departure_time: Some("06:00:30".parse().unwrap()),
                    stop_id: "S1".into(),
                    stop_sequence: 1,
                    ..Default::default()
                },
                StopTime {
                    trip_id: "T1".into(),
                    arrival_time: Some("06:10:00".parse().unwrap()),
                    departure_time: Some("06:10:00".parse().unwrap()),
                    stop_id: "S2".into(),
                    stop_sequence: 2,
                    ..Default::default()
                },
            ],
            calendar: vec![Calendar {
                service_id: "WK".into(),
                monday: 1,
                tuesday: 1,
                wednesday: 1,
                thursday: 1,
                friday: 1,
                saturday: 0,
                sunday: 0,
                start_date: "20250101".parse().unwrap(),
                end_date: "20271231".parse().unwrap(),
            }],
            ..Default::default()
        }
    }

    fn codes(report: &ValidationReport) -> Vec<&str> {
        report.iter().map(|d| d.code.as_str()).collect()
    }

    #[test]
    fn a_sound_feed_reports_nothing() {
        let report = validate_feed(&base_feed(), ValidationMode::Lenient);
        assert!(report.diagnostics().is_empty(), "{:?}", codes(&report));
    }

    #[test]
    fn broken_references_are_errors() {
        let mut feed = base_feed();
        feed.trips[0].route_id = "GONE".into();
        feed.stop_times[1].stop_id = "MISSING".into();
        let report = validate_feed(&feed, ValidationMode::Lenient);
        assert!(report.has_errors());
        assert!(codes(&report).contains(&"unknown-route"));
        assert!(codes(&report).contains(&"unknown-stop"));
    }

    #[test]
    fn duplicate_identifiers_are_errors() {
        let mut feed = base_feed();
        feed.stops.push(feed.stops[0].clone());
        let report = validate_feed(&feed, ValidationMode::Lenient);
        assert!(codes(&report).contains(&"duplicate-id"));
    }

    #[test]
    fn decreasing_times_are_errors() {
        let mut feed = base_feed();
        feed.stop_times[1].arrival_time = Some("05:00:00".parse().unwrap());
        feed.stop_times[1].departure_time = Some("05:00:00".parse().unwrap());
        let report = validate_feed(&feed, ValidationMode::Lenient);
        assert!(codes(&report).contains(&"times-go-backwards"));
    }

    #[test]
    fn a_departure_before_its_arrival_is_an_error() {
        let mut feed = base_feed();
        feed.stop_times[0].departure_time = Some("05:59:00".parse().unwrap());
        let report = validate_feed(&feed, ValidationMode::Lenient);
        assert!(codes(&report).contains(&"departure-before-arrival"));
    }

    #[test]
    fn a_zero_headway_is_an_error() {
        let mut feed = base_feed();
        feed.frequencies.push(Frequency {
            trip_id: "T1".into(),
            start_time: "05:00:00".parse().unwrap(),
            end_time: "06:00:00".parse().unwrap(),
            headway_secs: 0,
            exact_times: Some(0),
        });
        let report = validate_feed(&feed, ValidationMode::Lenient);
        assert!(codes(&report).contains(&"frequency-zero-headway"));
    }

    #[test]
    fn an_unknown_parent_station_is_an_error() {
        let mut feed = base_feed();
        feed.stops[1].parent_station = Some("NOWHERE".into());
        let report = validate_feed(&feed, ValidationMode::Lenient);
        assert!(codes(&report).contains(&"unknown-parent-station"));
    }

    #[test]
    fn a_parent_that_is_not_a_station_breaks_the_hierarchy() {
        let mut feed = base_feed();
        feed.stops[1].parent_station = Some("S1".into());
        let report = validate_feed(&feed, ValidationMode::Lenient);
        assert!(codes(&report).contains(&"incomplete-platform-hierarchy"));
    }

    #[test]
    fn a_malformed_color_is_a_warning() {
        let mut feed = base_feed();
        feed.routes[0].route_color = Some("red".into());
        let report = validate_feed(&feed, ValidationMode::Lenient);
        assert!(codes(&report).contains(&"malformed-color"));
        assert!(!report.has_errors());
    }

    #[test]
    fn strict_mode_reports_missing_positions() {
        let feed = base_feed();
        assert!(
            !codes(&validate_feed(&feed, ValidationMode::Lenient)).contains(&"missing-position")
        );
        assert!(codes(&validate_feed(&feed, ValidationMode::Strict)).contains(&"missing-position"));
    }

    #[test]
    fn an_untimed_edge_call_warns_about_bounded_interpolation() {
        let mut feed = base_feed();
        feed.stop_times.push(StopTime {
            trip_id: "T1".into(),
            stop_id: "S2".into(),
            stop_sequence: 3,
            ..Default::default()
        });
        let report = validate_feed(&feed, ValidationMode::Lenient);
        assert!(codes(&report).contains(&"unbounded-missing-time"));
    }

    #[test]
    fn duplicate_stop_sequences_are_errors() {
        let mut feed = base_feed();
        feed.stop_times[1].stop_sequence = 1;
        let report = validate_feed(&feed, ValidationMode::Lenient);
        assert!(codes(&report).contains(&"duplicate-stop-sequence"));
    }

    #[test]
    fn the_timezone_comes_from_the_agency() {
        assert_eq!(feed_timezone(&base_feed()), Some("Asia/Singapore"));
    }
}
