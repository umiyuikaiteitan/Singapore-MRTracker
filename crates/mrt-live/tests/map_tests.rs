//! Tests for the live map view model.
//!
//! The tests build the snapshot from the miniature fixture feed in
//! `mrt-gtfs/tests/fixtures/mini`, from a fixed service date, from a
//! fixed clock, and from synthetic trip updates. No test touches the
//! network.
//!
//! The fixture supplies every case the plan enumerates: `NS_T1` runs
//! between two stations, `EW_T1` stands at one, `BP_T1` is a non-exact
//! headway block, `TE_M1` has stop times that the feed leaves empty,
//! `NS_T5` continues past midnight, and `PW_L1` follows a loop that
//! visits Punggol twice.

use std::path::PathBuf;

use mrt_gtfs::{GtfsFeed, GtfsTime, MissingTimePolicy, RailNetwork, ServiceDate, StationId};
use mrt_gtfs_rt::{RailRtFeed, StopTimeEvent, StopTimeUpdate, TripUpdate};
use mrt_live::{
    FreshnessState, LineState, MapTrain, NetworkSnapshot, NetworkSnapshotBuilder, PositionQuality,
    TrainLocation,
};

/// The POSIX time that stands for "now" in the tests. The value never
/// reaches a position; it only measures the age of the realtime feed.
const NOW_UNIX: u64 = 1_746_400_000;

/// The miniature fixture network.
fn network() -> RailNetwork {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mrt-gtfs/tests/fixtures/mini");
    RailNetwork::from_feed(&GtfsFeed::from_dir(dir).unwrap()).unwrap()
}

/// A Monday, so the `WKDAY` service of the fixture runs.
fn date() -> ServiceDate {
    "20250505".parse().unwrap()
}

/// A trip update with a trip-level delay and optional per-stop events.
fn update(trip_id: &str, delay_secs: Option<i32>, stops: Vec<StopTimeUpdate>) -> TripUpdate {
    TripUpdate {
        trip_id: Some(trip_id.to_string()),
        delay_secs,
        stop_updates: stops,
        ..Default::default()
    }
}

/// A per-stop event that reports the same delay for both events.
fn stop_delay(stop_id: &str, delay_secs: i32) -> StopTimeUpdate {
    StopTimeUpdate {
        stop_id: Some(stop_id.to_string()),
        arrival: Some(StopTimeEvent {
            time: None,
            delay_secs: Some(delay_secs),
        }),
        departure: Some(StopTimeEvent {
            time: None,
            delay_secs: Some(delay_secs),
        }),
        ..Default::default()
    }
}

/// A feed with a timestamp and the given trip updates.
fn feed(timestamp: u64, updates: Vec<TripUpdate>) -> RailRtFeed {
    RailRtFeed {
        feed_timestamp: Some(timestamp),
        trip_updates: updates,
        ..Default::default()
    }
}

/// Find the one train of a run, or fail.
fn train<'a>(snapshot: &'a NetworkSnapshot, trip_id: &str) -> &'a MapTrain {
    let found: Vec<&MapTrain> = snapshot
        .trains
        .iter()
        .filter(|t| t.source_trip_id == trip_id)
        .collect();
    assert_eq!(found.len(), 1, "expected one train for {trip_id}");
    found[0]
}

/// Report whether any train belongs to the run.
fn has_train(snapshot: &NetworkSnapshot, trip_id: &str) -> bool {
    snapshot.trains.iter().any(|t| t.source_trip_id == trip_id)
}

/// Get the name of a station in the snapshot.
fn station_name(snapshot: &NetworkSnapshot, station: StationId) -> &str {
    snapshot
        .stations
        .iter()
        .find(|s| s.station == station)
        .map(|s| s.name.as_str())
        .expect("the snapshot names every station")
}

/// Report whether a diagnostic with the code is present.
fn has_diagnostic(snapshot: &NetworkSnapshot, code: &str) -> bool {
    snapshot.diagnostics.iter().any(|d| d.code == code)
}

/// Report whether a diagnostic names the subject with the code.
fn has_diagnostic_about(snapshot: &NetworkSnapshot, code: &str, subject: &str) -> bool {
    snapshot
        .diagnostics
        .iter()
        .any(|d| d.code == code && d.subject.as_deref() == Some(subject))
}

// ----------------------------------------------------------------------
// The static layers
// ----------------------------------------------------------------------

#[test]
fn the_snapshot_carries_the_whole_network() {
    let network = network();
    let snapshot = NetworkSnapshotBuilder::new(&network).build(date(), GtfsTime::from_hms(6, 5, 0));

    // The bus route of the fixture is not rail and never reaches the
    // map.
    assert!(snapshot.lines.iter().all(|l| l.route_id != "970"));
    assert!(snapshot.lines.iter().any(|l| l.name == "NSL"));
    assert_eq!(snapshot.lines.len(), network.lines().len());
    assert_eq!(snapshot.stations.len(), network.stations().len());

    // Every edge joins two neighbouring stations of one pattern.
    for edge in &snapshot.edges {
        let pattern = network.pattern(edge.pattern);
        assert_eq!(pattern.stations[edge.index], edge.from);
        assert_eq!(pattern.stations[edge.index + 1], edge.to);
        assert_eq!(pattern.line, edge.line);
    }

    // The order is stable: pattern first, then the index in it.
    let keys: Vec<(usize, usize)> = snapshot
        .edges
        .iter()
        .map(|e| (e.pattern.0, e.index))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted);
}

#[test]
fn the_alerts_set_the_state_of_a_line() {
    let network = network();
    let alerts = disrupted_alerts();
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_alerts(&alerts)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    let nsl = snapshot.lines.iter().find(|l| l.name == "NSL").unwrap();
    match &nsl.state {
        LineState::Disrupted { stations, .. } => {
            assert_eq!(stations, &vec!["NS1".to_string(), "NS4".to_string()]);
        }
        LineState::Normal => panic!("the NSL must be disrupted"),
    }
    let ewl = snapshot.lines.iter().find(|l| l.name == "EWL").unwrap();
    assert!(matches!(ewl.state, LineState::Normal));
}

/// The legacy alert payload of a North South Line disruption.
fn disrupted_alerts() -> mrt_datamall::TrainServiceAlerts {
    serde_json::from_str(
        r#"{
            "Status": 2,
            "AffectedSegments": [
                {
                    "Line": "NSL",
                    "Direction": "Both",
                    "Stations": "NS1,NS4",
                    "FreePublicBus": "NS1",
                    "FreeMRTShuttle": "",
                    "MRTShuttleDirection": ""
                }
            ],
            "Message": [{"Content": "NSL: no service.", "CreatedDate": ""}]
        }"#,
    )
    .unwrap()
}

// ----------------------------------------------------------------------
// Positions
// ----------------------------------------------------------------------

#[test]
fn a_run_between_two_stations_sits_on_an_edge() {
    let network = network();
    let snapshot = NetworkSnapshotBuilder::new(&network).build(date(), GtfsTime::from_hms(6, 5, 0));

    // NS_T1 leaves Jurong East at 06:00:30 and reaches Choa Chu Kang
    // at 06:10:00. At 06:05:00 it is 270 of 570 seconds along.
    let run = train(&snapshot, "NS_T1");
    match run.location {
        TrainLocation::OnEdge {
            index, from, to, ..
        } => {
            assert_eq!(index, 0);
            assert_eq!(station_name(&snapshot, from), "Jurong East");
            assert_eq!(station_name(&snapshot, to), "Choa Chu Kang");
        }
        TrainLocation::AtStation { .. } => panic!("NS_T1 is between two stations"),
    }
    assert!(
        (run.progress - 270.0 / 570.0).abs() < 1e-9,
        "{}",
        run.progress
    );
    assert_eq!(run.quality, PositionQuality::ScheduleOnly);
    assert_eq!(run.delay_secs, None);
    assert_eq!(run.destination, "Marina Bay");
    assert!(!run.schedule_interpolated);
}

#[test]
fn a_run_standing_at_a_station_sits_on_the_station() {
    let network = network();
    // EW_T1 arrives at Jurong East at 06:05:00 and leaves at 06:05:30.
    let realtime = feed(NOW_UNIX, vec![update("EW_T1", None, vec![])]);
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    let run = train(&snapshot, "EW_T1");
    match run.location {
        TrainLocation::AtStation { station, index, .. } => {
            assert_eq!(station_name(&snapshot, station), "Jurong East");
            assert_eq!(index, 0);
        }
        TrainLocation::OnEdge { .. } => panic!("EW_T1 stands at a station"),
    }
    assert_eq!(run.progress, 0.0);
    assert_eq!(run.quality, PositionQuality::AtStation);
}

#[test]
fn a_trip_update_shifts_the_position() {
    let network = network();
    let realtime = feed(NOW_UNIX, vec![update("NS_T1", Some(120), vec![])]);
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    // The whole run moves two minutes later: the departure behind is
    // 06:02:30 and the arrival ahead is 06:12:00.
    let run = train(&snapshot, "NS_T1");
    assert_eq!(run.quality, PositionQuality::InterpolatedRealtime);
    assert_eq!(run.delay_secs, Some(120));
    assert!(
        (run.progress - 150.0 / 570.0).abs() < 1e-9,
        "{}",
        run.progress
    );
}

#[test]
fn a_per_stop_event_wins_over_the_trip_delay() {
    let network = network();
    // The trip runs two minutes late everywhere, but the operator
    // predicts a five minute delay into Choa Chu Kang.
    let realtime = feed(
        NOW_UNIX,
        vec![update("NS_T1", Some(120), vec![stop_delay("CCK_NS", 300)])],
    );
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    // Departure behind 06:02:30, arrival ahead 06:15:00.
    let run = train(&snapshot, "NS_T1");
    assert!(
        (run.progress - 150.0 / 750.0).abs() < 1e-9,
        "{}",
        run.progress
    );
    assert_eq!(run.delay_secs, Some(120));
}

#[test]
fn a_predicted_time_without_a_delay_leaves_a_note() {
    let network = network();
    let realtime = feed(
        NOW_UNIX,
        vec![update(
            "NS_T1",
            None,
            vec![StopTimeUpdate {
                stop_id: Some("CCK_NS".to_string()),
                arrival: Some(StopTimeEvent {
                    time: Some(1_746_400_500),
                    delay_secs: None,
                }),
                ..Default::default()
            }],
        )],
    );
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    assert!(has_diagnostic_about(
        &snapshot,
        "stop-update-without-delay",
        "NS_T1"
    ));
    // The schedule still places the run; nothing was guessed from the
    // POSIX time.
    let run = train(&snapshot, "NS_T1");
    assert!((run.progress - 270.0 / 570.0).abs() < 1e-9);
    assert_eq!(run.delay_secs, None);
}

#[test]
fn a_canceled_run_is_not_drawn() {
    let network = network();
    let realtime = feed(
        NOW_UNIX,
        vec![TripUpdate {
            trip_id: Some("NS_T1".to_string()),
            canceled: true,
            ..Default::default()
        }],
    );
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    assert!(!has_train(&snapshot, "NS_T1"));
    assert!(has_diagnostic_about(
        &snapshot,
        "train-canceled",
        "20250505:NS_T1"
    ));
}

#[test]
fn a_cancellation_survives_a_stale_feed() {
    let network = network();
    let realtime = feed(
        NOW_UNIX - 3600,
        vec![TripUpdate {
            trip_id: Some("NS_T1".to_string()),
            canceled: true,
            ..Default::default()
        }],
    );
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    assert_eq!(snapshot.freshness.state, FreshnessState::Stale);
    assert!(!has_train(&snapshot, "NS_T1"));
}

#[test]
fn a_headway_band_never_becomes_a_train() {
    let network = network();
    // BP_T1 runs every ten minutes from 05:30 to 06:00 with
    // exact_times=0, so the individual runs do not exist.
    let snapshot =
        NetworkSnapshotBuilder::new(&network).build(date(), GtfsTime::from_hms(5, 15, 0));

    assert!(!has_train(&snapshot, "BP_T1"));
    let band = snapshot
        .bands
        .iter()
        .find(|b| b.source_trip_id == "BP_T1")
        .expect("the non-exact block reaches the snapshot as a band");
    assert_eq!(band.headway_secs, 600);
    assert_eq!(band.headway_minutes, 10);
    assert_eq!(band.start.to_string(), "05:30:00");
    assert_eq!(band.end.to_string(), "06:00:00");
    assert_eq!(band.destination.as_deref(), Some("South View"));

    // The exact headway block of the fixture stays a train, because
    // its runs do exist. The run that started at 05:10 is between
    // Woodlands and Woodlands South at 05:15.
    assert!(snapshot.bands.iter().all(|b| b.source_trip_id != "TE_F1"));
    let run = train(&snapshot, "TE_F1");
    assert_eq!(run.instance_id, "20250505:TE_F1@05:10:00");
    match run.location {
        TrainLocation::OnEdge { from, to, .. } => {
            assert_eq!(station_name(&snapshot, from), "Woodlands");
            assert_eq!(station_name(&snapshot, to), "Woodlands South");
        }
        TrainLocation::AtStation { .. } => panic!("the run is between two stations"),
    }
}

#[test]
fn a_run_bracketed_by_missing_times_is_not_placed() {
    let network = network();
    // TE_M1 has times at Woodlands North and at Springleaf only. With
    // the interpolation switched off, the two calls in between carry
    // no time, and the snapshot cannot say which edge the run is on.
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .missing_time_policy(MissingTimePolicy::None)
        .build(date(), GtfsTime::from_hms(7, 5, 0));

    assert!(!has_train(&snapshot, "TE_M1"));
    assert!(has_diagnostic_about(
        &snapshot,
        "train-between-missing-calls",
        "20250505:TE_M1"
    ));
}

#[test]
fn an_interpolated_schedule_marks_the_position() {
    let network = network();
    // With the default policy the same run is placed, and the marker
    // says that the schedule itself was computed.
    let realtime = feed(NOW_UNIX, vec![update("TE_M1", Some(0), vec![])]);
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(7, 5, 0));

    let run = train(&snapshot, "TE_M1");
    assert_eq!(run.quality, PositionQuality::InterpolatedSchedule);
    assert!(run.schedule_interpolated);
}

#[test]
fn a_stale_feed_falls_back_to_the_schedule() {
    let network = network();
    let realtime = feed(NOW_UNIX - 600, vec![update("NS_T1", Some(120), vec![])]);
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .staleness_secs(120)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    assert_eq!(snapshot.freshness.state, FreshnessState::Stale);
    assert_eq!(snapshot.freshness.age_secs, Some(600));
    assert!(has_diagnostic(&snapshot, "realtime-stale"));

    // The delay is not applied, and every train is schedule-only.
    let run = train(&snapshot, "NS_T1");
    assert!((run.progress - 270.0 / 570.0).abs() < 1e-9);
    assert_eq!(run.delay_secs, None);
    assert!(snapshot
        .trains
        .iter()
        .all(|t| t.quality == PositionQuality::ScheduleOnly));
}

#[test]
fn a_feed_without_a_timestamp_counts_as_stale() {
    let network = network();
    let realtime = RailRtFeed {
        feed_timestamp: None,
        trip_updates: vec![update("NS_T1", Some(120), vec![])],
        ..Default::default()
    };
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    assert_eq!(snapshot.freshness.state, FreshnessState::Stale);
    assert!(has_diagnostic(&snapshot, "realtime-without-timestamp"));
    assert_eq!(train(&snapshot, "NS_T1").delay_secs, None);
}

#[test]
fn without_a_realtime_layer_everything_is_schedule_only() {
    let network = network();
    let snapshot = NetworkSnapshotBuilder::new(&network).build(date(), GtfsTime::from_hms(6, 5, 0));

    assert_eq!(snapshot.freshness.state, FreshnessState::Unavailable);
    assert_eq!(snapshot.freshness.feed_timestamp, None);
    assert_eq!(snapshot.freshness.age_secs, None);
    assert!(!snapshot.trains.is_empty());
    assert!(snapshot
        .trains
        .iter()
        .all(|t| t.quality == PositionQuality::ScheduleOnly));
}

#[test]
fn an_empty_realtime_feed_says_so() {
    let network = network();
    let realtime = feed(NOW_UNIX, Vec::new());
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    assert_eq!(snapshot.freshness.state, FreshnessState::Live);
    assert!(has_diagnostic(&snapshot, "realtime-without-trip-updates"));
    assert!(snapshot
        .trains
        .iter()
        .all(|t| t.quality == PositionQuality::ScheduleOnly));
}

#[test]
fn a_run_past_midnight_stays_on_its_service_day() {
    let network = network();
    // NS_T5 leaves Jurong East at 23:50:30 and reaches Marina Bay at
    // 24:25:00, on the service day that began the morning before.
    let snapshot =
        NetworkSnapshotBuilder::new(&network).build(date(), GtfsTime::from_hms(24, 10, 0));

    let run = train(&snapshot, "NS_T5");
    match run.location {
        TrainLocation::OnEdge {
            index, from, to, ..
        } => {
            assert_eq!(index, 1);
            assert_eq!(station_name(&snapshot, from), "Choa Chu Kang");
            assert_eq!(station_name(&snapshot, to), "Marina Bay");
        }
        TrainLocation::AtStation { .. } => panic!("NS_T5 is between two stations"),
    }
    // 24:05:30 to 24:25:00, and the clock stands at 24:10:00.
    assert!(
        (run.progress - 270.0 / 1170.0).abs() < 1e-9,
        "{}",
        run.progress
    );
}

#[test]
fn a_loop_pattern_visits_a_station_twice() {
    let network = network();
    // PW_L1 leaves Punggol, calls at Sam Kee and Teck Lee, and returns
    // to Punggol. At 06:08:00 it is on the last edge, heading back.
    let snapshot = NetworkSnapshotBuilder::new(&network).build(date(), GtfsTime::from_hms(6, 8, 0));

    let run = train(&snapshot, "PW_L1");
    let (pattern, index, from, to) = match run.location {
        TrainLocation::OnEdge {
            pattern,
            index,
            from,
            to,
        } => (pattern, index, from, to),
        TrainLocation::AtStation { .. } => panic!("PW_L1 is between two stations"),
    };
    assert_eq!(index, 2);
    assert_eq!(station_name(&snapshot, from), "Teck Lee");
    assert_eq!(station_name(&snapshot, to), "Punggol");

    // The pattern really does visit Punggol twice, and the two edges
    // that touch it stay distinct.
    let stations = &network.pattern(pattern).stations;
    assert_eq!(stations.first(), stations.last());
    let touching: Vec<usize> = snapshot
        .edges
        .iter()
        .filter(|e| e.pattern == pattern && (e.from == to || e.to == to))
        .map(|e| e.index)
        .collect();
    assert_eq!(touching, vec![0, 2]);
}

// ----------------------------------------------------------------------
// Determinism
// ----------------------------------------------------------------------

#[test]
fn the_trains_come_out_in_a_stable_order() {
    let network = network();
    let snapshot = NetworkSnapshotBuilder::new(&network).build(date(), GtfsTime::from_hms(6, 5, 0));

    let ids: Vec<&str> = snapshot
        .trains
        .iter()
        .map(|t| t.instance_id.as_str())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
    assert!(ids.len() > 1);
}

#[test]
fn the_same_inputs_produce_the_same_json() {
    let network = network();
    let realtime = snapshot_realtime();
    let alerts = disrupted_alerts();
    let build = || {
        serde_json::to_string(
            &NetworkSnapshotBuilder::new(&network)
                .with_realtime(&realtime, NOW_UNIX)
                .with_alerts(&alerts)
                .build(date(), GtfsTime::from_hms(6, 5, 0)),
        )
        .unwrap()
    };
    assert_eq!(build(), build());
}

// ----------------------------------------------------------------------
// The committed snapshot
// ----------------------------------------------------------------------

/// The realtime layer of the acceptance snapshot.
///
/// It carries one run that is late with a per-stop prediction, one that
/// is early and standing at a station, and one that the operator
/// canceled.
fn snapshot_realtime() -> RailRtFeed {
    feed(
        NOW_UNIX - 30,
        vec![
            update("NS_T1", Some(60), vec![stop_delay("CCK_NS", 150)]),
            update("EW_T1", None, vec![stop_delay("JUR_EW", -20)]),
            TripUpdate {
                trip_id: Some("TE_T3".to_string()),
                canceled: true,
                ..Default::default()
            },
        ],
    )
}

fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name)
}

/// Compare `actual` with the stored snapshot, or write it.
///
/// To accept an intended change, run
///
/// ```sh
/// UPDATE_SNAPSHOTS=1 cargo test -p mrt-live --test map_tests
/// ```
///
/// and review the diff.
fn assert_snapshot(name: &str, actual: &str) {
    let path = snapshot_path(name);
    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "the snapshot {} is missing; run with UPDATE_SNAPSHOTS=1 to create it",
            path.display()
        )
    });
    if expected != actual {
        let line = expected
            .lines()
            .zip(actual.lines())
            .position(|(a, b)| a != b)
            .map(|index| index + 1)
            .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()) + 1);
        panic!(
            "the snapshot no longer matches {}\n  first difference at line {line}\n  \
             run with UPDATE_SNAPSHOTS=1 to accept the change",
            path.display()
        );
    }
}

#[test]
fn the_map_snapshot_is_stable() {
    let network = network();
    let realtime = snapshot_realtime();
    let alerts = disrupted_alerts();
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .with_alerts(&alerts)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    let mut json = serde_json::to_string_pretty(&snapshot).unwrap();
    json.push('\n');
    assert_snapshot("map-mini.json", &json);
}

#[test]
fn the_snapshot_file_is_committed() {
    // A snapshot that only exists on the machine that wrote it is no
    // snapshot at all.
    let path = snapshot_path("map-mini.json");
    assert!(path.metadata().is_ok_and(|m| m.len() > 0), "{path:?}");
}
