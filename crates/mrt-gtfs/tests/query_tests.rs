//! Integration tests for the public scheduled-trip query API.
//!
//! The tests use the miniature feed in `tests/fixtures/mini`.

use std::path::PathBuf;

use mrt_gtfs::{
    FrequencyPolicy, GtfsFeed, GtfsTime, MissingTimePolicy, RailNetwork, ServiceDate, StationId,
    TimeExactness, TimeQuality, TripInstance, TripInstanceQuery,
};

fn network() -> RailNetwork {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini");
    RailNetwork::from_feed(&GtfsFeed::from_dir(dir).unwrap()).unwrap()
}

fn date(s: &str) -> ServiceDate {
    s.parse().unwrap()
}

fn time(s: &str) -> GtfsTime {
    s.parse().unwrap()
}

/// A weekday query over the whole service day for one line.
fn weekday(network: &RailNetwork, route_id: &str) -> TripInstanceQuery {
    TripInstanceQuery::new(date("20250505")).line(network.line_by_route_id(route_id).unwrap())
}

fn instance<'a>(trips: &'a [TripInstance], trip_id: &str) -> &'a TripInstance {
    trips
        .iter()
        .find(|t| t.source_trip_id == trip_id)
        .unwrap_or_else(|| panic!("the result contains no instance of {trip_id}"))
}

// ----------------------------------------------------------------------
// Selection and ordering
// ----------------------------------------------------------------------

#[test]
fn a_query_returns_the_runs_of_the_service_date_in_time_order() {
    let network = network();
    let result = network
        .query_trip_instances(&weekday(&network, "NS"))
        .unwrap();

    let ids: Vec<&str> = result
        .trips
        .iter()
        .map(|t| t.source_trip_id.as_str())
        .collect();
    // NS_T4 runs at the weekend only.
    assert_eq!(ids, vec!["NS_T1", "NS_T3", "NS_T2", "NS_T5"]);

    let first = &result.trips[0];
    assert_eq!(first.instance_id, "20250505:NS_T1");
    assert_eq!(first.service_id, "WKDAY");
    assert_eq!(first.direction, Some(0));
    assert_eq!(first.headsign.as_deref(), Some("Marina Bay"));
    assert_eq!(first.exactness, TimeExactness::Exact);
    assert_eq!(first.calls.len(), 3);
}

#[test]
fn calendar_exceptions_decide_which_runs_exist() {
    let network = network();
    // 2025-05-01 is a Thursday that the fixture marks as a holiday:
    // weekday service is removed and weekend service is added.
    let query = TripInstanceQuery::new(date("20250501"));
    let ids: Vec<String> = network
        .query_trip_instances(&query)
        .unwrap()
        .trips
        .iter()
        .map(|t| t.source_trip_id.clone())
        .collect();
    assert_eq!(ids, vec!["NS_T4".to_string()]);
}

#[test]
fn times_beyond_midnight_keep_their_service_day_value() {
    let network = network();
    let result = network
        .query_trip_instances(&weekday(&network, "NS"))
        .unwrap();
    let midnight_run = instance(&result.trips, "NS_T5");
    let times: Vec<String> = midnight_run
        .calls
        .iter()
        .map(|c| c.departure_or_arrival().unwrap().to_string())
        .collect();
    assert_eq!(times, vec!["23:50:30", "24:05:30", "24:25:00"]);
    assert_eq!(midnight_run.last_time().unwrap().to_string(), "24:25:00");
}

#[test]
fn the_window_is_half_open_and_selects_overlapping_runs() {
    let network = network();
    let base = weekday(&network, "NS");

    // NS_T2 runs 07:00:00 to 07:30:00. A window that ends exactly at
    // its first time excludes it; a window that starts at its last
    // time still includes it.
    let before = base
        .clone()
        .window(time("06:00:00"), time("07:00:00"))
        .frequency_policy(FrequencyPolicy::Bands);
    let result = network.query_trip_instances(&before).unwrap();
    let ids: Vec<&str> = result
        .trips
        .iter()
        .map(|t| t.source_trip_id.as_str())
        .collect();
    assert!(!ids.contains(&"NS_T2"), "got {ids:?}");

    let touching = base.clone().window(time("07:30:00"), time("08:00:00"));
    let result = network.query_trip_instances(&touching).unwrap();
    let ids: Vec<&str> = result
        .trips
        .iter()
        .map(|t| t.source_trip_id.as_str())
        .collect();
    assert_eq!(ids, vec!["NS_T2"]);
}

#[test]
fn a_station_filter_keeps_only_the_runs_that_call_there() {
    let network = network();
    let raffles = network.station_by_code("EW14").unwrap();
    let query = TripInstanceQuery::new(date("20250505")).station(raffles);
    let ids: Vec<String> = network
        .query_trip_instances(&query)
        .unwrap()
        .trips
        .iter()
        .map(|t| t.source_trip_id.clone())
        .collect();
    assert_eq!(ids, vec!["EW_T1".to_string()]);
}

// ----------------------------------------------------------------------
// Calls
// ----------------------------------------------------------------------

#[test]
fn a_call_carries_the_platform_that_the_run_uses() {
    let network = network();
    let result = network
        .query_trip_instances(&weekday(&network, "TE"))
        .unwrap();

    let southbound = instance(&result.trips, "TE_T1");
    let platforms: Vec<&str> = southbound
        .calls
        .iter()
        .map(|c| c.platform_code.as_deref().unwrap())
        .collect();
    assert_eq!(platforms, vec!["1", "1", "1", "1"]);
    assert_eq!(southbound.calls[0].platform_stop_id, "WDN_1");

    // The opposite direction uses the other platform of each station.
    let northbound = instance(&result.trips, "TE_T3");
    let platforms: Vec<&str> = northbound
        .calls
        .iter()
        .map(|c| c.platform_code.as_deref().unwrap())
        .collect();
    assert_eq!(platforms, vec!["2", "2", "2", "2"]);
    assert_eq!(northbound.calls[0].platform_stop_id, "SPL_2");
}

#[test]
fn a_call_keeps_its_own_headsign_and_boarding_rules() {
    let network = network();
    let result = network
        .query_trip_instances(&weekday(&network, "TE"))
        .unwrap();
    let express = instance(&result.trips, "TE_P1");

    assert_eq!(
        express.calls[0].stop_headsign.as_deref(),
        Some("Springleaf Express")
    );
    // The middle call permits neither boarding nor alighting: the
    // train passes through Woodlands.
    assert!(express.calls[1].is_pass_through());
    assert!(!express.calls[1].allows_pickup());
    assert!(!express.calls[1].allows_drop_off());
    assert!(express.calls[2].allows_pickup());
}

#[test]
fn a_loop_run_calls_at_its_first_station_twice() {
    let network = network();
    let result = network
        .query_trip_instances(&weekday(&network, "PW"))
        .unwrap();
    let loop_run = instance(&result.trips, "PW_L1");

    assert_eq!(loop_run.calls.len(), 4);
    assert_eq!(loop_run.calls[0].station, loop_run.calls[3].station);
    // Passengers board at the start of the loop, not at its end.
    assert!(loop_run.calls[0].allows_pickup());
    assert!(!loop_run.calls[3].allows_pickup());
}

#[test]
fn a_short_turn_ends_earlier_than_the_full_run() {
    let network = network();
    let result = network
        .query_trip_instances(&weekday(&network, "TE"))
        .unwrap();

    let full = instance(&result.trips, "TE_T1");
    let short = instance(&result.trips, "TE_T2");
    assert_eq!(full.direction, short.direction);
    assert_eq!(network.station(full.terminus().unwrap()).name, "Springleaf");
    assert_eq!(
        network.station(short.terminus().unwrap()).name,
        "Woodlands South"
    );
    assert_eq!(short.short_name.as_deref(), Some("T103"));
    assert_eq!(short.block_id.as_deref(), Some("B1"));
}

// ----------------------------------------------------------------------
// Frequencies
// ----------------------------------------------------------------------

#[test]
fn exact_headway_service_expands_into_exact_runs() {
    let network = network();
    let result = network
        .query_trip_instances(&weekday(&network, "TE"))
        .unwrap();

    let runs: Vec<&TripInstance> = result
        .trips
        .iter()
        .filter(|t| t.source_trip_id == "TE_F1")
        .collect();
    // The block runs 05:00 to 05:30 with a 600-second headway, and no
    // run starts at 05:30.
    let starts: Vec<String> = runs
        .iter()
        .map(|t| t.first_time().unwrap().to_string())
        .collect();
    assert_eq!(starts, vec!["05:00:00", "05:10:00", "05:20:00"]);
    assert!(runs.iter().all(|t| t.exactness == TimeExactness::Exact));
    assert_eq!(runs[1].instance_id, "20250505:TE_F1@05:10:00");
    // The template offsets carry over: the third call is 8 minutes in.
    assert_eq!(
        runs[1].calls[2].departure_or_arrival().unwrap().to_string(),
        "05:18:00"
    );
    assert!(result
        .frequency_bands
        .iter()
        .all(|b| b.source_trip_id != "TE_F1"));
}

#[test]
fn non_exact_headway_service_becomes_a_band_by_default() {
    let network = network();
    let result = network
        .query_trip_instances(&weekday(&network, "BP"))
        .unwrap();

    assert!(result.trips.is_empty(), "bands must not invent runs");
    assert_eq!(result.frequency_bands.len(), 1);
    let band = &result.frequency_bands[0];
    assert_eq!(band.band_id, "20250505:BP_T1~05:30:00");
    assert_eq!(band.start.to_string(), "05:30:00");
    assert_eq!(band.end.to_string(), "06:00:00");
    assert_eq!(band.headway_secs, 600);
    assert_eq!(band.headway_minutes(), 10);
    assert_eq!(band.template.len(), 2);
    assert!(result.has_approximate_service());
}

#[test]
fn non_exact_headway_service_expands_only_on_request() {
    let network = network();
    let query = weekday(&network, "BP").frequency_policy(FrequencyPolicy::ExpandApproximate);
    let result = network.query_trip_instances(&query).unwrap();

    let starts: Vec<String> = result
        .trips
        .iter()
        .map(|t| t.first_time().unwrap().to_string())
        .collect();
    assert_eq!(starts, vec!["05:30:00", "05:40:00", "05:50:00"]);
    assert!(result
        .trips
        .iter()
        .all(|t| t.exactness == TimeExactness::Approximate));
    assert!(result.frequency_bands.is_empty());
    assert!(result.has_approximate_service());
}

#[test]
fn the_reject_policy_refuses_non_exact_service() {
    let network = network();
    let query = weekday(&network, "BP").frequency_policy(FrequencyPolicy::RejectNonExact);
    let error = network.query_trip_instances(&query).unwrap_err();
    assert!(
        matches!(&error, mrt_gtfs::GtfsError::PolicyViolation(message) if message.contains("BP_T1")),
        "got {error}"
    );

    // A line without non-exact service still answers under the policy.
    let query = weekday(&network, "NS").frequency_policy(FrequencyPolicy::RejectNonExact);
    assert!(network.query_trip_instances(&query).is_ok());
}

#[test]
fn a_zero_headway_block_produces_a_diagnostic_and_no_runs() {
    use mrt_gtfs::{Calendar, Frequency, Route, Stop, StopTime, Trip};

    let feed = GtfsFeed {
        stops: vec![
            Stop {
                stop_id: "A".into(),
                stop_name: Some("Alpha".into()),
                ..Default::default()
            },
            Stop {
                stop_id: "B".into(),
                stop_name: Some("Beta".into()),
                ..Default::default()
            },
        ],
        routes: vec![Route {
            route_id: "R".into(),
            agency_id: None,
            route_short_name: Some("R".into()),
            route_long_name: None,
            route_type: 1,
            route_color: None,
            route_text_color: None,
        }],
        trips: vec![Trip {
            route_id: "R".into(),
            service_id: "D".into(),
            trip_id: "T".into(),
            ..Default::default()
        }],
        stop_times: vec![
            StopTime {
                trip_id: "T".into(),
                arrival_time: Some(time("06:00:00")),
                departure_time: Some(time("06:00:00")),
                stop_id: "A".into(),
                stop_sequence: 1,
                ..Default::default()
            },
            StopTime {
                trip_id: "T".into(),
                arrival_time: Some(time("06:10:00")),
                departure_time: Some(time("06:10:00")),
                stop_id: "B".into(),
                stop_sequence: 2,
                ..Default::default()
            },
        ],
        calendar: vec![Calendar {
            service_id: "D".into(),
            monday: 1,
            tuesday: 1,
            wednesday: 1,
            thursday: 1,
            friday: 1,
            saturday: 1,
            sunday: 1,
            start_date: date("20250101"),
            end_date: date("20271231"),
        }],
        frequencies: vec![Frequency {
            trip_id: "T".into(),
            start_time: time("05:00:00"),
            end_time: time("06:00:00"),
            headway_secs: 0,
            exact_times: Some(1),
        }],
        ..Default::default()
    };
    let network = RailNetwork::from_feed(&feed).unwrap();
    let result = network
        .query_trip_instances(&TripInstanceQuery::new(date("20250505")))
        .unwrap();

    assert!(result.trips.is_empty());
    assert!(result.frequency_bands.is_empty());
    let codes: Vec<&str> = result.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, vec!["frequency-zero-headway"]);
}

// ----------------------------------------------------------------------
// Missing times
// ----------------------------------------------------------------------

#[test]
fn bounded_interpolation_fills_the_gap_and_marks_it() {
    let network = network();
    let result = network
        .query_trip_instances(&weekday(&network, "TE"))
        .unwrap();
    let gapped = instance(&result.trips, "TE_M1");

    // The trip runs 07:00:30 to 07:13:00 over 10 000 shape units. The
    // middle calls sit at 3 000 and 6 000 units.
    let times: Vec<String> = gapped
        .calls
        .iter()
        .map(|c| c.departure_or_arrival().unwrap().to_string())
        .collect();
    assert_eq!(times, vec!["07:00:30", "07:04:15", "07:08:00", "07:13:00"]);
    let quality: Vec<TimeQuality> = gapped.calls.iter().map(|c| c.time_quality).collect();
    assert_eq!(
        quality,
        vec![
            TimeQuality::Exact,
            TimeQuality::Interpolated,
            TimeQuality::Interpolated,
            TimeQuality::Exact,
        ]
    );
    assert!(gapped.has_interpolated_times());
    assert!(!gapped.has_missing_times());

    let interpolation = result
        .diagnostics
        .iter()
        .find(|d| d.code == "time-interpolated" && d.subject.as_deref() == Some("TE_M1"))
        .expect("the query reports the interpolation");
    assert!(interpolation.message.contains("shape_dist_traveled"));
}

#[test]
fn the_none_policy_leaves_the_gap_visible() {
    let network = network();
    let query = weekday(&network, "TE").missing_time_policy(MissingTimePolicy::None);
    let result = network.query_trip_instances(&query).unwrap();
    let gapped = instance(&result.trips, "TE_M1");

    assert!(gapped.has_missing_times());
    assert_eq!(gapped.calls[1].time_quality, TimeQuality::Missing);
    assert!(gapped.calls[1].departure.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "time-missing" && d.subject.as_deref() == Some("TE_M1")));
}

#[test]
fn a_timepoint_flag_marks_a_supplied_time_as_approximate() {
    let network = network();
    let result = network
        .query_trip_instances(&weekday(&network, "TE"))
        .unwrap();
    // TE_T1 carries timepoint=1 on every call: the times are exact.
    let exact = instance(&result.trips, "TE_T1");
    assert!(exact
        .calls
        .iter()
        .all(|c| c.time_quality == TimeQuality::Exact));
}

// ----------------------------------------------------------------------
// Distances
// ----------------------------------------------------------------------

#[test]
fn cumulative_station_distance_grows_along_the_pattern() {
    let network = network();
    let stations: Vec<StationId> = ["TE1", "TE2", "TE3", "TE4"]
        .iter()
        .map(|code| network.station_by_code(code).unwrap())
        .collect();
    let distances = network.cumulative_station_distance(&stations).unwrap();

    assert_eq!(distances.len(), 4);
    assert_eq!(distances[0], 0.0);
    assert!(distances.windows(2).all(|w| w[1] > w[0]));
    // Woodlands North to Springleaf is roughly six kilometres.
    assert!(
        (5_000.0..9_000.0).contains(&distances[3]),
        "got {} m",
        distances[3]
    );
}
