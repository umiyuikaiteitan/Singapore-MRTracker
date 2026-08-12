//! Tests for the live composition layer.
//!
//! The tests build a small in-memory GTFS feed, then merge synthetic
//! live layers into it. No test touches the network.

use mrt_datamall::{CrowdLevel, PlatformCrowd, ServiceStatus, TrainLine, TrainServiceAlerts};
use mrt_gtfs::{Calendar, GtfsFeed, RailNetwork, Route, Stop, StopTime, Trip};
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
        stop_headsign: None,
        pickup_type: None,
        drop_off_type: None,
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
            shape_id: None,
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
