//! Snapshot tests.
//!
//! The snapshots pin the exact bytes of a view model and of a drawing,
//! so a change in the projection or in the renderer shows up as a
//! reviewable diff instead of a silent shift in a printed timetable.
//!
//! The documents carry no clock reading, so the snapshots are stable.
//! To accept an intended change, run
//!
//! ```sh
//! UPDATE_SNAPSHOTS=1 cargo test -p mrt-publication-html --test snapshot_tests
//! ```
//!
//! and review the diff.

use std::path::{Path, PathBuf};

use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork};
use mrt_publication::{
    build_diagram, build_timetable, DiagramTarget, DocumentSeed, PublicationConfig,
};
use mrt_publication_html::render_diagram_svg;

fn network() -> RailNetwork {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mrt-gtfs/tests/fixtures/mini");
    RailNetwork::from_feed(&GtfsFeed::from_dir(dir).unwrap()).unwrap()
}

/// A seed with fixed values, so nothing in a snapshot depends on the
/// machine that produced it.
fn seed() -> DocumentSeed {
    DocumentSeed {
        generator_version: "snapshot".into(),
        feed_sha256: "0".repeat(64),
        feed_timestamp: Some("2026-08-10T00:00:00+08:00".into()),
        timezone: "Asia/Singapore".into(),
        generated_from_cache: false,
        configuration_sha256: "0".repeat(64),
    }
}

fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name)
}

/// Compare `actual` with the stored snapshot, or write it.
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
        let (line, before, after) = first_difference(&expected, actual);
        panic!(
            "the output no longer matches {}\n  first difference at line {line}\n  \
             expected: {before}\n  actual:   {after}\n  \
             run with UPDATE_SNAPSHOTS=1 to accept the change",
            path.display()
        );
    }
}

/// Find the first line that differs, for a readable failure.
fn first_difference(expected: &str, actual: &str) -> (usize, String, String) {
    for (index, (a, b)) in expected.lines().zip(actual.lines()).enumerate() {
        if a != b {
            return (index + 1, a.to_string(), b.to_string());
        }
    }
    let line = expected.lines().count().min(actual.lines().count()) + 1;
    (
        line,
        expected
            .lines()
            .nth(line - 1)
            .unwrap_or("<end>")
            .to_string(),
        actual.lines().nth(line - 1).unwrap_or("<end>").to_string(),
    )
}

/// Normalize an SVG for comparison: one element per line.
///
/// The drawing itself is already deterministic; splitting it makes a
/// failure point at one shape rather than at one very long line.
fn normalize_svg(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    for line in svg.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out
}

#[test]
fn the_timetable_view_model_is_stable() {
    let network = network();
    let config = PublicationConfig::default();
    let document = build_timetable(
        &network,
        network.station_by_code("TE1").unwrap(),
        "20250505".parse().unwrap(),
        None,
        &config,
        &seed(),
    )
    .unwrap();
    let mut json = serde_json::to_string_pretty(&document).unwrap();
    json.push('\n');
    assert_snapshot("timetable-te1.json", &json);
}

#[test]
fn the_diagram_view_model_is_stable() {
    let network = network();
    let config = PublicationConfig::default();
    let document = build_diagram(
        &network,
        &DiagramTarget::Line(network.line_by_route_id("TE").unwrap()),
        "20250505".parse().unwrap(),
        GtfsTime::from_hms(6, 0, 0),
        GtfsTime::from_hms(7, 0, 0),
        &config,
        &seed(),
    )
    .unwrap();
    let mut json = serde_json::to_string_pretty(&document).unwrap();
    json.push('\n');
    assert_snapshot("diagram-tel.json", &json);
}

#[test]
fn the_diagram_drawing_is_stable() {
    let network = network();
    let config = PublicationConfig::default();
    let document = build_diagram(
        &network,
        &DiagramTarget::Line(network.line_by_route_id("TE").unwrap()),
        "20250505".parse().unwrap(),
        GtfsTime::from_hms(6, 0, 0),
        GtfsTime::from_hms(7, 0, 0),
        &config,
        &seed(),
    )
    .unwrap();
    assert_snapshot(
        "diagram-tel.svg",
        &normalize_svg(&render_diagram_svg(&document, &config)),
    );
}

#[test]
fn every_snapshot_is_committed() {
    // A snapshot that only exists on the machine that wrote it is no
    // snapshot at all.
    let directory = snapshot_path("");
    let names: Vec<String> = std::fs::read_dir(&directory)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", directory.display()))
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    for expected in ["timetable-te1.json", "diagram-tel.json", "diagram-tel.svg"] {
        assert!(
            names.iter().any(|name| name == expected),
            "the snapshot {expected} is missing"
        );
        assert!(
            Path::new(&snapshot_path(expected))
                .metadata()
                .unwrap()
                .len()
                > 0
        );
    }
}
