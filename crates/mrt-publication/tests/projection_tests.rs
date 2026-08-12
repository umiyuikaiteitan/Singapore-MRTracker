//! Integration tests for the timetable and diagram projections.
//!
//! The tests use the miniature feed of `mrt-gtfs`.

use std::path::PathBuf;

use mrt_gtfs::{
    FrequencyPolicy, GtfsFeed, GtfsTime, MissingTimePolicy, RailNetwork, ServiceDate, StationId,
    TimeExactness,
};
use mrt_publication::{
    build_diagram, build_timetable, AxisDirection, ColumnLayout, DepartureFlag, DiagramTarget,
    DocumentSeed, Language, PublicationConfig, StationSpacing, TickLevel, TimetablePanel,
};

fn network() -> RailNetwork {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mrt-gtfs/tests/fixtures/mini");
    RailNetwork::from_feed(&GtfsFeed::from_dir(dir).unwrap()).unwrap()
}

fn date(s: &str) -> ServiceDate {
    s.parse().unwrap()
}

fn time(s: &str) -> GtfsTime {
    s.parse().unwrap()
}

fn seed() -> DocumentSeed {
    DocumentSeed {
        generator_version: "test".into(),
        feed_sha256: "0123456789abcdef".repeat(4),
        feed_timestamp: Some("2026-08-10T00:00:00+08:00".into()),
        timezone: "Asia/Singapore".into(),
        generated_from_cache: false,
        configuration_sha256: "cafebabe".into(),
    }
}

fn station(network: &RailNetwork, code: &str) -> StationId {
    network.station_by_code(code).unwrap()
}

// ----------------------------------------------------------------------
// Timetable
// ----------------------------------------------------------------------

#[test]
fn a_timetable_groups_by_line_platform_and_direction() {
    let network = network();
    let config = PublicationConfig::default();
    let document = build_timetable(
        &network,
        station(&network, "TE2"),
        date("20250505"),
        None,
        &config,
        &seed(),
    )
    .unwrap();

    // Woodlands has platform 1 southbound and platform 2 northbound.
    let headings: Vec<(&str, Option<&str>)> = document
        .panels
        .iter()
        .map(|p| (p.direction_label.as_str(), p.platform_label.as_deref()))
        .collect();
    assert_eq!(headings.len(), 2, "{headings:?}");
    assert!(headings.iter().any(|(_, p)| *p == Some("Platform 1")));
    assert!(headings.iter().any(|(_, p)| *p == Some("Platform 2")));

    let southbound = document
        .panels
        .iter()
        .find(|p| p.platform_label.as_deref() == Some("Platform 1"))
        .unwrap();
    // Two termini in one direction stay in one panel.
    assert_eq!(
        southbound.destination_summary,
        vec![
            "Springleaf".to_string(),
            "Branch Beta".to_string(),
            "Woodlands South".to_string(),
        ]
    );
    assert!(southbound
        .direction_label
        .starts_with("For Springleaf / Branch Beta"));
}

#[test]
fn every_boardable_departure_appears_exactly_once() {
    let network = network();
    let document = build_timetable(
        &network,
        station(&network, "TE1"),
        date("20250505"),
        None,
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let mut keys: Vec<String> = document
        .panels
        .iter()
        .flat_map(|p| p.hour_groups.iter())
        .flat_map(|g| g.departures.iter())
        .map(|d| format!("{}@{}", d.instance_id, d.scheduled_time))
        .collect();
    let before = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), before, "a departure appeared twice");

    // TE_T1, TE_T2, TE_M1, TE_P1, TE_B1 depart from Woodlands North,
    // and the exact headway block adds three more runs.
    assert_eq!(before, 8, "{keys:?}");
}

#[test]
fn a_terminus_carries_no_boarding_departure() {
    let network = network();
    let document = build_timetable(
        &network,
        station(&network, "TE4"),
        date("20250505"),
        None,
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();
    // Only the northbound run starts at Springleaf; every southbound
    // run ends there.
    assert_eq!(document.departure_count(), 1);
}

#[test]
fn a_loop_end_that_forbids_boarding_stays_out() {
    let network = network();
    let document = build_timetable(
        &network,
        station(&network, "PTC"),
        date("20250505"),
        None,
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();
    // Punggol appears twice on the loop run, but only the first call
    // permits boarding.
    assert_eq!(document.departure_count(), 1);
    let departure = &document.panels[0].hour_groups[document.panels[0]
        .hour_groups
        .iter()
        .position(|g| !g.departures.is_empty())
        .unwrap()]
    .departures[0];
    assert_eq!(departure.scheduled_time.to_string(), "06:00:30");
}

#[test]
fn hours_follow_the_service_day_not_the_clock() {
    let network = network();
    let document = build_timetable(
        &network,
        station(&network, "NS1"),
        date("20250505"),
        None,
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let panel = &document.panels[0];
    let hours: Vec<u32> = panel.hour_groups.iter().map(|g| g.service_hour).collect();
    assert_eq!(hours.first(), Some(&4));
    assert_eq!(hours.last(), Some(&27));
    let display: Vec<u8> = panel.hour_groups.iter().map(|g| g.display_hour).collect();
    assert_eq!(&display[19..], &[23, 0, 1, 2, 3]);

    // NS_T5 leaves Jurong East at 23:50:30 on the service day.
    let late = panel
        .hour_groups
        .iter()
        .find(|g| g.service_hour == 23)
        .unwrap();
    assert_eq!(late.departures.len(), 1);
    assert_eq!(late.departures[0].display_minute, 50);
    assert_eq!(late.departures[0].display_seconds, Some(30));
}

#[test]
fn a_past_midnight_departure_is_marked_and_displays_a_small_hour() {
    let network = network();
    let document = build_timetable(
        &network,
        station(&network, "NS4"),
        date("20250505"),
        None,
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let group = document.panels[0]
        .hour_groups
        .iter()
        .find(|g| g.service_hour == 24)
        .unwrap();
    assert_eq!(group.display_hour, 0);
    let departure = &group.departures[0];
    assert_eq!(departure.scheduled_time.to_string(), "24:05:30");
    assert!(departure.flags.contains(&DepartureFlag::PastMidnight));
    assert!(document.legend.iter().any(|l| l.key == "past-midnight"));
}

#[test]
fn non_exact_headway_service_becomes_a_band_row() {
    let network = network();
    // BP1 is Choa Chu Kang, an interchange, so the query narrows to
    // the LRT line.
    let document = build_timetable(
        &network,
        station(&network, "BP1"),
        date("20250505"),
        network.line_by_route_id("BP"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    // The Bukit Panjang LRT has no exact departures in the fixture.
    assert_eq!(document.departure_count(), 0);
    let panel = document
        .panels
        .iter()
        .find(|p| p.line.route_id == "BP")
        .expect("the LRT panel exists");
    assert_eq!(panel.frequency_notes.len(), 1);
    let note = &panel.frequency_notes[0];
    assert_eq!(note.headway_minutes, 10);
    assert_eq!(note.text, "05:30\u{2013}06:00  every 10 min approximately");
    assert_eq!(note.destination, "South View");
    assert!(document.legend.iter().any(|l| l.key == "headway"));
}

#[test]
fn expanded_headway_departures_all_carry_the_approximation_mark() {
    let network = network();
    let config = PublicationConfig {
        frequency_policy: FrequencyPolicy::ExpandApproximate,
        ..Default::default()
    };
    let document = build_timetable(
        &network,
        station(&network, "BP1"),
        date("20250505"),
        network.line_by_route_id("BP"),
        &config,
        &seed(),
    )
    .unwrap();

    assert_eq!(document.departure_count(), 3);
    let marked = document
        .panels
        .iter()
        .flat_map(|p| p.hour_groups.iter())
        .flat_map(|g| g.departures.iter())
        .all(|d| {
            d.exactness == TimeExactness::Approximate
                && d.flags.contains(&DepartureFlag::Approximate)
        });
    assert!(marked, "an expanded departure is missing its mark");
    assert!(document.legend.iter().any(|l| l.key == "approximate"));
}

#[test]
fn a_computed_time_is_marked_as_computed() {
    let network = network();
    let document = build_timetable(
        &network,
        station(&network, "TE2"),
        date("20250505"),
        None,
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let interpolated = document
        .panels
        .iter()
        .flat_map(|p| p.hour_groups.iter())
        .flat_map(|g| g.departures.iter())
        .find(|d| d.source_trip_id == "TE_M1")
        .expect("the run with missing times appears");
    assert!(interpolated.flags.contains(&DepartureFlag::Interpolated));
    assert_eq!(interpolated.scheduled_time.to_string(), "07:04:15");
    assert!(document.legend.iter().any(|l| l.key == "interpolated"));
}

#[test]
fn a_stop_headsign_beats_the_trip_headsign() {
    let network = network();
    let document = build_timetable(
        &network,
        station(&network, "TE1"),
        date("20250505"),
        None,
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let express = document
        .panels
        .iter()
        .flat_map(|p| p.hour_groups.iter())
        .flat_map(|g| g.departures.iter())
        .find(|d| d.source_trip_id == "TE_P1")
        .unwrap();
    assert_eq!(express.destination_full, "Springleaf Express");
    assert_eq!(express.platform.as_deref(), Some("1"));
    assert_eq!(express.trip_short_name.as_deref(), Some("T107"));
}

#[test]
fn configuration_can_override_directions_platforms_and_names() {
    let network = network();
    let mut config = PublicationConfig {
        language: Language::Ja,
        ..Default::default()
    };
    config.labels.direction_overrides.insert(
        "TE:0".into(),
        mrt_publication::LocalizedText::both("Down", "下り"),
    );
    config
        .labels
        .platform_overrides
        .insert("WDN_1".into(), "A".into());
    config
        .labels
        .destination_abbreviations
        .insert("Springleaf".into(), "SPL".into());

    let document = build_timetable(
        &network,
        station(&network, "TE1"),
        date("20250505"),
        None,
        &config,
        &seed(),
    )
    .unwrap();

    let panel = document
        .panels
        .iter()
        .find(|p| p.direction == Some(0))
        .unwrap();
    assert_eq!(panel.direction_label, "下り");
    assert_eq!(panel.platform_label.as_deref(), Some("A番のりば"));
    assert!(panel
        .hour_groups
        .iter()
        .flat_map(|g| g.departures.iter())
        .any(|d| d.destination == "SPL" && d.destination_full == "Springleaf"));
    assert!(document.title.get(Language::Ja).contains("発車時刻表"));
}

#[test]
fn the_column_layout_splits_the_hour_rows_in_order() {
    let network = network();
    let mut config = PublicationConfig::default();
    config.timetable.layout = ColumnLayout::Balanced;
    config.timetable.columns = 2;
    let document = build_timetable(
        &network,
        station(&network, "NS1"),
        date("20250505"),
        None,
        &config,
        &seed(),
    )
    .unwrap();

    let panel: &TimetablePanel = &document.panels[0];
    let columns = panel.columns();
    assert_eq!(columns.len(), 2);
    let rejoined: Vec<u32> = columns
        .iter()
        .flat_map(|c| c.iter())
        .map(|g| g.service_hour)
        .collect();
    let straight: Vec<u32> = panel.hour_groups.iter().map(|g| g.service_hour).collect();
    assert_eq!(rejoined, straight, "the columns changed the hour order");

    let mut single = config.clone();
    single.timetable.layout = ColumnLayout::Single;
    let document = build_timetable(
        &network,
        station(&network, "NS1"),
        date("20250505"),
        None,
        &single,
        &seed(),
    )
    .unwrap();
    assert!(document.panels[0].column_breaks.is_empty());
    assert_eq!(document.panels[0].columns().len(), 1);
}

#[test]
fn an_empty_day_reports_a_warning_and_no_panels() {
    let network = network();
    let document = build_timetable(
        &network,
        station(&network, "TE1"),
        date("20250503"), // a Saturday: the TEL runs on weekdays only
        None,
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();
    assert!(document.panels.is_empty());
    assert!(document
        .metadata
        .diagnostics
        .iter()
        .any(|d| d.code == "timetable-empty"));
    assert!(!document.metadata.warnings.is_empty());
}

#[test]
fn the_document_is_deterministic() {
    let network = network();
    let config = PublicationConfig::default();
    let a = build_timetable(
        &network,
        station(&network, "TE1"),
        date("20250505"),
        None,
        &config,
        &seed(),
    )
    .unwrap();
    let b = build_timetable(
        &network,
        station(&network, "TE1"),
        date("20250505"),
        None,
        &config,
        &seed(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

// ----------------------------------------------------------------------
// Diagram
// ----------------------------------------------------------------------

fn tel(network: &RailNetwork) -> DiagramTarget {
    DiagramTarget::Line(network.line_by_route_id("TE").unwrap())
}

#[test]
fn a_diagram_puts_the_corridor_on_one_axis() {
    let network = network();
    let document = build_diagram(
        &network,
        &tel(&network),
        date("20250505"),
        time("05:00:00"),
        time("10:00:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    // The main axis holds the four TEL stations; the branch cannot
    // share it, so it becomes its own panel.
    assert_eq!(document.corridor.panels.len(), 2);
    let main = &document.corridor.panels[0];
    let names: Vec<&str> = document.corridor.nodes[main.first_node..=main.last_node]
        .iter()
        .map(|n| n.station.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "Woodlands North",
            "Woodlands",
            "Woodlands South",
            "Springleaf"
        ]
    );
    assert!(document
        .metadata
        .diagnostics
        .iter()
        .any(|d| d.code == "corridor-split"));

    // Equal spacing puts one row height between neighbours.
    let ys: Vec<f64> = document.corridor.nodes[main.first_node..=main.last_node]
        .iter()
        .map(|n| n.y)
        .collect();
    assert_eq!(ys, vec![0.0, 34.0, 68.0, 102.0]);
}

#[test]
fn opposite_directions_slope_opposite_ways() {
    let network = network();
    let document = build_diagram(
        &network,
        &tel(&network),
        date("20250505"),
        time("05:00:00"),
        time("10:00:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let down = document
        .runs
        .iter()
        .find(|r| r.source_trip_id == "TE_T1")
        .unwrap();
    let up = document
        .runs
        .iter()
        .find(|r| r.source_trip_id == "TE_T3")
        .unwrap();
    assert_eq!(down.axis_direction, AxisDirection::Down);
    assert_eq!(up.axis_direction, AxisDirection::Up);

    let slope = |run: &mrt_publication::DiagramRun| {
        run.points.last().unwrap().y - run.points.first().unwrap().y
    };
    assert!(slope(down) > 0.0);
    assert!(slope(up) < 0.0);
}

#[test]
fn a_path_carries_the_dwell_segment() {
    let network = network();
    let document = build_diagram(
        &network,
        &tel(&network),
        date("20250505"),
        time("05:00:00"),
        time("10:00:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let run = document
        .runs
        .iter()
        .find(|r| r.source_trip_id == "TE_T1")
        .unwrap();
    // Woodlands North: arrive 06:00:00, depart 06:00:30. Two points
    // at the same height make the horizontal dwell segment.
    assert_eq!(run.points[0].time.to_string(), "06:00:00");
    assert_eq!(run.points[1].time.to_string(), "06:00:30");
    assert_eq!(run.points[0].y, run.points[1].y);
    assert!(run.points[1].x > run.points[0].x);
    // Then the train travels to the next station.
    assert!(run.points[2].y > run.points[1].y);

    let mut without_dwell = PublicationConfig::default();
    without_dwell.diagram.show_dwell = false;
    let plain = build_diagram(
        &network,
        &tel(&network),
        date("20250505"),
        time("05:00:00"),
        time("10:00:00"),
        &without_dwell,
        &seed(),
    )
    .unwrap();
    let run = plain
        .runs
        .iter()
        .find(|r| r.source_trip_id == "TE_T1")
        .unwrap();
    assert_eq!(run.points.len(), 4);
}

#[test]
fn a_path_is_cut_at_the_window_edges() {
    let network = network();
    // TE_T1 runs 06:00 to 06:13. A window of 06:05 to 06:10 cuts both
    // ends.
    let document = build_diagram(
        &network,
        &tel(&network),
        date("20250505"),
        time("06:05:00"),
        time("06:10:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let run = document
        .runs
        .iter()
        .find(|r| r.source_trip_id == "TE_T1")
        .unwrap();
    assert!(run.clipped_start);
    assert!(run.clipped_end);
    assert_eq!(run.points.first().unwrap().time.to_string(), "06:05:00");
    assert_eq!(run.points.last().unwrap().time.to_string(), "06:10:00");
    assert!(run
        .points
        .iter()
        .all(|p| p.x >= document.layout.margin_left - 0.01
            && p.x <= document.layout.margin_left + document.layout.plot_width + 0.01));
}

#[test]
fn a_pass_through_call_is_marked() {
    let network = network();
    let document = build_diagram(
        &network,
        &tel(&network),
        date("20250505"),
        time("05:00:00"),
        time("10:00:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let express = document
        .runs
        .iter()
        .find(|r| r.source_trip_id == "TE_P1")
        .unwrap();
    assert!(!express.calls[1].stops);
    assert!(express.calls[0].stops);
    assert!(document.legend.iter().any(|l| l.key == "pass-through"));
}

#[test]
fn a_loop_repeats_its_anchor_station_with_a_new_occurrence() {
    let network = network();
    let document = build_diagram(
        &network,
        &DiagramTarget::Line(network.line_by_route_id("PW").unwrap()),
        date("20250505"),
        time("05:00:00"),
        time("08:00:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let keys: Vec<&str> = document
        .corridor
        .nodes
        .iter()
        .map(|n| n.key.as_str())
        .collect();
    assert_eq!(keys, vec!["PGL#0", "PWA#0", "PWB#0", "PGL#1"]);
    assert_eq!(document.corridor.nodes[3].occurrence, 1);

    let run = &document.runs[0];
    let nodes: Vec<usize> = run.calls.iter().map(|c| c.node).collect();
    assert_eq!(nodes, vec![0, 1, 2, 3]);
    // The path descends the whole way, so the loop reads as one
    // traversal instead of jumping back to the top.
    assert!(run.points.last().unwrap().y > run.points.first().unwrap().y);
}

#[test]
fn the_day_boundary_gets_its_own_grid_line() {
    let network = network();
    let document = build_diagram(
        &network,
        &DiagramTarget::Line(network.line_by_route_id("NS").unwrap()),
        date("20250505"),
        time("23:00:00"),
        time("25:00:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let boundary = document
        .time_axis
        .ticks
        .iter()
        .find(|t| t.level == TickLevel::DayBoundary)
        .expect("the diagram draws the 24:00 boundary");
    assert_eq!(boundary.time.to_string(), "24:00:00");
    assert_eq!(boundary.label.as_deref(), Some("24:00"));
    assert!(document.legend.iter().any(|l| l.key == "day-boundary"));

    // The midnight run stays on the service day and crosses the line.
    let run = document
        .runs
        .iter()
        .find(|r| r.source_trip_id == "NS_T5")
        .unwrap();
    assert!(run.points.last().unwrap().time.seconds() > 24 * 3600);
}

#[test]
fn a_configured_corridor_places_the_branch_on_the_axis() {
    let network = network();
    let mut config = PublicationConfig::default();
    config.corridors.push(mrt_publication::CorridorConfig {
        id: "tel-main".into(),
        line: Some("TE".into()),
        label: Some(mrt_publication::LocalizedText::en("TEL with the branch")),
        axis: vec!["TE1".into(), "TE2".into(), "TE3".into(), "TE4".into()],
        branches: vec![mrt_publication::BranchConfig {
            junction: "TE2".into(),
            axis: vec!["TB1".into(), "TB2".into()],
            label: None,
        }],
        offsets: Vec::new(),
    });

    let document = build_diagram(
        &network,
        &DiagramTarget::Corridor("tel-main".into()),
        date("20250505"),
        time("05:00:00"),
        time("10:00:00"),
        &config,
        &seed(),
    )
    .unwrap();

    assert_eq!(document.corridor.label, "TEL with the branch");
    assert_eq!(document.corridor.panels.len(), 2);
    assert_eq!(document.corridor.panels[1].label, "For Branch Beta");

    // The branch run runs down the main axis to Woodlands and then
    // along the branch.
    let branch = document
        .runs
        .iter()
        .find(|r| r.source_trip_id == "TE_B1")
        .expect("the branch run is drawn");
    let nodes: Vec<usize> = branch.calls.iter().map(|c| c.node).collect();
    assert_eq!(nodes, vec![0, 1, 4, 5]);
    assert!(document
        .metadata
        .diagnostics
        .iter()
        .all(|d| d.code != "run-off-corridor"));
}

#[test]
fn a_run_that_does_not_fit_the_axis_is_reported_not_bent() {
    let network = network();
    // A pattern-scoped corridor holds only the main TEL pattern, so
    // the branch run cannot be drawn on it.
    let pattern = network
        .patterns_for_line(network.line_by_route_id("TE").unwrap())
        .enumerate()
        .find(|(_, p)| p.stations.len() == 4 && p.direction == Some(0))
        .map(|(index, _)| mrt_gtfs::PatternId(index))
        .unwrap();
    let document = build_diagram(
        &network,
        &DiagramTarget::Pattern(pattern),
        date("20250505"),
        time("05:00:00"),
        time("10:00:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    assert!(document.runs.iter().all(|r| r.source_trip_id != "TE_B1"));
    assert_eq!(document.corridor.panels.len(), 1);
}

#[test]
fn a_headway_block_becomes_a_band_with_two_envelope_paths() {
    let network = network();
    let document = build_diagram(
        &network,
        &DiagramTarget::Line(network.line_by_route_id("BP").unwrap()),
        date("20250505"),
        time("05:00:00"),
        time("07:00:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    assert!(document.runs.is_empty(), "a band must not draw solid runs");
    assert_eq!(document.frequency_bands.len(), 1);
    let band = &document.frequency_bands[0];
    assert_eq!(band.headway_minutes, 10);
    assert!(band.label.contains("approximately"));
    assert!(!band.first_path.is_empty());
    assert!(!band.last_path.is_empty());
    // The last representative run starts one headway before the end.
    assert_eq!(band.last_path[0].time.to_string(), "05:50:00");
    assert!(document.legend.iter().any(|l| l.key == "approximate"));
}

#[test]
fn distance_spacing_falls_back_when_the_feed_cannot_support_it() {
    let network = network();
    let mut config = PublicationConfig::default();
    config.diagram.station_spacing = StationSpacing::Distance;
    let document = build_diagram(
        &network,
        &tel(&network),
        date("20250505"),
        time("05:00:00"),
        time("10:00:00"),
        &config,
        &seed(),
    )
    .unwrap();
    // The fixture carries positions, so distance spacing works and the
    // rows are no longer evenly spaced.
    assert_eq!(document.station_spacing, StationSpacing::Distance);
    let main = &document.corridor.panels[0];
    let ys: Vec<f64> = document.corridor.nodes[main.first_node..=main.last_node]
        .iter()
        .map(|n| n.y)
        .collect();
    assert_eq!(ys.first(), Some(&0.0));
    assert_eq!(ys.last(), Some(&102.0));
    assert!(ys.windows(2).all(|w| w[1] > w[0]));
    assert!((ys[1] - 34.0).abs() > 1.0, "spacing stayed uniform: {ys:?}");
}

#[test]
fn labels_use_the_public_train_name_and_never_the_internal_identifier() {
    let network = network();
    let document = build_diagram(
        &network,
        &tel(&network),
        date("20250505"),
        time("05:00:00"),
        time("10:00:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap();

    let labelled = document
        .runs
        .iter()
        .find(|r| r.source_trip_id == "TE_T1")
        .unwrap();
    assert_eq!(labelled.label.as_deref(), Some("T101"));
    assert!(labelled.label_placement.is_some());

    // The headway template has no public name, so it gets no label.
    let unnamed = document
        .runs
        .iter()
        .find(|r| r.source_trip_id == "TE_F1")
        .unwrap();
    assert_eq!(unnamed.label, None);
    assert!(document
        .runs
        .iter()
        .all(|r| r.label.as_deref() != Some(r.source_trip_id.as_str())));
}

#[test]
fn an_empty_window_is_a_configuration_error() {
    let network = network();
    let error = build_diagram(
        &network,
        &tel(&network),
        date("20250505"),
        time("08:00:00"),
        time("08:00:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap_err();
    assert_eq!(error.exit_code(), 2);
}

#[test]
fn the_reject_policy_stops_the_diagram_with_exit_code_six() {
    let network = network();
    let config = PublicationConfig {
        frequency_policy: FrequencyPolicy::RejectNonExact,
        ..Default::default()
    };
    let error = build_diagram(
        &network,
        &DiagramTarget::Line(network.line_by_route_id("BP").unwrap()),
        date("20250505"),
        time("05:00:00"),
        time("07:00:00"),
        &config,
        &seed(),
    )
    .unwrap_err();
    assert_eq!(error.exit_code(), 6);
}

#[test]
fn a_missing_corridor_is_an_unresolved_target() {
    let network = network();
    let error = build_diagram(
        &network,
        &DiagramTarget::Corridor("nope".into()),
        date("20250505"),
        time("05:00:00"),
        time("07:00:00"),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap_err();
    assert_eq!(error.exit_code(), 5);
}

#[test]
fn missing_times_that_stay_missing_do_not_break_the_path() {
    let network = network();
    let config = PublicationConfig {
        missing_time_policy: MissingTimePolicy::None,
        ..Default::default()
    };
    let document = build_diagram(
        &network,
        &tel(&network),
        date("20250505"),
        time("05:00:00"),
        time("10:00:00"),
        &config,
        &seed(),
    )
    .unwrap();

    let gapped = document
        .runs
        .iter()
        .find(|r| r.source_trip_id == "TE_M1")
        .unwrap();
    // Only the two calls that carry times remain on the polyline.
    assert_eq!(gapped.points.len(), 3);
    assert!(document
        .metadata
        .diagnostics
        .iter()
        .any(|d| d.code == "time-missing"));
}
