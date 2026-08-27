//! Tests for the map page and the snapshot document behind it.
//!
//! Everything here is deterministic and offline: the network comes
//! from the miniature fixture feed in `mrt-gtfs/tests/fixtures/mini`,
//! the geometry from the committed layout
//! `config/layout-mini.geojson`, the clock and the service date are
//! fixed, and the realtime layer is synthetic. No test touches the
//! network or reads a clock.

use std::path::PathBuf;

use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork, ServiceDate};
use mrt_gtfs_rt::{RailRtFeed, StopTimeEvent, StopTimeUpdate, TripUpdate};
use mrt_live::{Layout, NetworkSnapshot, NetworkSnapshotBuilder};
use mrt_map_web::{
    map_geometry, map_snapshot_json, render_map_page, render_network_svg, MapPageInput,
};

/// The POSIX time that stands for "now" in the tests. It only measures
/// the age of the realtime feed; no position depends on it.
const NOW_UNIX: u64 = 1_746_400_000;

/// The miniature fixture network.
fn network() -> RailNetwork {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mrt-gtfs/tests/fixtures/mini");
    RailNetwork::from_feed(&GtfsFeed::from_dir(dir).unwrap()).unwrap()
}

/// The committed layout of the fixture network, bound.
fn bound_layout(network: &RailNetwork) -> mrt_live::BoundLayout {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/layout-mini.geojson");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    Layout::from_geojson_str(&text)
        .expect("the committed layout is valid JSON")
        .bind(network)
}

/// A Monday, so the `WKDAY` service of the fixture runs.
fn date() -> ServiceDate {
    "20250505".parse().unwrap()
}

/// The schedule-only snapshot that pins the static layer: no realtime,
/// a fixed date, a fixed clock.
fn schedule_snapshot(network: &RailNetwork) -> NetworkSnapshot {
    NetworkSnapshotBuilder::new(network).build(date(), GtfsTime::from_hms(6, 5, 0))
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

/// A synthetic realtime layer: one run late with a per-stop
/// prediction, one early and standing at a station.
fn synthetic_realtime() -> RailRtFeed {
    RailRtFeed {
        feed_timestamp: Some(NOW_UNIX - 30),
        trip_updates: vec![
            TripUpdate {
                trip_id: Some("NS_T1".to_string()),
                delay_secs: Some(60),
                stop_updates: vec![stop_delay("CCK_NS", 150)],
                ..Default::default()
            },
            TripUpdate {
                trip_id: Some("EW_T1".to_string()),
                stop_updates: vec![stop_delay("JUR_EW", -20)],
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

/// Render the whole page from the fixture, schedule-only.
fn schedule_page(snapshot_url: &str) -> String {
    let network = network();
    let snapshot = schedule_snapshot(&network);
    let layout = bound_layout(&network);
    render_map_page(&MapPageInput {
        snapshot: &snapshot,
        layout: &layout,
        snapshot_url,
        deployment: "test build",
    })
}

// ----------------------------------------------------------------------
// The committed SVG snapshot
// ----------------------------------------------------------------------

fn svg_snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/map-mini.svg")
}

/// Compare the rendered SVG with the stored snapshot, or write it.
///
/// To accept an intended change, run
///
/// ```sh
/// UPDATE_SNAPSHOTS=1 cargo test -p mrt-map-web --test map_page_tests
/// ```
///
/// and review the diff.
#[test]
fn the_network_svg_is_stable() {
    let network = network();
    let snapshot = schedule_snapshot(&network);
    let layout = bound_layout(&network);
    let geometry = map_geometry(&snapshot, &layout);
    let actual = render_network_svg(&snapshot, &geometry);

    let path = svg_snapshot_path();
    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
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
            "the SVG no longer matches {}\n  first difference at line {line}\n  \
             run with UPDATE_SNAPSHOTS=1 to accept the change",
            path.display()
        );
    }
}

#[test]
fn the_svg_snapshot_file_is_committed() {
    // A snapshot that only exists on the machine that wrote it is no
    // snapshot at all.
    let path = svg_snapshot_path();
    assert!(path.metadata().is_ok_and(|m| m.len() > 0), "{path:?}");
}

// ----------------------------------------------------------------------
// The snapshot document
// ----------------------------------------------------------------------

#[test]
fn the_snapshot_document_places_a_known_train() {
    // A known synthetic realtime layer puts NS_T1 on a known edge with
    // a known progress and provenance, and the transported document
    // carries exactly that.
    let network = network();
    let realtime = synthetic_realtime();
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));
    let body = map_snapshot_json(&snapshot, true, NOW_UNIX as i64);

    assert_eq!(body["live"], true);
    assert_eq!(body["generated"], NOW_UNIX);
    assert_eq!(body["snapshot"]["freshness"]["state"], "live");

    let trains = body["snapshot"]["trains"].as_array().unwrap();
    let train = trains
        .iter()
        .find(|t| t["source_trip_id"] == "NS_T1")
        .expect("NS_T1 is running at 06:05");
    // Jurong East (station 0) to Choa Chu Kang (station 1): departed
    // 06:01 shifted by +150 s, arriving 06:12 shifted by +60 s, so at
    // 06:05:00 the run is 210/660 of the way along the edge.
    assert_eq!(train["location"]["kind"], "on-edge");
    assert_eq!(train["location"]["from"], 0);
    assert_eq!(train["location"]["to"], 1);
    assert_eq!(train["quality"], "interpolated-realtime");
    assert_eq!(train["delay_secs"], 60);
    assert_eq!(train["edge_secs"], 660);
    let progress = train["progress"].as_f64().unwrap();
    assert!(
        (progress - 210.0 / 660.0).abs() < 1e-9,
        "progress was {progress}"
    );

    // The early run stands at its station, which is the strongest
    // claim the data supports: progress 0 and no edge span.
    let train = trains
        .iter()
        .find(|t| t["source_trip_id"] == "EW_T1")
        .expect("EW_T1 is running at 06:05");
    assert_eq!(train["location"]["kind"], "at-station");
    assert_eq!(train["quality"], "at-station");
    assert_eq!(train["progress"], 0.0);
    assert_eq!(train["edge_secs"], serde_json::Value::Null);
}

// ----------------------------------------------------------------------
// The page
// ----------------------------------------------------------------------

#[test]
fn the_page_shows_the_network_without_javascript() {
    let page = schedule_page("/api/map-snapshot");

    // The static floor: ribbons, station discs, and names are in the
    // document itself, so the page is complete with scripts off.
    assert!(page.contains("<path class=\"casing\""));
    assert!(page.contains("<path class=\"ribbon\""));
    assert!(page.contains("<circle class=\"disc\""));
    for name in ["Jurong East", "Choa Chu Kang", "Marina Bay", "Punggol"] {
        assert!(page.contains(name), "the page names {name}");
    }
    // The headway band is words on the line, never trains.
    assert!(page.contains("every 10 min approximately"));
    // Trains are the one thing the script adds; without it the group
    // is present and empty.
    assert!(page.contains("<g class=\"trains\" id=\"map-trains\"></g>"));
    // Without a realtime layer the page says schedule-only, in words.
    assert!(page.contains("schedule only \u{00B7} no realtime layer"));
}

#[test]
fn the_only_network_target_is_the_snapshot_url() {
    let page = schedule_page("/api/map-snapshot");

    // The one URL the script may fetch, delivered as data.
    assert!(page.contains("data-snapshot-url=\"/api/map-snapshot\""));
    // The Content-Security-Policy forbids everything else; a
    // same-origin snapshot URL widens nothing.
    assert!(page.contains("default-src 'none'"));
    assert!(page.contains("connect-src 'self';"));
    // No element loads an external resource: no source attributes, no
    // links, no imports, no CSS url() — the page is self-contained.
    for needle in ["src=", "href=", "@import", "url("] {
        assert!(!page.contains(needle), "the page contains {needle}");
    }

    // A static site pointed at a fast-refresh snapshot on another
    // origin allows exactly that origin, and only for connections.
    let page = schedule_page("https://raw.example.com/live/map.json");
    assert!(page.contains("connect-src 'self' https://raw.example.com;"));
    assert!(page.contains("data-snapshot-url=\"https://raw.example.com/live/map.json\""));
}

#[test]
fn hostile_feed_text_renders_inert() {
    let network = network();
    let mut snapshot = schedule_snapshot(&network);
    let layout = bound_layout(&network);

    // A station name with markup, a headsign with markup, and a
    // route_color that tries to escape the attribute. All three come
    // from feeds, and none may reach the page as anything but text.
    for station in &mut snapshot.stations {
        if station.name == "Jurong East" {
            station.name = "<script>alert('x')</script>".to_string();
        }
    }
    for line in &mut snapshot.lines {
        line.color = Some("\" onload=\"alert(1)".to_string());
    }
    let page = render_map_page(&MapPageInput {
        snapshot: &snapshot,
        layout: &layout,
        snapshot_url: "/api/map-snapshot",
        deployment: "test build",
    });

    assert!(!page.contains("<script>alert"));
    assert!(page.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;"));
    // The hostile colour never reaches a presentation attribute; the
    // renderer falls back to the layout colour or the palette.
    assert!(!page.contains("onload"));
}
