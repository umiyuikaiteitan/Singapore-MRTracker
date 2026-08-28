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

/// A trip update that names the run it is about: the service date in
/// the GTFS `YYYYMMDD` form, and the start time of the run for a trip
/// that a headway block expands.
fn run_update(
    trip_id: &str,
    start_date: Option<&str>,
    start_time: Option<&str>,
    delay_secs: Option<i32>,
) -> TripUpdate {
    TripUpdate {
        trip_id: Some(trip_id.to_string()),
        start_date: start_date.map(str::to_string),
        start_time: start_time.map(str::to_string),
        delay_secs,
        ..Default::default()
    }
}

/// A cancellation that names the run it is about.
fn cancellation(trip_id: &str, start_date: Option<&str>, start_time: Option<&str>) -> TripUpdate {
    TripUpdate {
        canceled: true,
        ..run_update(trip_id, start_date, start_time, None)
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

#[test]
fn the_alert_messages_reach_the_snapshot_as_network_notices() {
    let network = network();
    let alerts = disrupted_alerts();
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_alerts(&alerts)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    // The legacy payload attaches no message to a segment, so the
    // messages are the network's and the snapshot carries them once.
    assert_eq!(snapshot.notices, vec!["NSL: no service.".to_string()]);

    // Without alerts there is nothing to say.
    let quiet = NetworkSnapshotBuilder::new(&network).build(date(), GtfsTime::from_hms(6, 5, 0));
    assert!(quiet.notices.is_empty());
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
    // EW_T1 arrives at Jurong East at 06:05:00 and leaves at 06:05:30,
    // and the operator reports it running to time.
    let realtime = feed(NOW_UNIX, vec![update("EW_T1", Some(0), vec![])]);
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

// ----------------------------------------------------------------------
// Which update belongs to which run
// ----------------------------------------------------------------------

#[test]
fn an_update_applies_only_to_the_service_date_it_names() {
    let network = network();
    let build = |update: TripUpdate| {
        let realtime = feed(NOW_UNIX, vec![update]);
        NetworkSnapshotBuilder::new(&network)
            .with_realtime(&realtime, NOW_UNIX)
            .build(date(), GtfsTime::from_hms(6, 5, 0))
    };

    // The run is the Monday's NS_T1. An update that names the Monday
    // shifts it; an update that names the Sunday before is about
    // another day's run of the same trip and shifts nothing.
    let today = build(run_update("NS_T1", Some("20250505"), None, Some(120)));
    assert_eq!(train(&today, "NS_T1").delay_secs, Some(120));
    assert!((train(&today, "NS_T1").progress - 150.0 / 570.0).abs() < 1e-9);

    let yesterday = build(run_update("NS_T1", Some("20250504"), None, Some(120)));
    let run = train(&yesterday, "NS_T1");
    assert_eq!(run.delay_secs, None);
    assert_eq!(run.quality, PositionQuality::ScheduleOnly);
    assert!((run.progress - 270.0 / 570.0).abs() < 1e-9);

    // The documented fallback: an update that names no date applies to
    // whichever scanned day carries the trip.
    let undated = build(run_update("NS_T1", None, None, Some(120)));
    assert_eq!(train(&undated, "NS_T1").delay_secs, Some(120));
}

#[test]
fn an_update_names_its_day_across_midnight() {
    let network = network();
    // Ten past midnight on the Tuesday. The only run out is NS_T5,
    // which belongs to the Monday's service day and reads 24:10 there.
    // Both days are scanned, so the date of the update decides which
    // of the two the operator meant.
    let build = |update: TripUpdate| {
        let realtime = feed(NOW_UNIX, vec![update]);
        NetworkSnapshotBuilder::new(&network)
            .with_realtime(&realtime, NOW_UNIX)
            .build("20250506".parse().unwrap(), GtfsTime::from_hms(0, 10, 0))
    };

    // The Monday: the day the run started on, and the one it belongs
    // to.
    let monday = build(run_update("NS_T5", Some("20250505"), None, Some(120)));
    let run = train(&monday, "NS_T5");
    assert_eq!(run.instance_id, "20250505:NS_T5");
    assert_eq!(run.delay_secs, Some(120));
    assert!(
        (run.progress - 150.0 / 1170.0).abs() < 1e-9,
        "{}",
        run.progress
    );

    // The Tuesday: the calendar day the clock reads, and not the
    // service day of this run. Nothing shifts.
    let tuesday = build(run_update("NS_T5", Some("20250506"), None, Some(120)));
    let run = train(&tuesday, "NS_T5");
    assert_eq!(run.instance_id, "20250505:NS_T5");
    assert_eq!(run.delay_secs, None);
    assert_eq!(run.quality, PositionQuality::ScheduleOnly);
    assert!(
        (run.progress - 270.0 / 1170.0).abs() < 1e-9,
        "{}",
        run.progress
    );
}

#[test]
fn frequency_siblings_match_by_their_start_time() {
    let network = network();
    // TE_F1 is an exact headway block from 05:00 to 05:30 every ten
    // minutes, so three runs share the one trip_id. At 05:15 the run
    // that left Woodlands North at 05:10 is the one on the network.
    let build = |update: TripUpdate| {
        let realtime = feed(NOW_UNIX, vec![update]);
        NetworkSnapshotBuilder::new(&network)
            .with_realtime(&realtime, NOW_UNIX)
            .build(date(), GtfsTime::from_hms(5, 15, 0))
    };

    // A minute of delay on that run puts it in Woodlands at 05:15.
    let mine = build(run_update("TE_F1", None, Some("05:10:00"), Some(60)));
    let run = train(&mine, "TE_F1");
    assert_eq!(run.instance_id, "20250505:TE_F1@05:10:00");
    assert_eq!(run.delay_secs, Some(60));
    match run.location {
        TrainLocation::AtStation { station, .. } => {
            assert_eq!(station_name(&mine, station), "Woodlands");
        }
        TrainLocation::OnEdge { .. } => panic!("the delayed run stands at Woodlands at 05:15"),
    }

    // The same delay on the sibling that leaves at 05:20 says nothing
    // about the run on the network, which stays on the schedule.
    let sibling = build(run_update("TE_F1", None, Some("05:20:00"), Some(60)));
    let run = train(&sibling, "TE_F1");
    assert_eq!(run.instance_id, "20250505:TE_F1@05:10:00");
    assert_eq!(run.delay_secs, None);
    assert_eq!(run.quality, PositionQuality::ScheduleOnly);
    match run.location {
        TrainLocation::OnEdge { from, to, .. } => {
            assert_eq!(station_name(&sibling, from), "Woodlands");
            assert_eq!(station_name(&sibling, to), "Woodlands South");
        }
        TrainLocation::AtStation { .. } => panic!("the untouched run is between two stations"),
    }
    // Naming one sibling is not ambiguous: the feed said which run.
    assert!(!has_diagnostic(&sibling, "realtime-update-ambiguous"));
}

#[test]
fn a_frequency_update_without_a_start_time_moves_no_sibling() {
    let network = network();
    // The operator reports a minute of delay on TE_F1 and does not say
    // which of the three runs of the block it means. Applying it to
    // every sibling would state three delays where one was published,
    // so the map applies it to none and names the run it left alone.
    let realtime = feed(NOW_UNIX, vec![run_update("TE_F1", None, None, Some(60))]);
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(5, 15, 0));

    let run = train(&snapshot, "TE_F1");
    assert_eq!(run.instance_id, "20250505:TE_F1@05:10:00");
    assert_eq!(run.delay_secs, None);
    assert_eq!(run.quality, PositionQuality::ScheduleOnly);
    assert!(has_diagnostic_about(
        &snapshot,
        "realtime-update-ambiguous",
        "20250505:TE_F1@05:10:00"
    ));
}

#[test]
fn a_start_time_never_filters_a_fixed_trip() {
    let network = network();
    // NS_T1 runs once on the service date, so the trip_id and the date
    // already name the run. A start_time that disagrees with the
    // schedule does not detach the operator's prediction from it.
    let realtime = feed(
        NOW_UNIX,
        vec![run_update(
            "NS_T1",
            Some("20250505"),
            Some("23:59:00"),
            Some(120),
        )],
    );
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    assert_eq!(train(&snapshot, "NS_T1").delay_secs, Some(120));
    assert!(!has_diagnostic(&snapshot, "realtime-update-ambiguous"));
}

#[test]
fn an_unreadable_start_date_is_reported_and_read_as_none() {
    let network = network();
    let realtime = feed(
        NOW_UNIX,
        vec![run_update("NS_T1", Some("05/05/2025"), None, Some(120))],
    );
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    assert!(has_diagnostic_about(
        &snapshot,
        "realtime-unreadable-start-date",
        "NS_T1"
    ));
    // It falls back to the behaviour of an update that names no date,
    // rather than detaching itself from every run in silence.
    assert_eq!(train(&snapshot, "NS_T1").delay_secs, Some(120));
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
fn an_empty_trip_update_leaves_the_run_schedule_only() {
    let network = network();
    // An update that names the run and says nothing else: no delay, no
    // stop event. It shifts no time, so it supports no claim beyond the
    // schedule, and the provenance says exactly that.
    let realtime = feed(NOW_UNIX, vec![update("EW_T1", None, vec![])]);
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    let run = train(&snapshot, "EW_T1");
    assert!(matches!(run.location, TrainLocation::AtStation { .. }));
    assert_eq!(run.quality, PositionQuality::ScheduleOnly);
    assert_eq!(run.delay_secs, None);

    // An update that names another stop of the same run says nothing
    // about the call the position stands on either.
    let realtime = feed(
        NOW_UNIX,
        vec![update("EW_T1", None, vec![stop_delay("RFP_EW", 60)])],
    );
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));
    assert_eq!(
        train(&snapshot, "EW_T1").quality,
        PositionQuality::ScheduleOnly
    );
}

#[test]
fn a_skipped_call_is_passed_and_never_stood_at() {
    let network = network();
    // The operator says NS_T1 does not serve Choa Chu Kang, where it
    // was scheduled to stand from 06:10:00 to 06:10:30. A run that
    // skips a station never dwells there: through the whole scheduled
    // dwell it is already on the edge beyond it.
    let realtime = feed(
        NOW_UNIX,
        vec![update(
            "NS_T1",
            None,
            vec![StopTimeUpdate {
                stop_id: Some("CCK_NS".to_string()),
                skipped: true,
                ..Default::default()
            }],
        )],
    );
    let build = |clock: GtfsTime| {
        NetworkSnapshotBuilder::new(&network)
            .with_realtime(&realtime, NOW_UNIX)
            .build(date(), clock)
    };

    for clock in [GtfsTime::from_hms(6, 10, 0), GtfsTime::from_hms(6, 10, 15)] {
        let snapshot = build(clock);
        let run = train(&snapshot, "NS_T1");
        match run.location {
            TrainLocation::OnEdge {
                index, from, to, ..
            } => {
                assert_eq!(index, 1);
                assert_eq!(station_name(&snapshot, from), "Choa Chu Kang");
                assert_eq!(station_name(&snapshot, to), "Marina Bay");
            }
            TrainLocation::AtStation { .. } => {
                panic!("the run skips Choa Chu Kang, so it never stands there")
            }
        }
        // The edge beyond the skipped call starts at the arrival, not
        // at the departure the run no longer makes.
        assert_eq!(run.edge_secs, Some(1200));
    }
    // 06:10:15 is 15 s into that edge.
    let progress = train(&build(GtfsTime::from_hms(6, 10, 15)), "NS_T1").progress;
    assert!((progress - 15.0 / 1200.0).abs() < 1e-9, "{progress}");

    // The skip is reported, not swallowed.
    assert!(has_diagnostic_about(
        &build(GtfsTime::from_hms(6, 10, 15)),
        "train-call-skipped",
        "20250505:NS_T1"
    ));

    // Before the skipped call the run still rides the edge that leads
    // to it: the station is out of the trajectory, not out of the line.
    let snapshot = build(GtfsTime::from_hms(6, 5, 0));
    match train(&snapshot, "NS_T1").location {
        TrainLocation::OnEdge { index, to, .. } => {
            assert_eq!(index, 0);
            assert_eq!(station_name(&snapshot, to), "Choa Chu Kang");
        }
        TrainLocation::AtStation { .. } => panic!("NS_T1 is between two stations at 06:05"),
    }
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
    // A cancellation that names the service date of the run is about
    // that run and no other, so it outlives the staleness threshold:
    // the operator's statement that a train does not run does not
    // expire the way a prediction does.
    let build = |update: TripUpdate| {
        let realtime = feed(NOW_UNIX - 3600, vec![update]);
        NetworkSnapshotBuilder::new(&network)
            .with_realtime(&realtime, NOW_UNIX)
            .build(date(), GtfsTime::from_hms(6, 5, 0))
    };

    let dated = build(cancellation("NS_T1", Some("20250505"), None));
    assert_eq!(dated.freshness.state, FreshnessState::Stale);
    assert!(!has_train(&dated, "NS_T1"));
    assert!(has_diagnostic_about(
        &dated,
        "train-canceled",
        "20250505:NS_T1"
    ));

    // A cancellation that names no date is a statement about a
    // trip_id, and a stale feed cannot say which day it was made on.
    // Suppressing the run would let an old cancellation delete a train
    // that is running, so the map draws the scheduled run and says why.
    let undated = build(cancellation("NS_T1", None, None));
    assert_eq!(undated.freshness.state, FreshnessState::Stale);
    assert!(has_train(&undated, "NS_T1"));
    assert_eq!(
        train(&undated, "NS_T1").quality,
        PositionQuality::ScheduleOnly
    );
    assert!(has_diagnostic_about(
        &undated,
        "train-cancellation-not-attributed",
        "20250505:NS_T1"
    ));
}

#[test]
fn yesterdays_cancellation_does_not_suppress_todays_run() {
    let network = network();
    // The Sunday's cancellation of NS_T1, still in a fresh feed on the
    // Monday. The trip_id recurs; the run does not.
    let realtime = feed(
        NOW_UNIX,
        vec![cancellation("NS_T1", Some("20250504"), None)],
    );
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    assert_eq!(snapshot.freshness.state, FreshnessState::Live);
    assert!(has_train(&snapshot, "NS_T1"));
    assert!(!has_diagnostic(&snapshot, "train-canceled"));

    // Across midnight the same rule keeps the two days apart in the
    // other direction: the run out at 00:10 on the Tuesday belongs to
    // the Monday, and only the Monday's cancellation reaches it.
    let build = |start_date: &str| {
        let realtime = feed(
            NOW_UNIX,
            vec![cancellation("NS_T5", Some(start_date), None)],
        );
        NetworkSnapshotBuilder::new(&network)
            .with_realtime(&realtime, NOW_UNIX)
            .build("20250506".parse().unwrap(), GtfsTime::from_hms(0, 10, 0))
    };
    assert!(!has_train(&build("20250505"), "NS_T5"));
    assert!(has_train(&build("20250506"), "NS_T5"));
}

#[test]
fn a_frequency_cancellation_names_the_run_it_cancels() {
    let network = network();
    let build = |update: TripUpdate| {
        let realtime = feed(NOW_UNIX, vec![update]);
        NetworkSnapshotBuilder::new(&network)
            .with_realtime(&realtime, NOW_UNIX)
            .build(date(), GtfsTime::from_hms(5, 15, 0))
    };

    // The run on the network is the one that left at 05:10.
    assert!(!has_train(
        &build(cancellation("TE_F1", Some("20250505"), Some("05:10:00"))),
        "TE_F1"
    ));
    // Cancelling a sibling leaves it running.
    assert!(has_train(
        &build(cancellation("TE_F1", Some("20250505"), Some("05:20:00"))),
        "TE_F1"
    ));
    // A cancellation of the block that names no run cancels none of
    // them: it cannot be shown to be about this train.
    let ambiguous = build(cancellation("TE_F1", Some("20250505"), None));
    assert!(has_train(&ambiguous, "TE_F1"));
    assert!(has_diagnostic_about(
        &ambiguous,
        "realtime-update-ambiguous",
        "20250505:TE_F1@05:10:00"
    ));
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
fn an_ageing_feed_still_carries_its_predictions() {
    let network = network();
    // Ninety seconds is past the default ageing threshold of 60 s and
    // short of the default staleness threshold of 120 s.
    let realtime = feed(NOW_UNIX - 90, vec![update("NS_T1", Some(120), vec![])]);
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));

    assert_eq!(snapshot.freshness.state, FreshnessState::Ageing);
    assert!(snapshot.freshness.state.is_current());
    assert_eq!(snapshot.freshness.age_secs, Some(90));
    assert_eq!(snapshot.freshness.ageing_secs, 60);
    assert_eq!(snapshot.freshness.staleness_secs, 120);
    assert!(has_diagnostic(&snapshot, "realtime-ageing"));

    // Ageing is not stale: the operator's prediction still shifts the
    // position, and the provenance says so.
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
fn the_ageing_threshold_is_the_callers_to_set() {
    let network = network();
    let realtime = feed(NOW_UNIX - 30, vec![update("NS_T1", Some(120), vec![])]);
    let build = |ageing: u32| {
        NetworkSnapshotBuilder::new(&network)
            .with_realtime(&realtime, NOW_UNIX)
            .ageing_secs(ageing)
            .build(date(), GtfsTime::from_hms(6, 5, 0))
            .freshness
            .state
    };
    // A feed 30 s old is live under the default and ageing under a
    // threshold a caller measured to be tighter.
    assert_eq!(build(60), FreshnessState::Live);
    assert_eq!(build(20), FreshnessState::Ageing);

    // The staleness test runs first, so an ageing threshold at or
    // above it simply never fires.
    let old = feed(NOW_UNIX - 600, vec![update("NS_T1", Some(120), vec![])]);
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&old, NOW_UNIX)
        .staleness_secs(120)
        .ageing_secs(900)
        .build(date(), GtfsTime::from_hms(6, 5, 0));
    assert_eq!(snapshot.freshness.state, FreshnessState::Stale);
    assert!(!snapshot.freshness.state.is_current());
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
fn a_run_past_midnight_reaches_the_map_after_midnight() {
    let network = network();
    // Ten past midnight on the Tuesday. Nothing runs on the service
    // day that has just begun, and yet trains are out: NS_T5 left
    // Jurong East at 23:50:30 on the Monday and is still running, as
    // 24:xx on the Monday's own service day. The builder scans that day
    // too, exactly as `RailNetwork::departure_board` does.
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .build("20250506".parse().unwrap(), GtfsTime::from_hms(0, 10, 0));

    assert_eq!(snapshot.freshness.service_date.to_string(), "20250506");
    let run = train(&snapshot, "NS_T5");
    assert_eq!(run.instance_id, "20250505:NS_T5");
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
    // 24:05:30 to 24:25:00 on the Monday, and the clock stands at
    // 24:10:00 there.
    assert!(
        (run.progress - 270.0 / 1170.0).abs() < 1e-9,
        "{}",
        run.progress
    );

    // It is the only run out at that hour, and it appears once.
    assert_eq!(snapshot.trains.len(), 1);
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
