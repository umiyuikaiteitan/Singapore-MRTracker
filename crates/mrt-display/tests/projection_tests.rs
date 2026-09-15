use std::path::PathBuf;

use mrt_display::{build_frame, Frame, FrameOptions, Layout, LayoutPixel, SourceState};
use mrt_gtfs::{DirectorySource, GtfsFeed, RailNetwork};
use mrt_gtfs_rt::{
    ActivePeriod, Alert, AlertCause, AlertEffect, InformedEntity, RailRtFeed, StopTimeEvent,
    StopTimeUpdate, TripUpdate,
};

fn feed() -> GtfsFeed {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mrt-gtfs/tests/fixtures/mini");
    GtfsFeed::load(&mut DirectorySource::new(path)).unwrap()
}

fn network() -> RailNetwork {
    RailNetwork::from_feed(&feed()).unwrap()
}

fn layout() -> Layout {
    serde_json::from_str(include_str!("fixtures/tel-four.json")).unwrap()
}

fn one_station(station: &str, line: Option<&str>) -> Layout {
    Layout {
        schema_version: 1,
        layout_id: "test-v1".into(),
        led_count: 1,
        pixels: vec![LayoutPixel {
            index: 0,
            station: station.into(),
            station_aliases: Vec::new(),
            line: line.map(str::to_string),
            x_mm: 10.0,
            y_mm: 10.0,
        }],
        board_station: station.into(),
    }
}

fn options(clock: &str) -> FrameOptions {
    FrameOptions::new(1000, "20260810".parse().unwrap(), clock.parse().unwrap())
}

fn fresh(update: TripUpdate) -> RailRtFeed {
    RailRtFeed {
        feed_timestamp: Some(990),
        trip_updates: vec![update],
        ..Default::default()
    }
}

fn delay(trip: &str, seconds: i32) -> TripUpdate {
    TripUpdate {
        trip_id: Some(trip.into()),
        delay_secs: Some(seconds),
        timestamp: Some(980),
        ..Default::default()
    }
}

#[test]
fn deterministic_complete_frame_has_stable_json_contract() {
    let network = network();
    let mut layout = layout();
    layout.pixels.reverse();
    let frame = build_frame(&network, &layout, None, &options("06:00:00")).unwrap();
    let expected = r#"{"schema_version":1,"layout_id":"fixture-mini-v1","generated_at":1000,"valid_until":1120,"source_state":"schedule","pixels":[{"index":0,"r":14,"g":8,"b":3},{"index":1,"r":14,"g":8,"b":3},{"index":2,"r":14,"g":8,"b":3},{"index":3,"r":14,"g":8,"b":3}],"board":{"station_name":"Woodlands North","rows":[{"line":"TEL","destination":"Springleaf","minutes":1,"approximate":false,"realtime":false},{"line":"TEL","destination":"Woodlands South","minutes":21,"approximate":false,"realtime":false}],"notice":"Timetable departures; live predictions unavailable. Station departure activity."}}"#;
    assert_eq!(serde_json::to_string(&frame).unwrap(), expected);
    assert_eq!(serde_json::from_str::<Frame>(expected).unwrap(), frame);
}

#[test]
fn empty_realtime_and_vehicle_only_feeds_do_not_upgrade_timetable_rows() {
    let feed = RailRtFeed {
        feed_timestamp: Some(999),
        ..Default::default()
    };
    let frame = build_frame(&network(), &layout(), Some(&feed), &options("06:00:00")).unwrap();
    assert_eq!(frame.source_state, SourceState::Schedule);
    assert!(frame.board.rows.iter().all(|row| !row.realtime));
}

#[test]
fn fresh_delay_changes_countdown_and_caps_frame_to_measurement_expiry() {
    let frame = build_frame(
        &network(),
        &layout(),
        Some(&fresh(delay("TE_T1", 120))),
        &options("06:00:00"),
    )
    .unwrap();
    assert_eq!(frame.source_state, SourceState::Realtime);
    assert_eq!(frame.board.rows[0].minutes, 3);
    assert!(frame.board.rows[0].realtime);
    assert!(!frame.board.rows[1].realtime);
    assert_eq!(frame.valid_until, 1100);
    assert!(frame.pixels[0].r > 14);
}

#[test]
fn delayed_departure_survives_its_scheduled_time() {
    let frame = build_frame(
        &network(),
        &layout(),
        Some(&fresh(delay("TE_T1", 120))),
        &options("06:01:00"),
    )
    .unwrap();
    assert_eq!(frame.board.rows[0].destination, "Springleaf");
    assert_eq!(frame.board.rows[0].minutes, 2);
    assert!(frame.board.rows[0].realtime);
}

#[test]
fn stale_missing_and_future_feed_timestamps_show_explicit_fallback() {
    for timestamp in [None, Some(0), Some(880), Some(1001)] {
        let mut rt = fresh(delay("TE_T1", 120));
        rt.feed_timestamp = timestamp;
        let frame = build_frame(&network(), &layout(), Some(&rt), &options("06:00:00")).unwrap();
        assert_eq!(frame.source_state, SourceState::Stale, "{timestamp:?}");
        assert!(frame.board.rows.iter().all(|row| !row.realtime));
        assert_eq!(frame.board.rows[0].minutes, 1);
        assert!(frame
            .pixels
            .iter()
            .all(|pixel| pixel.r == 0 && pixel.g == 0 && pixel.b == 0));
        assert!(frame.board.notice.contains("timetable fallback"));
    }
}

#[test]
fn fresh_feed_does_not_refresh_stale_or_future_trip_measurements() {
    for timestamp in [Some(800), Some(1001)] {
        let mut update = delay("TE_T1", 120);
        update.timestamp = timestamp;
        let frame = build_frame(
            &network(),
            &layout(),
            Some(&fresh(update)),
            &options("06:00:00"),
        )
        .unwrap();
        assert_eq!(frame.source_state, SourceState::Stale);
        assert!(!frame.board.rows[0].realtime);
    }
}

#[test]
fn zero_delay_is_realtime_but_unmatched_updates_are_not() {
    let zero = build_frame(
        &network(),
        &layout(),
        Some(&fresh(delay("TE_T1", 0))),
        &options("06:00:00"),
    )
    .unwrap();
    assert!(zero.board.rows[0].realtime);
    let unmatched = build_frame(
        &network(),
        &layout(),
        Some(&fresh(delay("MISSING", 0))),
        &options("06:00:00"),
    )
    .unwrap();
    assert_eq!(unmatched.source_state, SourceState::Schedule);
}

#[test]
fn trip_cancellation_and_skipped_platform_remove_departures() {
    let canceled = TripUpdate {
        trip_id: Some("TE_T1".into()),
        canceled: true,
        ..Default::default()
    };
    let skipped = TripUpdate {
        trip_id: Some("TE_T1".into()),
        stop_updates: vec![StopTimeUpdate {
            stop_id: Some("WDN_1".into()),
            skipped: true,
            ..Default::default()
        }],
        ..Default::default()
    };
    for update in [canceled, skipped] {
        let frame = build_frame(
            &network(),
            &layout(),
            Some(&fresh(update)),
            &options("06:00:00"),
        )
        .unwrap();
        assert_eq!(frame.board.rows.len(), 1);
        assert_eq!(frame.board.rows[0].destination, "Woodlands South");
        assert_eq!(frame.source_state, SourceState::Realtime);
        assert!(!frame.board.rows[0].realtime);
        assert!(frame.board.notice.contains("Canceled/skipped"));
    }
}

#[test]
fn frequency_update_requires_a_specific_run_and_never_cancels_whole_template() {
    let mut update = TripUpdate {
        trip_id: Some("TE_F1".into()),
        canceled: true,
        ..Default::default()
    };
    let no_identity = build_frame(
        &network(),
        &layout(),
        Some(&fresh(update.clone())),
        &options("05:00:00"),
    )
    .unwrap();
    assert_eq!(no_identity.board.rows.len(), 3);
    assert_eq!(no_identity.source_state, SourceState::Schedule);
    update.start_date = Some("20260810".into());
    update.start_time = Some("05:10:00".into());
    let matched = build_frame(
        &network(),
        &layout(),
        Some(&fresh(update)),
        &options("05:00:00"),
    )
    .unwrap();
    assert_eq!(
        matched
            .board
            .rows
            .iter()
            .map(|row| row.minutes)
            .collect::<Vec<_>>(),
        [0, 20]
    );
}

#[test]
fn approximate_headway_departures_retain_approximate_provenance() {
    let layout = one_station("BP1", Some("BPL"));
    let frame = build_frame(&network(), &layout, None, &options("05:30:00")).unwrap();
    assert_eq!(frame.board.rows.len(), 3);
    assert!(frame
        .board
        .rows
        .iter()
        .all(|row| row.approximate && !row.realtime));
    assert!(frame.board.notice.contains("Approximate"));
}

#[test]
fn no_pickup_terminal_and_interpolated_calls_are_handled() {
    let layout = one_station("TE2", Some("TEL"));
    let no_pickup = build_frame(&network(), &layout, None, &options("08:00:00")).unwrap();
    assert!(no_pickup.board.rows.is_empty());
    let interpolated = build_frame(&network(), &layout, None, &options("07:00:00")).unwrap();
    assert!(interpolated.board.rows[0].approximate);
    let terminal = build_frame(
        &network(),
        &one_station("TE4", None),
        None,
        &options("06:11:00"),
    )
    .unwrap();
    assert!(terminal.board.rows.is_empty());
}

#[test]
fn previous_service_day_times_are_shown_after_midnight() {
    let mut opts = options("00:00:00");
    opts.service_date = "20260811".parse().unwrap();
    let frame = build_frame(&network(), &one_station("NS4", Some("NSL")), None, &opts).unwrap();
    assert_eq!(frame.board.rows[0].minutes, 6);
    assert_eq!(frame.board.rows[0].destination, "Marina Bay");
}

#[test]
fn wrong_date_route_and_ambiguous_updates_do_not_attach() {
    let mut wrong_date = delay("TE_T1", 120);
    wrong_date.start_date = Some("20260809".into());
    let mut wrong_route = delay("TE_T1", 120);
    wrong_route.route_id = Some("NS".into());
    for rt in [
        fresh(wrong_date),
        fresh(wrong_route),
        RailRtFeed {
            feed_timestamp: Some(990),
            trip_updates: vec![delay("TE_T1", 120), delay("TE_T1", 60)],
            ..Default::default()
        },
    ] {
        let frame = build_frame(&network(), &layout(), Some(&rt), &options("06:00:00")).unwrap();
        assert_eq!(frame.source_state, SourceState::Schedule);
        assert_eq!(frame.board.rows[0].minutes, 1);
    }
}

#[test]
fn only_the_used_platform_delay_is_applied_and_no_data_does_not_inherit_trip_delay() {
    let mut update = delay("TE_T1", 300);
    update.stop_updates = vec![StopTimeUpdate {
        stop_id: Some("WDL_1".into()),
        departure: Some(StopTimeEvent {
            delay_secs: Some(120),
            time: None,
        }),
        ..Default::default()
    }];
    let frame = build_frame(
        &network(),
        &layout(),
        Some(&fresh(update.clone())),
        &options("06:00:00"),
    )
    .unwrap();
    assert!(!frame.board.rows[0].realtime);
    assert_eq!(frame.board.rows[0].minutes, 1);
    assert!(frame.pixels[1].r > frame.pixels[0].r);
    update.stop_updates[0] = StopTimeUpdate {
        stop_id: Some("WDN_1".into()),
        ..Default::default()
    };
    let no_data = build_frame(
        &network(),
        &layout(),
        Some(&fresh(update)),
        &options("06:00:00"),
    )
    .unwrap();
    assert!(!no_data.board.rows[0].realtime);
}

#[test]
fn absolute_stop_times_are_realtime_and_override_conflicting_delays() {
    let update = TripUpdate {
        trip_id: Some("TE_T1".into()),
        stop_updates: vec![StopTimeUpdate {
            stop_id: Some("WDN_1".into()),
            departure: Some(StopTimeEvent {
                time: Some(1090),
                delay_secs: Some(900),
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    let frame = build_frame(
        &network(),
        &layout(),
        Some(&fresh(update)),
        &options("06:00:00"),
    )
    .unwrap();
    assert_eq!(frame.board.rows[0].minutes, 2);
    assert!(frame.board.rows[0].realtime);
}

#[test]
fn arrival_only_absolute_prediction_preserves_scheduled_dwell() {
    let update = TripUpdate {
        trip_id: Some("TE_T1".into()),
        stop_updates: vec![StopTimeUpdate {
            stop_id: Some("WDN_1".into()),
            arrival: Some(StopTimeEvent {
                time: Some(1060),
                delay_secs: None,
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    let frame = build_frame(
        &network(),
        &layout(),
        Some(&fresh(update)),
        &options("06:00:00"),
    )
    .unwrap();
    assert_eq!(frame.board.rows[0].minutes, 2); // arrival in 60s + 30s dwell
    assert!(frame.board.rows[0].realtime);
}

#[test]
fn a_repeated_loop_platform_requires_unavailable_sequence_identity() {
    let update = TripUpdate {
        trip_id: Some("PW_L1".into()),
        stop_updates: vec![StopTimeUpdate {
            stop_id: Some("PGL_1".into()),
            stop_sequence: Some(4),
            skipped: true,
            departure: Some(StopTimeEvent {
                time: None,
                delay_secs: Some(120),
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    let frame = build_frame(
        &network(),
        &one_station("PTC", None),
        Some(&fresh(update)),
        &options("06:00:00"),
    )
    .unwrap();
    assert_eq!(frame.board.rows.len(), 1);
    assert!(!frame.board.rows[0].realtime);
    assert_eq!(frame.board.rows[0].minutes, 1);
}

fn alert(informed: InformedEntity) -> Alert {
    Alert {
        cause: AlertCause::Unknown,
        effect: AlertEffect::NoService,
        header: Some("Service suspended".into()),
        description: None,
        url: None,
        active_periods: vec![ActivePeriod {
            start: Some(900),
            end: Some(1100),
        }],
        informed: vec![informed],
    }
}

#[test]
fn alert_selector_fields_are_conjunctive_and_end_is_exclusive() {
    let mut rt = RailRtFeed {
        feed_timestamp: Some(990),
        ..Default::default()
    };
    rt.alerts.push(alert(InformedEntity {
        route_id: Some("TE".into()),
        stop_id: Some("NONEXISTENT".into()),
        ..Default::default()
    }));
    let mismatch = build_frame(&network(), &layout(), Some(&rt), &options("06:00:00")).unwrap();
    assert_eq!(mismatch.board.rows.len(), 2);
    rt.alerts[0].informed[0].stop_id = Some("WDN_1".into());
    let matched = build_frame(&network(), &layout(), Some(&rt), &options("06:00:00")).unwrap();
    assert!(matched.board.rows.is_empty());
    rt.alerts[0].active_periods[0].end = Some(1000);
    let ended = build_frame(&network(), &layout(), Some(&rt), &options("06:00:00")).unwrap();
    assert_eq!(ended.board.rows.len(), 2);
}

#[test]
fn malformed_layouts_fail_instead_of_silently_remapping_leds() {
    let network = network();
    let valid = layout();
    let mut invalid = valid.clone();
    invalid.pixels[0].station = "Woodlands North".into();
    assert!(invalid
        .validate(&network)
        .unwrap_err()
        .to_string()
        .contains("unknown station"));
    invalid = valid.clone();
    invalid.pixels[0].line = Some("NSL".into());
    assert!(invalid.validate(&network).is_err());
    invalid = valid.clone();
    invalid.pixels[0].index = 1;
    assert!(invalid.validate(&network).is_err());
    invalid = valid.clone();
    invalid.pixels[0].index = 500;
    assert!(invalid.validate(&network).is_err());
    invalid = valid.clone();
    invalid.pixels.pop();
    assert!(invalid.validate(&network).is_err());
    invalid = valid.clone();
    invalid.pixels[0].x_mm = f64::NAN;
    assert!(invalid.validate(&network).is_err());
    invalid = valid.clone();
    invalid.schema_version = 2;
    assert!(invalid.validate(&network).is_err());
    invalid = valid;
    invalid.board_station = "TE999".into();
    assert!(invalid.validate(&network).is_err());
}

#[test]
fn duplicate_station_codes_are_rejected_even_if_network_index_accepts_them() {
    let mut source = feed();
    source
        .stops
        .iter_mut()
        .find(|stop| stop.stop_id == "WDL_1")
        .unwrap()
        .stop_code = Some("TE1".into());
    let network = RailNetwork::from_feed(&source).unwrap();
    assert!(layout()
        .validate(&network)
        .unwrap_err()
        .to_string()
        .contains("ambiguous station"));
}

#[test]
fn explicit_aliases_merge_separate_interchange_parents_and_deduplicate_grouped_ones() {
    let mut interchange = one_station("NS1", None);
    interchange.pixels[0].station_aliases = vec!["EW24".into()];
    let grouped = build_frame(&network(), &interchange, None, &options("06:00:00")).unwrap();
    assert_eq!(grouped.board.rows.len(), 2);
    assert_eq!(
        grouped
            .board
            .rows
            .iter()
            .map(|row| row.line.as_str())
            .collect::<Vec<_>>(),
        ["NSL", "EWL"]
    );

    let mut source = feed();
    let mut separate_parent = source
        .stops
        .iter()
        .find(|stop| stop.stop_id == "JUR")
        .unwrap()
        .clone();
    separate_parent.stop_id = "JUR_EW_PARENT".into();
    source.stops.push(separate_parent);
    source
        .stops
        .iter_mut()
        .find(|stop| stop.stop_id == "JUR_EW")
        .unwrap()
        .parent_station = Some("JUR_EW_PARENT".into());
    let split = RailNetwork::from_feed(&source).unwrap();
    let separate = build_frame(&split, &interchange, None, &options("06:00:00")).unwrap();
    assert_eq!(separate.board.rows, grouped.board.rows);
    assert!(interchange.warnings(&split).unwrap().is_empty());
}

#[test]
fn historical_alias_fallback_is_explicit_and_all_missing_groups_still_fail() {
    let mut renamed = one_station("NEW_CODE", None);
    renamed.pixels[0].station_aliases = vec!["TE1".into()];
    let network = network();
    let frame = build_frame(&network, &renamed, None, &options("06:00:00")).unwrap();
    assert_eq!(frame.board.station_name, "Woodlands North");
    assert_eq!(renamed.warnings(&network).unwrap().len(), 1);
    assert!(frame.board.notice.contains("1 layout identifier(s) absent"));
    renamed.pixels[0].station_aliases = vec!["TE999".into()];
    assert!(renamed.validate(&network).is_err());
}

#[test]
fn utf8_text_is_bounded_by_bytes_for_embedded_consumers() {
    let mut source = feed();
    source
        .stops
        .iter_mut()
        .find(|stop| stop.stop_id == "WDN")
        .unwrap()
        .stop_name = Some("界".repeat(100));
    source
        .trips
        .iter_mut()
        .find(|trip| trip.trip_id == "TE_T1")
        .unwrap()
        .trip_headsign = Some("界".repeat(100));
    let network = RailNetwork::from_feed(&source).unwrap();
    let frame = build_frame(&network, &layout(), None, &options("06:00:00")).unwrap();
    assert!(frame.board.station_name.len() <= 80);
    assert!(frame.board.rows[0].destination.len() <= 96);
    let unavailable = Frame::unavailable(&layout(), 1000, 120, &"界".repeat(100)).unwrap();
    assert!(unavailable.board.notice.len() <= 240);
}

#[test]
fn dark_frames_clear_every_led_and_have_explicit_unavailable_state() {
    let frame = Frame::unavailable(&layout(), 1000, 120, "GTFS download failed").unwrap();
    assert_eq!(frame.source_state, SourceState::Unavailable);
    assert_eq!(frame.pixels.len(), 4);
    assert!(frame
        .pixels
        .iter()
        .all(|pixel| pixel.r == 0 && pixel.g == 0 && pixel.b == 0));
    assert!(frame.board.rows.is_empty());
    assert!(frame.board.notice.starts_with("Data unavailable."));
}

#[test]
fn empty_schedule_window_does_not_claim_feed_validity_or_train_presence() {
    let frame = build_frame(&network(), &layout(), None, &options("03:00:00")).unwrap();
    assert_eq!(frame.source_state, SourceState::Schedule);
    assert!(frame.board.rows.is_empty());
    assert!(frame
        .pixels
        .iter()
        .all(|pixel| pixel.r == 0 && pixel.g == 0 && pixel.b == 0));
    assert!(frame.board.notice.contains("No upcoming departures"));
}

#[test]
fn invalid_clock_and_lifetime_cannot_overflow_or_produce_unbounded_frames() {
    for (ttl, rows, clock) in [
        (0, 6, "06:00:00"),
        (301, 6, "06:00:00"),
        (120, 9, "06:00:00"),
        (120, 6, "25:00:00"),
    ] {
        let mut opts = options(clock);
        opts.ttl_secs = ttl;
        opts.max_rows = rows;
        assert!(build_frame(&network(), &layout(), None, &opts).is_err());
    }
    assert!(Frame::unavailable(&layout(), u64::MAX, 120, "offline").is_err());
}
