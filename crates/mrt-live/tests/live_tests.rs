//! Tests for the live composition layer.
//!
//! The tests build a small in-memory GTFS feed, then merge synthetic
//! live layers into it. No test touches the network.

use mrt_datamall::{CrowdLevel, PlatformCrowd, ServiceStatus, TrainLine, TrainServiceAlerts};
use mrt_gtfs::{Calendar, Frequency, GtfsFeed, RailNetwork, Route, Stop, StopTime, Trip};
use mrt_gtfs_rt::{RailRtFeed, StopTimeEvent, StopTimeUpdate, TripUpdate};
use mrt_live::{match_train_line, LineState, LiveBoardBuilder, NetworkStatus};

fn stop(id: &str, code: &str, name: &str) -> Stop {
    Stop {
        stop_id: id.to_string(),
        stop_code: Some(code.to_string()),
        stop_name: Some(name.to_string()),
        location_type: Some(0),
        ..Default::default()
    }
}

fn stop_time(trip: &str, time: &str, stop: &str, seq: u32) -> StopTime {
    StopTime {
        trip_id: trip.to_string(),
        arrival_time: Some(time.parse().unwrap()),
        departure_time: Some(time.parse().unwrap()),
        stop_id: stop.to_string(),
        stop_sequence: seq,
        ..Default::default()
    }
}

/// A one-line feed: three North South Line stations, daily service.
fn tiny_feed() -> GtfsFeed {
    GtfsFeed {
        stops: vec![
            stop("S_NS1", "NS1", "Jurong East"),
            stop("S_NS4", "NS4", "Choa Chu Kang"),
            stop("S_NS27", "NS27", "Marina Bay"),
        ],
        routes: vec![Route {
            route_id: "NS".to_string(),
            agency_id: None,
            route_short_name: Some("NSL".to_string()),
            route_long_name: Some("North South Line".to_string()),
            route_type: 1,
            route_color: Some("D42E12".to_string()),
            route_text_color: None,
        }],
        trips: vec![Trip {
            route_id: "NS".to_string(),
            service_id: "DAILY".to_string(),
            trip_id: "T1".to_string(),
            trip_headsign: Some("Marina Bay".to_string()),
            direction_id: Some(0),
            ..Default::default()
        }],
        stop_times: vec![
            stop_time("T1", "08:00:00", "S_NS1", 1),
            stop_time("T1", "08:10:00", "S_NS4", 2),
            stop_time("T1", "08:30:00", "S_NS27", 3),
        ],
        calendar: vec![Calendar {
            service_id: "DAILY".to_string(),
            monday: 1,
            tuesday: 1,
            wednesday: 1,
            thursday: 1,
            friday: 1,
            saturday: 1,
            sunday: 1,
            start_date: "20250101".parse().unwrap(),
            end_date: "20271231".parse().unwrap(),
        }],
        ..Default::default()
    }
}

fn tiny_network() -> RailNetwork {
    RailNetwork::from_feed(&tiny_feed()).unwrap()
}

fn disrupted_alerts() -> TrainServiceAlerts {
    serde_json::from_str(
        r#"{
            "Status": 2,
            "AffectedSegments": [
                {
                    "Line": "NSL",
                    "Direction": "Both",
                    "Stations": "NS1,NS4",
                    "FreePublicBus": "NS1,NS4",
                    "FreeMRTShuttle": "",
                    "MRTShuttleDirection": ""
                }
            ],
            "Message": [
                {
                    "Content": "NSL: no service between NS1 and NS4.",
                    "CreatedDate": "2026-08-10 08:00:00"
                }
            ]
        }"#,
    )
    .unwrap()
}

// ----------------------------------------------------------------------
// Line matching
// ----------------------------------------------------------------------

#[test]
fn line_matching_uses_codes_and_names() {
    let network = tiny_network();
    let line = network.line(network.line_by_route_id("NS").unwrap());
    assert_eq!(match_train_line(line), Some(TrainLine::NSL));

    let by_name = mrt_gtfs::Line {
        route_id: "X".to_string(),
        name: "Sengkang LRT".to_string(),
        long_name: None,
        route_type: 12,
        color: None,
        text_color: None,
    };
    assert_eq!(match_train_line(&by_name), Some(TrainLine::SLRT));

    let hyphenated = mrt_gtfs::Line {
        route_id: "NS".to_string(),
        name: "NS".to_string(),
        long_name: Some("North-South Line".to_string()),
        route_type: 1,
        color: None,
        text_color: None,
    };
    assert_eq!(match_train_line(&hyphenated), Some(TrainLine::NSL));

    let bus = mrt_gtfs::Line {
        route_id: "970".to_string(),
        name: "970".to_string(),
        long_name: Some("Bus Service 970".to_string()),
        route_type: 3,
        color: None,
        text_color: None,
    };
    assert_eq!(match_train_line(&bus), None);
}

// ----------------------------------------------------------------------
// Network status
// ----------------------------------------------------------------------

#[test]
fn network_status_marks_disrupted_lines() {
    let status = NetworkStatus::from_alerts(&disrupted_alerts());

    assert_eq!(status.overall, ServiceStatus::Disrupted);
    assert_eq!(status.lines.len(), TrainLine::ALL.len());
    assert!(status.line(TrainLine::NSL).is_disrupted());
    assert!(!status.line(TrainLine::EWL).is_disrupted());
    assert_eq!(status.messages.len(), 1);

    match &status.line(TrainLine::NSL).state {
        LineState::Disrupted {
            stations,
            direction,
            free_public_bus,
        } => {
            assert_eq!(stations, &vec!["NS1".to_string(), "NS4".to_string()]);
            assert_eq!(direction, "Both");
            assert_eq!(free_public_bus.len(), 2);
        }
        LineState::Normal => panic!("NSL must be disrupted"),
    }
}

#[test]
fn network_status_with_normal_service() {
    let alerts: TrainServiceAlerts =
        serde_json::from_str(r#"{"Status": 1, "AffectedSegments": [], "Message": []}"#).unwrap();
    let status = NetworkStatus::from_alerts(&alerts);
    assert_eq!(status.overall, ServiceStatus::Normal);
    assert!(status.lines.iter().all(|l| !l.is_disrupted()));
    assert!(status.messages.is_empty());
}

// ----------------------------------------------------------------------
// Live board
// ----------------------------------------------------------------------

#[test]
fn the_static_board_lists_departures() {
    let network = tiny_network();
    let station = network.station_by_code("NS4").unwrap();
    let board = LiveBoardBuilder::new(&network).build(
        station,
        "20260810".parse().unwrap(),
        "08:00:00".parse().unwrap(),
        3600,
    );

    assert_eq!(board.station_name, "Choa Chu Kang");
    assert_eq!(board.station_codes, vec!["NS4"]);
    assert_eq!(board.rows.len(), 1);

    let row = &board.rows[0];
    assert_eq!(row.line_code, "NSL");
    assert_eq!(row.line_color.as_deref(), Some("D42E12"));
    assert_eq!(row.destination, "Marina Bay");
    assert_eq!(row.departs_in_secs, 600);
    assert_eq!(row.clock_time, "08:10:00");
    assert!(!row.approximate);
    assert_eq!(row.delay_secs, None);
    assert!(!row.canceled);
    assert_eq!(row.crowd, None);
    assert!(board.notices.is_empty());
}

#[test]
fn live_layers_decorate_the_board() {
    let network = tiny_network();
    let station = network.station_by_code("NS4").unwrap();

    let alerts = disrupted_alerts();
    let crowd = vec![PlatformCrowd {
        station: "NS4".to_string(),
        start_time: "2026-08-10T08:00:00+08:00".to_string(),
        end_time: "2026-08-10T08:10:00+08:00".to_string(),
        crowd_level: CrowdLevel::High,
    }];
    let realtime = RailRtFeed {
        trip_updates: vec![TripUpdate {
            trip_id: Some("T1".to_string()),
            stop_updates: vec![StopTimeUpdate {
                stop_id: Some("S_NS4".to_string()),
                departure: Some(StopTimeEvent {
                    delay_secs: Some(120),
                    time: None,
                }),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    let board = LiveBoardBuilder::new(&network)
        .with_alerts(&alerts)
        .with_crowd(&crowd)
        .with_realtime(&realtime)
        .build(
            station,
            "20260810".parse().unwrap(),
            "08:00:00".parse().unwrap(),
            3600,
        );

    let row = &board.rows[0];
    assert_eq!(row.delay_secs, Some(120));
    assert_eq!(row.crowd, Some(CrowdLevel::High));
    assert!(!row.canceled);
    assert_eq!(board.notices, vec!["NSL: no service between NS1 and NS4."]);
}

#[test]
fn canceled_trips_are_flagged() {
    let network = tiny_network();
    let station = network.station_by_code("NS4").unwrap();
    let realtime = RailRtFeed {
        trip_updates: vec![TripUpdate {
            trip_id: Some("T1".to_string()),
            canceled: true,
            ..Default::default()
        }],
        ..Default::default()
    };

    let board = LiveBoardBuilder::new(&network)
        .with_realtime(&realtime)
        .build(
            station,
            "20260810".parse().unwrap(),
            "08:00:00".parse().unwrap(),
            3600,
        );
    assert!(board.rows[0].canceled);
}

#[test]
fn notices_stay_away_from_unaffected_stations() {
    let network = tiny_network();
    // Marina Bay (NS27) serves the disrupted line, so the notice
    // appears there too. But an alert for another line must not.
    let alerts: TrainServiceAlerts = serde_json::from_str(
        r#"{
            "Status": 2,
            "AffectedSegments": [
                {
                    "Line": "NEL",
                    "Direction": "Both",
                    "Stations": "NE1,NE3",
                    "FreePublicBus": "",
                    "FreeMRTShuttle": "",
                    "MRTShuttleDirection": ""
                }
            ],
            "Message": [{"Content": "NEL disruption.", "CreatedDate": ""}]
        }"#,
    )
    .unwrap();

    let station = network.station_by_code("NS4").unwrap();
    let board = LiveBoardBuilder::new(&network).with_alerts(&alerts).build(
        station,
        "20260810".parse().unwrap(),
        "08:00:00".parse().unwrap(),
        3600,
    );
    assert!(board.notices.is_empty());
}

#[test]
fn max_rows_caps_the_board() {
    let network = tiny_network();
    let station = network.station_by_code("NS1").unwrap();
    let board = LiveBoardBuilder::new(&network).max_rows(0).build(
        station,
        "20260810".parse().unwrap(),
        "08:00:00".parse().unwrap(),
        3600,
    );
    assert!(board.rows.is_empty());
}

// ----------------------------------------------------------------------
// GTFS-Realtime service alerts on the board
// ----------------------------------------------------------------------

use mrt_gtfs_rt::{ActivePeriod, Alert, AlertCause, AlertEffect, InformedEntity};

fn rt_alert(effect: AlertEffect, informed: Vec<InformedEntity>) -> Alert {
    Alert {
        cause: AlertCause::Unknown,
        effect,
        header: Some("Test alert".to_string()),
        description: None,
        url: None,
        active_periods: Vec::new(),
        informed,
    }
}

fn trip_entity(trip_id: &str) -> InformedEntity {
    InformedEntity {
        agency_id: None,
        route_id: None,
        stop_id: None,
        trip_id: Some(trip_id.to_string()),
    }
}

fn route_entity(route_id: &str) -> InformedEntity {
    InformedEntity {
        agency_id: None,
        route_id: Some(route_id.to_string()),
        stop_id: None,
        trip_id: None,
    }
}

fn stop_entity(stop_id: &str) -> InformedEntity {
    InformedEntity {
        agency_id: None,
        route_id: None,
        stop_id: Some(stop_id.to_string()),
        trip_id: None,
    }
}

fn build_with_alerts(alerts: &[Alert], now_unix: u64) -> mrt_live::LiveBoard {
    let network = tiny_network();
    let station = network.station_by_code("NS4").unwrap();
    LiveBoardBuilder::new(&network)
        .with_rt_alerts(alerts, now_unix)
        .build(
            station,
            "20260810".parse().unwrap(),
            "08:00:00".parse().unwrap(),
            3600,
        )
}

#[test]
fn a_no_service_alert_cancels_the_named_trip() {
    let alerts = vec![rt_alert(AlertEffect::NoService, vec![trip_entity("T1")])];
    let board = build_with_alerts(&alerts, 1_000);
    assert!(board.rows[0].canceled);
    assert!(!board.rows[0].alerted);
}

#[test]
fn a_route_alert_reaches_every_departure_of_the_line() {
    let alerts = vec![rt_alert(
        AlertEffect::SignificantDelays,
        vec![route_entity("NS")],
    )];
    let board = build_with_alerts(&alerts, 1_000);
    assert!(!board.rows[0].canceled);
    assert!(board.rows[0].alerted);
    // The alert text joins the notices.
    assert_eq!(board.notices, vec!["Test alert"]);
}

#[test]
fn a_platform_alert_reaches_the_station() {
    let alerts = vec![rt_alert(AlertEffect::NoService, vec![stop_entity("S_NS4")])];
    let board = build_with_alerts(&alerts, 1_000);
    assert!(board.rows[0].canceled);
    assert_eq!(board.notices, vec!["Test alert"]);
}

#[test]
fn alerts_for_other_parts_of_the_network_stay_away() {
    let alerts = vec![
        rt_alert(AlertEffect::NoService, vec![route_entity("EW")]),
        rt_alert(AlertEffect::NoService, vec![stop_entity("S_EW1")]),
        rt_alert(AlertEffect::NoService, vec![trip_entity("T9")]),
    ];
    let board = build_with_alerts(&alerts, 1_000);
    assert!(!board.rows[0].canceled);
    assert!(!board.rows[0].alerted);
    assert!(board.notices.is_empty());
}

#[test]
fn inactive_alerts_change_nothing() {
    let mut alert = rt_alert(AlertEffect::NoService, vec![route_entity("NS")]);
    alert.active_periods = vec![ActivePeriod {
        start: Some(100),
        end: Some(200),
    }];
    let board = build_with_alerts(&[alert], 1_000);
    assert!(!board.rows[0].canceled);
    assert!(board.notices.is_empty());
}

#[test]
fn a_non_disruptive_alert_leaves_the_timing_alone() {
    let alerts = vec![rt_alert(
        AlertEffect::AccessibilityIssue,
        vec![route_entity("NS")],
    )];
    let board = build_with_alerts(&alerts, 1_000);
    assert!(!board.rows[0].canceled);
    assert!(!board.rows[0].alerted);
}

#[test]
fn a_modified_schedule_is_a_notice_without_marking_rows() {
    // The LTA feed publishes months-long planned adjustments as
    // ModifiedService on a whole route. The board names them, but
    // must not mark every departure of the line.
    let alerts = vec![rt_alert(
        AlertEffect::ModifiedService,
        vec![route_entity("NS")],
    )];
    let board = build_with_alerts(&alerts, 1_000);
    assert!(!board.rows[0].alerted);
    assert!(!board.rows[0].canceled);
    assert_eq!(board.notices, vec!["Test alert"]);
}

// ----------------------------------------------------------------------
// Realtime predictions on the board
// ----------------------------------------------------------------------

/// The tiny network with a second run four minutes behind the first:
/// T1 calls NS4 at 08:10:00, T2 at 08:14:00.
fn two_train_network() -> RailNetwork {
    let mut feed = tiny_feed();
    feed.trips.push(Trip {
        route_id: "NS".to_string(),
        service_id: "DAILY".to_string(),
        trip_id: "T2".to_string(),
        trip_headsign: Some("Marina Bay".to_string()),
        direction_id: Some(0),
        ..Default::default()
    });
    feed.stop_times
        .push(stop_time("T2", "08:04:00", "S_NS1", 1));
    feed.stop_times
        .push(stop_time("T2", "08:14:00", "S_NS4", 2));
    feed.stop_times
        .push(stop_time("T2", "08:34:00", "S_NS27", 3));
    RailNetwork::from_feed(&feed).unwrap()
}

/// A realtime feed with one trip-level delay.
fn trip_delay(trip_id: &str, delay_secs: i32) -> RailRtFeed {
    RailRtFeed {
        trip_updates: vec![TripUpdate {
            trip_id: Some(trip_id.to_string()),
            delay_secs: Some(delay_secs),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn board_at(
    network: &RailNetwork,
    realtime: &RailRtFeed,
    code: &str,
    clock: &str,
    lookahead_secs: u32,
) -> mrt_live::LiveBoard {
    let station = network.station_by_code(code).unwrap();
    LiveBoardBuilder::new(network)
        .with_realtime(realtime)
        .build(
            station,
            "20260810".parse().unwrap(),
            clock.parse().unwrap(),
            lookahead_secs,
        )
}

#[test]
fn a_delay_moves_and_reorders_the_rows() {
    let network = two_train_network();
    // T1 slips from 08:10 to 08:16, behind the on-time 08:14 run.
    let realtime = trip_delay("T1", 360);
    let board = board_at(&network, &realtime, "NS4", "08:00:00", 3600);

    assert_eq!(board.rows.len(), 2);
    assert_eq!(board.rows[0].clock_time, "08:14:00");
    assert_eq!(board.rows[0].departs_in_secs, 840);
    assert_eq!(board.rows[0].delay_secs, None);
    assert_eq!(board.rows[1].clock_time, "08:16:00");
    assert_eq!(board.rows[1].departs_in_secs, 960);
    assert_eq!(board.rows[1].delay_secs, Some(360));
}

#[test]
fn the_row_limit_counts_predicted_times() {
    let network = two_train_network();
    let realtime = trip_delay("T1", 360);
    let station = network.station_by_code("NS4").unwrap();
    let board = LiveBoardBuilder::new(&network)
        .with_realtime(&realtime)
        .max_rows(1)
        .build(
            station,
            "20260810".parse().unwrap(),
            "08:00:00".parse().unwrap(),
            3600,
        );

    // The one surviving row is the train predicted first, not the
    // one scheduled first.
    assert_eq!(board.rows.len(), 1);
    assert_eq!(board.rows[0].clock_time, "08:14:00");
}

#[test]
fn a_delay_pushes_a_row_out_of_the_window() {
    let network = two_train_network();
    // T1 slips from 08:10 to 09:01, past the end of the window.
    let realtime = trip_delay("T1", 3060);
    let board = board_at(&network, &realtime, "NS4", "08:00:00", 3600);

    assert_eq!(board.rows.len(), 1);
    assert_eq!(board.rows[0].clock_time, "08:14:00");
}

#[test]
fn a_late_train_scheduled_before_now_still_shows() {
    let network = two_train_network();
    // At 08:12 the 08:10 run has not left: it is five minutes late.
    let realtime = trip_delay("T1", 300);
    let board = board_at(&network, &realtime, "NS4", "08:12:00", 3600);

    assert_eq!(board.rows.len(), 2);
    assert_eq!(board.rows[0].clock_time, "08:14:00");
    assert_eq!(board.rows[0].departs_in_secs, 120);
    assert_eq!(board.rows[1].clock_time, "08:15:00");
    assert_eq!(board.rows[1].departs_in_secs, 180);
}

#[test]
fn an_early_train_enters_the_window() {
    let network = two_train_network();
    // The window to 08:05 holds no scheduled call; T2 runs ten
    // minutes early and comes in at 08:04.
    let realtime = trip_delay("T2", -600);
    let board = board_at(&network, &realtime, "NS4", "08:00:00", 300);

    assert_eq!(board.rows.len(), 1);
    assert_eq!(board.rows[0].clock_time, "08:04:00");
    assert_eq!(board.rows[0].departs_in_secs, 240);
    assert_eq!(board.rows[0].delay_secs, Some(-600));
}

#[test]
fn a_stop_event_beats_the_trip_level_delay() {
    let network = tiny_network();
    let realtime = RailRtFeed {
        trip_updates: vec![TripUpdate {
            trip_id: Some("T1".to_string()),
            delay_secs: Some(600),
            stop_updates: vec![StopTimeUpdate {
                stop_id: Some("S_NS4".to_string()),
                departure: Some(StopTimeEvent {
                    delay_secs: Some(120),
                    time: None,
                }),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    // The named call follows its own event.
    let board = board_at(&network, &realtime, "NS4", "08:00:00", 3600);
    assert_eq!(board.rows[0].delay_secs, Some(120));
    assert_eq!(board.rows[0].departs_in_secs, 720);
    assert_eq!(board.rows[0].clock_time, "08:12:00");

    // A call the update does not name follows the trip-level delay.
    let origin = board_at(&network, &realtime, "NS1", "07:50:00", 3600);
    assert_eq!(origin.rows[0].delay_secs, Some(600));
    assert_eq!(origin.rows[0].departs_in_secs, 1200);
    assert_eq!(origin.rows[0].clock_time, "08:10:00");
}

#[test]
fn a_skipped_stop_leaves_the_board() {
    let network = two_train_network();
    let realtime = RailRtFeed {
        trip_updates: vec![TripUpdate {
            trip_id: Some("T1".to_string()),
            stop_updates: vec![StopTimeUpdate {
                stop_id: Some("S_NS4".to_string()),
                skipped: true,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    // Only T2 calls here now; the skipped row is gone, not merely
    // undecorated.
    let board = board_at(&network, &realtime, "NS4", "08:00:00", 3600);
    assert_eq!(board.rows.len(), 1);
    assert_eq!(board.rows[0].clock_time, "08:14:00");

    // At another station the run still calls as scheduled.
    let origin = board_at(&network, &realtime, "NS1", "07:50:00", 3600);
    assert_eq!(origin.rows.len(), 2);
}

#[test]
fn a_canceled_row_keeps_its_scheduled_slot() {
    let network = two_train_network();
    let realtime = RailRtFeed {
        trip_updates: vec![TripUpdate {
            trip_id: Some("T1".to_string()),
            canceled: true,
            delay_secs: Some(1200),
            ..Default::default()
        }],
        ..Default::default()
    };
    let board = board_at(&network, &realtime, "NS4", "08:00:00", 3600);

    // The canceled run stays visible, first, at its scheduled time:
    // nothing predicts a train that will not run.
    assert_eq!(board.rows.len(), 2);
    assert!(board.rows[0].canceled);
    assert_eq!(board.rows[0].clock_time, "08:10:00");
    assert_eq!(board.rows[0].departs_in_secs, 600);
    assert!(!board.rows[1].canceled);
}

#[test]
fn an_irrelevant_realtime_layer_changes_nothing() {
    let network = two_train_network();
    let station = network.station_by_code("NS4").unwrap();
    let build = |realtime: Option<&RailRtFeed>| {
        let mut builder = LiveBoardBuilder::new(&network);
        if let Some(realtime) = realtime {
            builder = builder.with_realtime(realtime);
        }
        builder.build(
            station,
            "20260810".parse().unwrap(),
            "08:00:00".parse().unwrap(),
            3600,
        )
    };

    let plain = build(None);
    // A feed that names only an unknown trip widens the schedule
    // query but must not move, add, or drop a row.
    let decorated = build(Some(&trip_delay("T9", 900)));
    assert_eq!(
        serde_json::to_value(&plain).unwrap(),
        serde_json::to_value(&decorated).unwrap()
    );
    assert_eq!(plain.rows.len(), 2);
    assert_eq!(plain.rows[0].departs_in_secs, 600);
    assert_eq!(plain.rows[0].clock_time, "08:10:00");
}

// ----------------------------------------------------------------------
// Which trip update reaches which row
// ----------------------------------------------------------------------

/// The tiny network with one headway block instead of the fixed trip.
///
/// `TF` starts a run at 08:00 and another at 08:10, and each of them
/// calls NS4 ten minutes after it started: 08:10 and 08:20. Both rows
/// carry the same `trip_id`, so only the start of the run tells them
/// apart.
fn frequency_network() -> RailNetwork {
    let mut feed = tiny_feed();
    feed.trips = vec![Trip {
        route_id: "NS".to_string(),
        service_id: "DAILY".to_string(),
        trip_id: "TF".to_string(),
        trip_headsign: Some("Marina Bay".to_string()),
        direction_id: Some(0),
        ..Default::default()
    }];
    feed.stop_times = vec![
        stop_time("TF", "08:00:00", "S_NS1", 1),
        stop_time("TF", "08:10:00", "S_NS4", 2),
        stop_time("TF", "08:30:00", "S_NS27", 3),
    ];
    feed.frequencies = vec![Frequency {
        trip_id: "TF".to_string(),
        start_time: "08:00:00".parse().unwrap(),
        end_time: "08:20:00".parse().unwrap(),
        headway_secs: 600,
        exact_times: Some(1),
    }];
    RailNetwork::from_feed(&feed).unwrap()
}

/// A trip update that names one run of a headway block.
fn run_update(trip_id: &str, start_time: Option<&str>, delay_secs: i32) -> RailRtFeed {
    RailRtFeed {
        trip_updates: vec![TripUpdate {
            trip_id: Some(trip_id.to_string()),
            start_time: start_time.map(str::to_string),
            delay_secs: Some(delay_secs),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn an_update_naming_one_start_time_moves_only_that_sibling() {
    let network = frequency_network();
    // The 08:10 run is five minutes late. Its sibling, which started
    // at 08:00 and calls here at 08:10, is not.
    let realtime = run_update("TF", Some("08:10:00"), 300);
    let board = board_at(&network, &realtime, "NS4", "08:00:00", 3600);

    assert_eq!(board.rows.len(), 2);
    assert_eq!(board.rows[0].clock_time, "08:10:00");
    assert_eq!(board.rows[0].delay_secs, None);
    assert_eq!(board.rows[0].departs_in_secs, 600);
    assert_eq!(board.rows[1].clock_time, "08:25:00");
    assert_eq!(board.rows[1].delay_secs, Some(300));
    assert_eq!(board.rows[1].departs_in_secs, 1500);
}

#[test]
fn a_cancellation_naming_one_start_time_cancels_only_that_sibling() {
    let network = frequency_network();
    let realtime = RailRtFeed {
        trip_updates: vec![TripUpdate {
            trip_id: Some("TF".to_string()),
            start_time: Some("08:00:00".to_string()),
            canceled: true,
            ..Default::default()
        }],
        ..Default::default()
    };
    let board = board_at(&network, &realtime, "NS4", "08:00:00", 3600);

    assert_eq!(board.rows.len(), 2);
    assert!(board.rows[0].canceled);
    assert_eq!(board.rows[0].clock_time, "08:10:00");
    assert!(!board.rows[1].canceled);
    assert_eq!(board.rows[1].clock_time, "08:20:00");
}

#[test]
fn a_skipped_stop_naming_one_start_time_removes_only_that_sibling() {
    let network = frequency_network();
    let realtime = RailRtFeed {
        trip_updates: vec![TripUpdate {
            trip_id: Some("TF".to_string()),
            start_time: Some("08:10:00".to_string()),
            stop_updates: vec![StopTimeUpdate {
                stop_id: Some("S_NS4".to_string()),
                skipped: true,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let board = board_at(&network, &realtime, "NS4", "08:00:00", 3600);

    // The 08:20 row is gone; the sibling that started at 08:00 keeps
    // its call.
    assert_eq!(board.rows.len(), 1);
    assert_eq!(board.rows[0].clock_time, "08:10:00");
}

#[test]
fn an_update_naming_no_start_time_reaches_no_sibling() {
    let network = frequency_network();
    // The operator says one of these trains is five minutes late and
    // does not say which. Marking both would state something the feed
    // never said, so the board marks neither — the rule the map view
    // model records as `realtime-update-ambiguous`.
    let realtime = run_update("TF", None, 300);
    let board = board_at(&network, &realtime, "NS4", "08:00:00", 3600);

    assert_eq!(board.rows.len(), 2);
    assert!(board.rows.iter().all(|row| row.delay_secs.is_none()));
    assert_eq!(board.rows[0].clock_time, "08:10:00");
    assert_eq!(board.rows[1].clock_time, "08:20:00");
}

/// A network with one run that calls at NS4 after midnight: the trip
/// starts on its own service day and reaches NS4 at 24:10:00, which is
/// 00:10 on the next calendar day.
fn past_midnight_network() -> RailNetwork {
    let mut feed = tiny_feed();
    feed.stop_times = vec![
        stop_time("T1", "24:00:00", "S_NS1", 1),
        stop_time("T1", "24:10:00", "S_NS4", 2),
        stop_time("T1", "24:30:00", "S_NS27", 3),
    ];
    RailNetwork::from_feed(&feed).unwrap()
}

/// A dated trip update: a delay the operator attributes to one
/// service day.
fn dated_update(trip_id: &str, start_date: &str, delay_secs: i32) -> RailRtFeed {
    RailRtFeed {
        trip_updates: vec![TripUpdate {
            trip_id: Some(trip_id.to_string()),
            start_date: Some(start_date.to_string()),
            delay_secs: Some(delay_secs),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn a_todays_update_cannot_change_yesterdays_run() {
    let network = past_midnight_network();
    // At 00:05 on the 11th the only train on the board is the run that
    // started on the 10th and reaches NS4 at 00:10. An update about the
    // 11th names the run that starts tonight, not this one.
    let station = network.station_by_code("NS4").unwrap();
    let build = |realtime: &RailRtFeed| {
        LiveBoardBuilder::new(&network)
            .with_realtime(realtime)
            .build(
                station,
                "20260811".parse().unwrap(),
                "00:05:00".parse().unwrap(),
                3600,
            )
    };

    let today = build(&dated_update("T1", "20260811", 300));
    assert_eq!(today.rows.len(), 1);
    assert_eq!(today.rows[0].delay_secs, None);
    assert_eq!(today.rows[0].clock_time, "00:10:00");
    assert_eq!(today.rows[0].departs_in_secs, 300);

    // The same update dated to the day the run belongs to does reach
    // it, so the test measures the date and not some other refusal.
    let yesterday = build(&dated_update("T1", "20260810", 300));
    assert_eq!(yesterday.rows.len(), 1);
    assert_eq!(yesterday.rows[0].delay_secs, Some(300));
    assert_eq!(yesterday.rows[0].clock_time, "00:15:00");
    assert_eq!(yesterday.rows[0].departs_in_secs, 600);
}

#[test]
fn a_todays_cancellation_cannot_cancel_yesterdays_run() {
    let network = past_midnight_network();
    let station = network.station_by_code("NS4").unwrap();
    let realtime = RailRtFeed {
        trip_updates: vec![TripUpdate {
            trip_id: Some("T1".to_string()),
            start_date: Some("20260811".to_string()),
            canceled: true,
            ..Default::default()
        }],
        ..Default::default()
    };
    let board = LiveBoardBuilder::new(&network)
        .with_realtime(&realtime)
        .build(
            station,
            "20260811".parse().unwrap(),
            "00:05:00".parse().unwrap(),
            3600,
        );

    assert_eq!(board.rows.len(), 1);
    assert!(!board.rows[0].canceled);
}

#[test]
fn an_undated_update_still_reaches_a_fixed_trip() {
    // The documented fallback: a feed that names no start date applies
    // on whichever of the two scanned days carries the trip. The board
    // must keep serving the LTA feed, which sends no descriptor dates.
    let network = past_midnight_network();
    let station = network.station_by_code("NS4").unwrap();
    let realtime = trip_delay("T1", 300);
    let board = LiveBoardBuilder::new(&network)
        .with_realtime(&realtime)
        .build(
            station,
            "20260811".parse().unwrap(),
            "00:05:00".parse().unwrap(),
            3600,
        );

    assert_eq!(board.rows.len(), 1);
    assert_eq!(board.rows[0].delay_secs, Some(300));
}

#[test]
fn a_start_time_never_narrows_a_fixed_trip() {
    // A fixed trip is named by its `trip_id` and its service date, so
    // a descriptor start time is not a second key to match on: an
    // update that carries one still reaches the run.
    let network = two_train_network();
    let realtime = run_update("T1", Some("09:99:99"), 360);
    let board = board_at(&network, &realtime, "NS4", "08:00:00", 3600);

    assert_eq!(board.rows.len(), 2);
    assert_eq!(board.rows[1].clock_time, "08:16:00");
    assert_eq!(board.rows[1].delay_secs, Some(360));
}

#[test]
fn a_blank_alert_text_reaches_no_notice() {
    let mut alert = rt_alert(AlertEffect::NoService, vec![route_entity("NS")]);
    alert.header = Some(" ".to_string());
    let board = build_with_alerts(&[alert], 1_000);
    // The effect still applies; only the empty text stays out.
    assert!(board.rows[0].canceled);
    assert!(board.notices.is_empty());
}
