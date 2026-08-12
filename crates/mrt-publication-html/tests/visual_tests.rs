//! Visual regression fixtures.
//!
//! Two pages stand in for the two reference designs:
//!
//! 1. `examples/timetable-woodlands-north.html` — a Japanese-style
//!    station departure timetable.
//! 2. `examples/diagram-tel.html` and `.svg` — a multi-direction
//!    string diagram.
//!
//! The tests below check the *visual grammar* that the references
//! define: the strong direction header, the dark hour column, the
//! alternating rows, the large minute numerals with small destination
//! annotations, the legend; and for the diagram, the layered grid, the
//! station axis, paths that slope both ways, the dwell segments, and
//! the run labels.
//!
//! A pixel comparison lives in `scripts/visual-regression.sh`, which
//! needs a browser and therefore stays out of `cargo test`. These
//! tests and the SVG snapshot are the deterministic ones.
//!
//! Running the tests refreshes the committed example pages, so the
//! files in `examples/` never drift from the renderer.

use std::path::PathBuf;

use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork};
use mrt_publication::{
    build_diagram, build_timetable, DiagramTarget, DocumentSeed, PublicationConfig,
};
use mrt_publication_html::{render_diagram, render_diagram_svg, render_timetable};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn network() -> RailNetwork {
    let dir = repository_root().join("crates/mrt-gtfs/tests/fixtures/mini");
    RailNetwork::from_feed(&GtfsFeed::from_dir(dir).unwrap()).unwrap()
}

fn seed() -> DocumentSeed {
    DocumentSeed {
        generator_version: "example".into(),
        feed_sha256: "0".repeat(64),
        feed_timestamp: Some("2026-08-10T00:00:00+08:00".into()),
        timezone: "Asia/Singapore".into(),
        generated_from_cache: false,
        configuration_sha256: "0".repeat(64),
    }
}

/// Write an example page and return it.
fn publish(name: &str, body: String) -> String {
    let path = repository_root().join("examples").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Only write when the content changed, so a read-only checkout and
    // an unchanged run both stay quiet.
    if std::fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
        let _ = std::fs::write(&path, &body);
    }
    body
}

fn timetable_page() -> String {
    let network = network();
    let config = PublicationConfig::default();
    let document = build_timetable(
        &network,
        network.station_by_code("TE2").unwrap(),
        "20250505".parse().unwrap(),
        network.line_by_route_id("TE"),
        &config,
        &seed(),
    )
    .unwrap();
    publish(
        "timetable-woodlands.html",
        render_timetable(&document, &config),
    )
}

/// Publish both diagram pages and return them.
fn diagram_pages() -> (String, String) {
    let document = diagram_document();
    let config = PublicationConfig::default();
    (
        publish("diagram-tel.html", render_diagram(&document, &config)),
        publish("diagram-tel.svg", render_diagram_svg(&document, &config)),
    )
}

fn diagram_document() -> mrt_publication::DiagramDocument {
    let network = network();
    build_diagram(
        &network,
        &DiagramTarget::Line(network.line_by_route_id("TE").unwrap()),
        "20250505".parse().unwrap(),
        GtfsTime::from_hms(5, 0, 0),
        GtfsTime::from_hms(10, 0, 0),
        &PublicationConfig::default(),
        &seed(),
    )
    .unwrap()
}

// ----------------------------------------------------------------------
// Reference 1: the station departure timetable
// ----------------------------------------------------------------------

#[test]
fn the_timetable_page_carries_the_reference_visual_grammar() {
    let html = timetable_page();

    // A strong direction header on the line colour, with the line name
    // and the platform beside it.
    assert!(html.contains("class=\"panel-head\""));
    assert!(html.contains("class=\"line-name\""));
    assert!(html.contains("class=\"direction\""));
    assert!(html.contains("class=\"platform\""));
    assert!(html.contains("--line-color: #9D5B25"));

    // A dark hour column, alternating rows, and large minute numerals
    // with a small destination annotation beside each one.
    assert!(html.contains(".hour-cell {"));
    assert!(html.contains("background: var(--hour-bg)"));
    assert!(html.contains(".hour-row:nth-child(even)"));
    assert!(html.contains("class=\"min\""));
    assert!(html.contains("class=\"dest\""));

    // A legend and a quiet colophon with the feed fingerprint.
    assert!(html.contains("class=\"legend\""));
    assert!(html.contains("class=\"colophon\""));
    assert!(html.contains("Feed fingerprint"));
}

#[test]
fn the_timetable_page_prints_on_a4_and_stays_readable_without_style() {
    let html = timetable_page();
    assert!(html.contains("size: A4 portrait"));
    assert!(html.contains("print-color-adjust: exact"));
    // A row must not break across two pages.
    assert!(html.contains("page-break-inside: avoid"));
    // Removing the stylesheet leaves a table with an hour heading per
    // row and an ordered list of departures.
    assert!(html.contains("<th scope=\"row\" class=\"hour-cell\">06</th>"));
    assert!(html.contains("<ol>"));
}

#[test]
fn the_timetable_page_shows_the_service_day_in_order() {
    let html = timetable_page();
    let hours: Vec<&str> = html
        .match_indices("class=\"hour-cell\">")
        .map(|(index, marker)| &html[index + marker.len()..index + marker.len() + 2])
        .collect();
    // Woodlands has a panel per platform, and each panel carries the
    // whole service day in two columns.
    assert_eq!(hours.len(), 48);
    let expected: Vec<String> = (4..28).map(|hour| format!("{:02}", hour % 24)).collect();
    assert_eq!(hours[..24], expected[..]);
    assert_eq!(hours[24..], expected[..]);
    // The service day, not the clock: the small hours come last.
    assert_eq!(hours[0], "04");
    assert_eq!(hours[23], "03");
}

// ----------------------------------------------------------------------
// Reference 2: the multi-direction string diagram
// ----------------------------------------------------------------------

#[test]
fn the_diagram_page_carries_the_reference_visual_grammar() {
    let (html, svg) = diagram_pages();

    // Three grid levels plus the day boundary style.
    for class in ["grid-minor", "grid-medium", "grid-major", "grid-day"] {
        assert!(svg.contains(class), "the drawing has no {class}");
    }
    // Time labels above and below the plot.
    let labels = svg.matches("class=\"axis-label major\"").count();
    assert!(labels >= 12, "only {labels} time labels");

    // A station axis with names and codes at the left.
    assert!(svg.contains("class=\"station-name\""));
    assert!(svg.contains("class=\"station-code\""));
    assert!(svg.contains(">Woodlands North<"));
    assert!(svg.contains(">TE1<"));

    // Labelled train paths with stop markers.
    assert!(svg.contains("class=\"run-label\""));
    assert!(svg.contains("class=\"stop-dot\""));
    assert!(svg.contains(">T101<"));

    // Filters and print controls on the page around the drawing.
    assert!(html.contains("class=\"filters"));
    assert!(html.contains("id=\"download-svg\""));
    assert!(html.contains("size: A3 landscape"));
}

#[test]
fn the_diagram_draws_both_directions_and_the_dwell_segments() {
    let document = diagram_document();

    let down = document
        .runs
        .iter()
        .filter(|r| r.axis_direction == mrt_publication::AxisDirection::Down)
        .count();
    let up = document
        .runs
        .iter()
        .filter(|r| r.axis_direction == mrt_publication::AxisDirection::Up)
        .count();
    assert!(down > 0 && up > 0, "{down} down and {up} up");

    // A dwell segment is two points at the same height and different
    // times.
    let dwells = document
        .runs
        .iter()
        .flat_map(|run| run.points.windows(2))
        .filter(|pair| (pair[0].y - pair[1].y).abs() < f64::EPSILON && pair[0].time != pair[1].time)
        .count();
    assert!(dwells > 0, "the drawing carries no dwell segment");
}

#[test]
fn the_example_pages_are_committed_and_current() {
    // Regenerate here too, so the test does not depend on the order in
    // which the other tests ran.
    timetable_page();
    diagram_pages();
    for name in [
        "timetable-woodlands.html",
        "diagram-tel.html",
        "diagram-tel.svg",
    ] {
        let path = repository_root().join("examples").join(name);
        assert!(path.exists(), "{} is missing", path.display());
        assert!(path.metadata().unwrap().len() > 1000);
    }
}
