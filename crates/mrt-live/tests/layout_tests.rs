//! Tests for the schematic layout and its binding.
//!
//! The acceptance test reads the committed layout
//! `config/layout-mini.geojson` — an OpenFantasyMap GeoJSON export of
//! the miniature fixture network — and binds it against the network
//! that `mrt-gtfs/tests/fixtures/mini` builds. Every layout station
//! must match a network station, and every network station must be
//! drawn, or the run names what it could not match.
//!
//! The other tests hand the reader a broken layout and check that each
//! failure is a diagnostic rather than a panic or a silent drop. They
//! build those layouts in the test, so the committed file stays a
//! faithful export.
//!
//! No test touches the network, and no test carries a table of station
//! codes: the codes come from the layout file and from the feed.

use std::path::PathBuf;

use mrt_gtfs::{GtfsFeed, RailNetwork};
use mrt_live::{BoundLayout, Layout, UnmatchedReason};
use serde_json::{json, Value};

/// The miniature fixture network.
fn network() -> RailNetwork {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mrt-gtfs/tests/fixtures/mini");
    RailNetwork::from_feed(&GtfsFeed::from_dir(dir).unwrap()).unwrap()
}

/// The path of the committed layout.
fn layout_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/layout-mini.geojson")
}

/// The text of the committed layout.
fn layout_text() -> String {
    let path = layout_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The committed layout, parsed.
fn sample() -> Layout {
    Layout::from_geojson_str(&layout_text()).expect("the committed layout is valid JSON")
}

/// Report whether a layout carries a diagnostic with the given code.
fn layout_has(layout: &Layout, code: &str) -> bool {
    layout.diagnostics.iter().any(|d| d.code == code)
}

/// Report whether a binding carries a diagnostic with the given code.
fn bound_has(bound: &BoundLayout, code: &str) -> bool {
    bound.diagnostics.iter().any(|d| d.code == code)
}

/// A minimal layout: one line and the given station features.
fn layout_with(stations: Vec<Value>) -> Value {
    let mut features = vec![json!({
        "type": "Feature",
        "geometry": {
            "type": "LineString",
            "coordinates": [[103.74, 1.33], [103.74, 1.41]],
        },
        "properties": {
            "ofm": "line",
            "id": "line-test",
            "name": "Test Line",
            "mode": "Metro",
            "minRadius": 150,
            "color": "#d42e12",
            "visible": true,
            "nodes": [[1.33, 103.74], [1.41, 103.74]],
            "segments": [{"profile": "manual", "guide": []}],
            "branchOf": null,
        },
    })];
    features.extend(stations);
    json!({ "type": "FeatureCollection", "features": features })
}

/// One station feature on the line that [`layout_with`] draws.
fn station(id: &str, name: &str, code: Option<&str>, t: f64) -> Value {
    let mut properties = json!({
        "ofm": "station",
        "id": id,
        "lineId": "line-test",
        "name": name,
        "t": t,
    });
    if let Some(code) = code {
        properties["code"] = json!(code);
    }
    json!({
        "type": "Feature",
        "geometry": { "type": "Point", "coordinates": [103.74, 1.33] },
        "properties": properties,
    })
}

// ----------------------------------------------------------------------
// The committed layout
// ----------------------------------------------------------------------

#[test]
fn the_committed_layout_parses() {
    let layout = sample();
    assert_eq!(layout.unknown_features, 0);
    assert_eq!(layout.diagnostics, Vec::new());
    assert_eq!(layout.lines.len(), 6);
    assert_eq!(layout.stations.len(), 17);

    // The export carries the whole editor model, so every line has a
    // drawn polyline, a name, and a colour.
    for line in &layout.lines {
        assert!(line.points.len() >= 2, "{} has no polyline", line.id);
        assert!(line.name.is_some(), "{} has no name", line.id);
        assert!(line.color.is_some(), "{} has no colour", line.id);
        assert!(line.visible);
    }
    // Every station names a line the layout carries, a code, and a
    // position along that line.
    for station in &layout.stations {
        assert!(
            layout.line(&station.line).is_some(),
            "{} names the unknown line {}",
            station.id,
            station.line
        );
        assert!(station.code.is_some(), "{} has no code", station.id);
        let t = station
            .t
            .unwrap_or_else(|| panic!("{} has no t", station.id));
        assert!((0.0..=1.0).contains(&t), "{} has t = {t}", station.id);
    }
}

#[test]
fn the_committed_layout_carries_a_branch_and_a_station_area() {
    let layout = sample();

    // A branch names the line it leaves and the node it leaves from.
    let branch = layout
        .lines
        .iter()
        .find(|line| line.branch_of.is_some())
        .expect("the layout draws a branch");
    let link = branch.branch_of.as_ref().unwrap();
    assert!(layout.line(&link.line).is_some());
    assert_eq!(link.node_index, Some(1));

    // An interchange drawn as an area keeps its ring, and its anchor
    // is the centre of it.
    let area = layout
        .stations
        .iter()
        .find(|station| station.area.is_some())
        .expect("the layout draws a station area");
    let ring = area.area.as_ref().unwrap();
    assert_eq!(ring.len(), 4, "a rectangle without its closing position");
    let x = ring.iter().map(|p| p.x).sum::<f64>() / 4.0;
    assert!((x - area.point.x).abs() < 1e-9);
}

/// The acceptance test of phase 2: the committed layout and the fixture
/// network agree in both directions.
#[test]
fn the_committed_layout_matches_the_fixture_network() {
    let network = network();
    let layout = sample();
    let stations = layout.stations.len();
    let bound = layout.bind(&network);

    assert_eq!(bound.unmatched.len(), 0, "{:?}", bound.unmatched);
    assert_eq!(bound.uncovered.len(), 0, "{:?}", bound.uncovered);
    assert!(bound.is_complete());
    assert_eq!(bound.stations.len(), stations);
    assert_eq!(bound.diagnostics, Vec::new());

    // Every station of the network is drawn, and the layout name and
    // the network name agree.
    let drawn: Vec<usize> = bound.stations.iter().map(|s| s.station.0).collect();
    for index in 0..network.stations().len() {
        assert!(drawn.contains(&index), "{index} is drawn nowhere");
    }
    for station in &bound.stations {
        let layout_station = bound
            .layout
            .stations
            .iter()
            .find(|s| s.id == station.layout_station)
            .unwrap();
        assert_eq!(layout_station.name.as_deref(), Some(station.name.as_str()));
    }
}

#[test]
fn an_interchange_binds_once_per_line() {
    let network = network();
    let bound = sample().bind(&network);

    // Jurong East is drawn on two layout lines, under the code each
    // line carries, and both bind to the one network station.
    let jurong: Vec<_> = bound
        .stations
        .iter()
        .filter(|station| station.name == "Jurong East")
        .collect();
    assert_eq!(jurong.len(), 2);
    assert_eq!(jurong[0].station, jurong[1].station);
    assert_ne!(jurong[0].layout_line, jurong[1].layout_line);
    assert_ne!(jurong[0].code, jurong[1].code);
}

#[test]
fn the_binding_of_the_committed_layout_is_deterministic() {
    let network = network();
    let text = layout_text();
    let first =
        serde_json::to_string_pretty(&Layout::from_geojson_str(&text).unwrap().bind(&network))
            .unwrap();
    let second =
        serde_json::to_string_pretty(&Layout::from_geojson_str(&text).unwrap().bind(&network))
            .unwrap();
    assert_eq!(first, second);
}

#[test]
fn the_layout_file_is_committed() {
    // A layout that only exists on the machine that drew it is no
    // layout at all.
    let path = layout_path();
    assert!(path.metadata().is_ok_and(|m| m.len() > 0), "{path:?}");
}

// ----------------------------------------------------------------------
// What the reader cannot use
// ----------------------------------------------------------------------

#[test]
fn a_station_with_an_unknown_code_is_reported() {
    let network = network();
    let value = layout_with(vec![station("station-zz9", "Nowhere", Some("ZZ9"), 0.5)]);
    let bound = Layout::from_geojson(&value).bind(&network);

    assert_eq!(bound.stations.len(), 0);
    assert_eq!(bound.unmatched.len(), 1);
    assert_eq!(bound.unmatched[0].layout_station, "station-zz9");
    assert_eq!(bound.unmatched[0].code.as_deref(), Some("ZZ9"));
    assert_eq!(bound.unmatched[0].reason, UnmatchedReason::UnknownCode);
    assert!(bound_has(&bound, "layout-station-unmatched"));
    assert!(bound
        .diagnostics
        .iter()
        .any(|d| d.subject.as_deref() == Some("station-zz9")));
}

#[test]
fn a_network_station_the_layout_misses_is_reported() {
    let network = network();
    // The layout draws Jurong East and nothing else.
    let value = layout_with(vec![station(
        "station-ns1",
        "Jurong East",
        Some("NS1"),
        0.0,
    )]);
    let bound = Layout::from_geojson(&value).bind(&network);

    assert_eq!(bound.stations.len(), 1);
    assert_eq!(bound.uncovered.len(), network.stations().len() - 1);
    assert!(!bound.is_complete());
    assert!(bound_has(&bound, "network-station-uncovered"));

    let marina = bound
        .uncovered
        .iter()
        .find(|station| station.name == "Marina Bay")
        .expect("Marina Bay is uncovered");
    assert_eq!(marina.codes, vec!["NS27".to_string()]);
    // The report is in station order, so it does not move between runs.
    let order: Vec<usize> = bound.uncovered.iter().map(|s| s.station.0).collect();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(order, sorted);
}

#[test]
fn a_station_without_a_code_is_reported() {
    let network = network();
    let value = layout_with(vec![station("station-unnamed", "Somewhere", None, 0.5)]);
    let layout = Layout::from_geojson(&value);
    assert!(layout_has(&layout, "layout-station-without-code"));

    let bound = layout.bind(&network);
    assert_eq!(bound.stations.len(), 0);
    assert_eq!(bound.unmatched.len(), 1);
    assert_eq!(bound.unmatched[0].reason, UnmatchedReason::NoCode);
    assert_eq!(bound.unmatched[0].code, None);
    assert!(bound_has(&bound, "layout-station-without-code"));
}

#[test]
fn a_foreign_feature_is_counted_and_reported() {
    let mut value = layout_with(vec![station(
        "station-ns1",
        "Jurong East",
        Some("NS1"),
        0.0,
    )]);
    // A feature from another tool: valid GeoJSON, no OpenFantasyMap
    // kind.
    value["features"].as_array_mut().unwrap().push(json!({
        "type": "Feature",
        "geometry": { "type": "Point", "coordinates": [103.85, 1.29] },
        "properties": { "amenity": "cafe", "name": "Somewhere else" },
    }));
    // And a feature of a kind this reader does not know.
    value["features"].as_array_mut().unwrap().push(json!({
        "type": "Feature",
        "geometry": { "type": "Point", "coordinates": [103.86, 1.30] },
        "properties": { "ofm": "depot", "id": "depot-1" },
    }));

    let layout = Layout::from_geojson(&value);
    assert_eq!(layout.unknown_features, 2);
    assert_eq!(layout.lines.len(), 1);
    assert_eq!(layout.stations.len(), 1);
    assert!(layout_has(&layout, "layout-unknown-feature"));
    assert!(layout
        .diagnostics
        .iter()
        .any(|d| d.code == "layout-unknown-feature" && d.message.contains('2')));
}

#[test]
fn two_stations_with_one_code_are_reported() {
    let network = network();
    let value = layout_with(vec![
        station("station-ns1", "Jurong East", Some("NS1"), 0.0),
        station("station-ns1-again", "Jurong East", Some("ns-1"), 0.4),
    ]);
    let bound = Layout::from_geojson(&value).bind(&network);

    // Both stay bound — the map draws what the layout says — and the
    // duplicate is reported.
    assert_eq!(bound.stations.len(), 2);
    assert!(bound_has(&bound, "layout-duplicate-code"));
}

#[test]
fn a_station_on_an_unknown_line_is_reported() {
    let mut value = layout_with(vec![station(
        "station-ns1",
        "Jurong East",
        Some("NS1"),
        0.0,
    )]);
    value["features"][1]["properties"]["lineId"] = json!("line-missing");
    let layout = Layout::from_geojson(&value);

    assert_eq!(layout.stations.len(), 1);
    assert!(layout_has(&layout, "layout-station-without-line"));
}

#[test]
fn malformed_geometry_is_a_diagnostic() {
    let value = json!({
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "geometry": { "type": "LineString", "coordinates": [[103.74, 1.33]] },
                "properties": { "ofm": "line", "id": "line-short", "name": "Short" },
            },
            {
                "type": "Feature",
                "geometry": { "type": "Point", "coordinates": ["east", "north"] },
                "properties": {
                    "ofm": "station",
                    "id": "station-broken",
                    "lineId": "line-short",
                    "name": "Broken",
                    "code": "NS1",
                    "t": 0.5,
                },
            },
            {
                "type": "Feature",
                "geometry": { "type": "Polygon", "coordinates": [] },
                "properties": {
                    "ofm": "station-area",
                    "id": "area-broken",
                    "lineId": "line-short",
                    "name": "Broken area",
                    "code": "NS4",
                    "t": 0.5,
                },
            },
        ],
    });
    let layout = Layout::from_geojson(&value);

    assert_eq!(layout.lines.len(), 0);
    assert_eq!(layout.stations.len(), 0);
    assert!(layout_has(&layout, "layout-line-without-geometry"));
    assert!(layout_has(&layout, "layout-station-without-position"));
    assert_eq!(
        layout
            .diagnostics
            .iter()
            .filter(|d| d.code == "layout-station-without-position")
            .count(),
        2
    );
}

#[test]
fn a_value_that_is_not_a_feature_collection_is_a_diagnostic() {
    let layout = Layout::from_geojson(&json!({ "type": "Feature" }));
    assert_eq!(layout.lines.len(), 0);
    assert!(layout_has(&layout, "layout-not-a-feature-collection"));

    let layout = Layout::from_geojson(&json!({ "type": "FeatureCollection" }));
    assert!(layout_has(&layout, "layout-without-features"));
}

#[test]
fn text_that_is_not_json_is_an_error() {
    assert!(Layout::from_geojson_str("{ not json").is_err());
}

#[test]
fn an_empty_layout_reports_every_network_station() {
    let network = network();
    let layout = Layout::from_geojson(&json!({
        "type": "FeatureCollection",
        "features": [],
    }));
    let bound = layout.bind(&network);

    assert_eq!(bound.stations.len(), 0);
    assert_eq!(bound.uncovered.len(), network.stations().len());
    assert!(!bound.is_complete());
}
