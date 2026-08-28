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
use mrt_live::{Layout, LineState, NetworkSnapshot, NetworkSnapshotBuilder};
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

/// Bind a layout written for one test to the fixture network.
///
/// The committed layout answers most questions; a few — a drawing far
/// taller than it is wide, an interchange the layout draws twice — need
/// a shape the fixture layout does not have.
fn layout_of(network: &RailNetwork, value: serde_json::Value) -> mrt_live::BoundLayout {
    Layout::from_geojson(&value).bind(network)
}

/// One line feature of a test layout.
fn layout_line(id: &str, color: &str, points: &[(f64, f64)]) -> serde_json::Value {
    let coordinates: Vec<serde_json::Value> = points
        .iter()
        .map(|&(x, y)| serde_json::json!([x, y]))
        .collect();
    serde_json::json!({
        "type": "Feature",
        "properties": {"ofm": "line", "id": id, "name": id, "color": color, "visible": true},
        "geometry": {"type": "LineString", "coordinates": coordinates},
    })
}

/// One station feature of a test layout.
fn layout_stop(id: &str, line: &str, code: &str, t: f64, at: (f64, f64)) -> serde_json::Value {
    serde_json::json!({
        "type": "Feature",
        "properties": {"ofm": "station", "id": id, "lineId": line, "code": code, "t": t},
        "geometry": {"type": "Point", "coordinates": [at.0, at.1]},
    })
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

/// Render the whole page from a snapshot the caller built.
fn page_of(network: &RailNetwork, snapshot: &NetworkSnapshot) -> String {
    let layout = bound_layout(network);
    render_map_page(&MapPageInput {
        snapshot,
        layout: &layout,
        snapshot_url: "/api/map-snapshot",
        deployment: "test build",
    })
}

/// The legacy alert payload of a North South Line disruption.
///
/// It names two stations that one edge of the line joins — Jurong East
/// (`NS1`) and Choa Chu Kang (`NS4`) — so the map has a section to
/// mark and does not have to guess one.
fn disrupted_alerts(stations: &str) -> mrt_datamall::TrainServiceAlerts {
    serde_json::from_str(&format!(
        r#"{{
            "Status": 2,
            "AffectedSegments": [
                {{
                    "Line": "NSL",
                    "Direction": "Both",
                    "Stations": "{stations}",
                    "FreePublicBus": "NS1",
                    "FreeMRTShuttle": "",
                    "MRTShuttleDirection": ""
                }}
            ],
            "Message": [{{"Content": "NSL: no service.", "CreatedDate": ""}}]
        }}"#
    ))
    .unwrap()
}

// ----------------------------------------------------------------------
// The committed SVG snapshot
// ----------------------------------------------------------------------

fn svg_snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name)
}

/// Compare a rendered SVG with the stored snapshot, or write it.
///
/// To accept an intended change, run
///
/// ```sh
/// UPDATE_SNAPSHOTS=1 cargo test -p mrt-map-web --test map_page_tests
/// ```
///
/// and review the diff.
fn assert_svg_snapshot(name: &str, actual: &str) {
    let path = svg_snapshot_path(name);
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
            "the SVG no longer matches {}\n  first difference at line {line}\n  \
             run with UPDATE_SNAPSHOTS=1 to accept the change",
            path.display()
        );
    }
}

#[test]
fn the_network_svg_is_stable() {
    let network = network();
    let snapshot = schedule_snapshot(&network);
    let layout = bound_layout(&network);
    let geometry = map_geometry(&snapshot, &layout);
    assert_svg_snapshot("map-mini.svg", &render_network_svg(&snapshot, &geometry));
}

#[test]
fn the_disrupted_network_svg_is_stable() {
    // The same drawing with one line disrupted: the ribbon greys and
    // the section the alert names is cut. The snapshot pins both.
    let network = network();
    let alerts = disrupted_alerts("NS1,NS4");
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_alerts(&alerts)
        .build(date(), GtfsTime::from_hms(6, 5, 0));
    let layout = bound_layout(&network);
    let geometry = map_geometry(&snapshot, &layout);
    assert_svg_snapshot(
        "map-mini-disrupted.svg",
        &render_network_svg(&snapshot, &geometry),
    );
}

#[test]
fn the_svg_snapshot_files_are_committed() {
    // A snapshot that only exists on the machine that wrote it is no
    // snapshot at all.
    for name in ["map-mini.svg", "map-mini-disrupted.svg"] {
        let path = svg_snapshot_path(name);
        assert!(path.metadata().is_ok_and(|m| m.len() > 0), "{path:?}");
    }
}

// ----------------------------------------------------------------------
// The drawing
// ----------------------------------------------------------------------

#[test]
fn a_ribbon_that_starts_at_an_interchange_keeps_its_own_colour() {
    let network = network();
    let snapshot = schedule_snapshot(&network);
    let layout = bound_layout(&network);
    let geometry = map_geometry(&snapshot, &layout);

    // The first station of the East West Line ribbon is Jurong East,
    // which is also the first station of the North South Line. The
    // colour of a ribbon follows the line it carries — the majority
    // vote over the edges the layout draws on it — and never whichever
    // line the first station happens to touch first.
    let color = |id: &str| {
        geometry
            .lines
            .iter()
            .find(|line| line.layout_id == id)
            .map(|line| line.color.clone())
            .unwrap_or_else(|| panic!("the layout draws {id}"))
    };
    assert_eq!(color("line-nsl"), "#D42E12");
    assert_eq!(color("line-ewl"), "#009645");
    // The LRT ribbon starts at Choa Chu Kang, an interchange too.
    assert_eq!(color("line-bpl"), "#748477");

    // A station the layout draws on one ribbon takes that ribbon's
    // colour, for the same reason.
    let disc = |name: &str| {
        geometry
            .stations
            .iter()
            .find(|station| station.name == name)
            .map(|station| station.color.clone())
            .unwrap_or_else(|| panic!("the drawing carries {name}"))
    };
    assert_eq!(disc("Raffles Place"), "#009645");
    assert_eq!(disc("South View"), "#748477");
    assert_eq!(disc("Marina Bay"), "#D42E12");
}

#[test]
fn a_tall_layout_keeps_its_names_beside_the_discs() {
    let network = network();
    let snapshot = schedule_snapshot(&network);
    // One straight line, drawn far taller than it is wide. The fit
    // scales it to a box much narrower than the full view.
    let layout = layout_of(
        &network,
        serde_json::json!({
            "type": "FeatureCollection",
            "features": [
                layout_line("line-nsl", "#d42e12", &[(103.74, 1.28), (103.74, 1.46)]),
                layout_stop("s1", "line-nsl", "NS1", 0.0, (103.74, 1.28)),
                layout_stop("s2", "line-nsl", "NS4", 0.5, (103.74, 1.37)),
                layout_stop("s3", "line-nsl", "NS27", 1.0, (103.74, 1.46)),
            ],
        }),
    );
    let geometry = map_geometry(&snapshot, &layout);

    // The drawing is nowhere near the 1000 units of the full view.
    assert!(geometry.width < 200.0, "{}", geometry.width);
    // Every station sits at the left of that box, so every name goes
    // to the right of its disc and stays on the drawing.
    assert!(geometry.stations.iter().all(|station| !station.label_left));
    let svg = render_network_svg(&snapshot, &geometry);
    assert!(!svg.contains("text-anchor=\"end\""));
    assert!(svg.contains("text-anchor=\"start\""));
}

#[test]
fn a_chord_starts_at_the_disc_of_an_interchange() {
    let network = network();
    let snapshot = schedule_snapshot(&network);
    // Jurong East is drawn twice, on two ribbons a long way apart, so
    // its disc sits at the mean of the two placements. Choa Chu Kang is
    // drawn on a third ribbon of its own, so the edge that joins the
    // two stations shares no ribbon and the map falls back to a chord.
    let layout = layout_of(
        &network,
        serde_json::json!({
            "type": "FeatureCollection",
            "features": [
                layout_line("line-a", "#d42e12", &[(0.0, 0.0), (0.0, 10.0)]),
                layout_line("line-b", "#009645", &[(4.0, 0.0), (4.0, 10.0)]),
                layout_line("line-c", "#748477", &[(8.0, 0.0), (8.0, 10.0)]),
                layout_stop("a1", "line-a", "NS1", 0.0, (0.0, 0.0)),
                layout_stop("a2", "line-a", "EW21", 1.0, (0.0, 10.0)),
                layout_stop("b1", "line-b", "EW24", 0.0, (4.0, 0.0)),
                layout_stop("c1", "line-c", "NS4", 1.0, (8.0, 10.0)),
            ],
        }),
    );
    let geometry = map_geometry(&snapshot, &layout);

    let jurong = geometry
        .stations
        .iter()
        .find(|station| station.name == "Jurong East")
        .expect("the drawing carries Jurong East");
    assert!(jurong.interchange);
    let choa = geometry
        .stations
        .iter()
        .find(|station| station.name == "Choa Chu Kang")
        .expect("the drawing carries Choa Chu Kang");

    // The map says so rather than hiding the gap.
    assert!(geometry
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "map-edge-without-geometry"));

    // The chord runs between the two discs. Anything else leaves the
    // chord — and every train riding it — floating off the disc.
    let edge = snapshot
        .edges
        .iter()
        .find(|edge| edge.from == jurong.station && edge.to == choa.station)
        .expect("the network joins Jurong East to Choa Chu Kang");
    let key = format!("{}-{}-{}", edge.line.0, edge.from.0, edge.to.0);
    let chord = geometry.sections.get(&key).expect("the edge is drawn");
    assert_eq!(chord.len(), 2);
    assert!((chord[0].x - jurong.point.x).abs() < 1e-9);
    assert!((chord[0].y - jurong.point.y).abs() < 1e-9);
    assert!((chord[1].x - choa.point.x).abs() < 1e-9);
    assert!((chord[1].y - choa.point.y).abs() < 1e-9);
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
fn the_script_re_ages_the_freshness_it_was_given() {
    // A static deployment polls a bundled data/map.json that keeps
    // answering long after it was written. The freshness inside it was
    // judged when it was built, so the script may not read that state
    // verbatim: it adds the time that has really passed since the
    // document's own `generated` stamp and reads both thresholds the
    // freshness carries. The script is inline, so the assertions are
    // on its text, as the rest of the page's script tests are.
    let page = schedule_page("data/map.json");

    assert!(page.contains("generated = body.generated;"));
    assert!(page.contains("function sinceGenerated()"));
    assert!(page.contains("freshness.age_secs + sinceGenerated()"));
    assert!(page.contains("age > freshness.staleness_secs"));
    assert!(page.contains("age > freshness.ageing_secs"));
    // The lamp and the words follow the recomputed state.
    assert!(page.contains("var freshness = agedFreshness();"));
    // A document without a usable stamp keeps the state it was built
    // with rather than guessing a clock offset.
    assert!(page.contains("typeof generated !== \"number\""));

    // The stamp the script ages against is in the document the page
    // polls, whichever deployment writes it.
    let network = network();
    let body = map_snapshot_json(&schedule_snapshot(&network), false, NOW_UNIX as i64);
    assert_eq!(body["generated"], NOW_UNIX);
}

// ----------------------------------------------------------------------
// The three states of the acceptance criteria
// ----------------------------------------------------------------------

#[test]
fn a_disrupted_line_greys_and_its_named_section_is_marked() {
    let network = network();
    let alerts = disrupted_alerts("NS1,NS4");
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_alerts(&alerts)
        .build(date(), GtfsTime::from_hms(6, 5, 0));
    let page = page_of(&network, &snapshot);

    // The line is not deleted: it keeps its ribbon and takes the
    // disrupted class, which is what greys it.
    assert!(page.contains("<g class=\"ribbon-group disrupted\""));
    // The section the alert names is cut out of it. Jurong East and
    // Choa Chu Kang are joined by one edge of the line, so exactly one
    // section is marked.
    assert_eq!(page.matches("<path class=\"disrupted-section\"").count(), 1);
    // Only the disrupted line greys; the others keep their identity.
    assert!(page.contains("<g class=\"ribbon-group\" data-layout-line=\"line-ewl\""));

    // The state is named in words, from the fields the alert carries
    // and from nothing else.
    assert!(page.contains("<p class=\"notice disrupted\">"));
    assert!(page.contains("NSL disrupted"));
    assert!(page.contains("direction Both"));
    assert!(page.contains("NS1, NS4"));
    assert!(page.contains("free public bus at NS1"));
    // The alert text reaches the page as the network notice it is.
    assert!(page.contains("<p class=\"notice\">NSL: no service.</p>"));

    // The trains of a disrupted line are still drawn from what the
    // feed says: the line loses its colour, not its service.
    assert!(page.contains("<g class=\"trains\" id=\"map-trains\"></g>"));
    assert!(snapshot.trains.iter().any(|train| train.line.0 == 0));
}

#[test]
fn an_alert_without_stations_marks_the_line_and_not_a_guess() {
    let network = network();
    let alerts = disrupted_alerts("");
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_alerts(&alerts)
        .build(date(), GtfsTime::from_hms(6, 5, 0));
    let page = page_of(&network, &snapshot);

    // The line greys, and nothing on it is marked: the alert names no
    // station, so the map has no section it may claim.
    assert!(page.contains("<g class=\"ribbon-group disrupted\""));
    assert!(!page.contains("<path class=\"disrupted-section\""));
    assert!(page.contains("the alert names no station"));
    // The gap is reported rather than hidden.
    assert!(page.contains("map-disruption-without-segment"));
}

#[test]
fn a_stale_feed_reads_red_with_its_age_in_words() {
    let network = network();
    let realtime = RailRtFeed {
        feed_timestamp: Some(NOW_UNIX - 600),
        trip_updates: vec![TripUpdate {
            trip_id: Some("NS_T1".to_string()),
            delay_secs: Some(120),
            ..Default::default()
        }],
        ..Default::default()
    };
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));
    let page = page_of(&network, &snapshot);

    // Red, and the age is in words beside it.
    assert!(page.contains("<span class=\"lamp stale\""));
    assert!(page.contains("schedule only \u{00B7} realtime feed 10 min ago"));
    // The prediction is not applied, and the page says why.
    assert!(page.contains("realtime-stale"));
}

#[test]
fn an_ageing_feed_reads_amber_and_keeps_its_predictions() {
    let network = network();
    let realtime = RailRtFeed {
        feed_timestamp: Some(NOW_UNIX - 80),
        trip_updates: vec![TripUpdate {
            trip_id: Some("NS_T1".to_string()),
            delay_secs: Some(120),
            ..Default::default()
        }],
        ..Default::default()
    };
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));
    let page = page_of(&network, &snapshot);

    assert!(page.contains("<span class=\"lamp ageing\""));
    assert!(page.contains("ageing \u{00B7} realtime feed 80 s ago"));
    // Amber is not red: the run still carries the operator's delay.
    let run = snapshot
        .trains
        .iter()
        .find(|train| train.source_trip_id == "NS_T1")
        .expect("NS_T1 is running at 06:05");
    assert_eq!(run.delay_secs, Some(120));
}

#[test]
fn an_empty_realtime_snapshot_says_what_it_is() {
    let network = network();
    // A feed that arrived, is current, and names no run at all.
    let realtime = RailRtFeed {
        feed_timestamp: Some(NOW_UNIX - 10),
        trip_updates: Vec::new(),
        ..Default::default()
    };
    let snapshot = NetworkSnapshotBuilder::new(&network)
        .with_realtime(&realtime, NOW_UNIX)
        .build(date(), GtfsTime::from_hms(6, 5, 0));
    let page = page_of(&network, &snapshot);

    // The lamp is green, because the feed itself is current, and the
    // page has to say plainly that nothing on the map came from it.
    assert!(page.contains("<span class=\"lamp live\""));
    assert!(page.contains("The realtime layer names no run drawn on this map"));
    assert!(page.contains("realtime-without-trip-updates"));
    assert!(!snapshot.trains.is_empty());
}

#[test]
fn a_normal_network_carries_no_notice_area() {
    // The notice area exists only when there is something to say.
    let page = schedule_page("/api/map-snapshot");
    assert!(!page.contains("<section class=\"notices\""));
    assert!(!page.contains("<path class=\"disrupted-section\""));
    assert!(!page.contains("ribbon-group disrupted"));
    assert!(page.contains("<span class=\"lamp stale\""));
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

#[test]
fn hostile_alert_text_renders_inert() {
    let network = network();
    let mut snapshot = schedule_snapshot(&network);

    // The alert fields are feed text too: the message, the direction,
    // and the station codes all reach the notice area, and none of
    // them may reach it as markup.
    snapshot.notices = vec!["<img src=x onerror=alert(1)>".to_string()];
    for line in &mut snapshot.lines {
        if line.name == "NSL" {
            line.state = LineState::Disrupted {
                stations: vec!["NS1</p><script>alert(2)</script>".to_string()],
                direction: "\"><script>alert(3)</script>".to_string(),
                free_public_bus: vec!["<b>NS1</b>".to_string()],
            };
        }
    }
    let page = page_of(&network, &snapshot);

    assert!(page.contains("class=\"notice disrupted\""));
    assert!(!page.contains("<script>alert"));
    assert!(!page.contains("<img src=x"));
    assert!(!page.contains("<b>NS1</b>"));
    assert!(page.contains("&lt;img src=x onerror=alert(1)&gt;"));
}
