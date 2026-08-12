//! Rendering tests: escaping, accessibility, print, and determinism.

use std::path::PathBuf;

use mrt_gtfs::{
    Calendar, FrequencyPolicy, GtfsFeed, GtfsTime, RailNetwork, Route, ServiceDate, StationId,
    Stop, StopTime, Trip,
};
use mrt_publication::{
    build_diagram, build_timetable, DiagramTarget, DocumentSeed, Language, PublicationConfig,
};
use mrt_publication_html::{render_diagram, render_diagram_svg, render_timetable};

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
        generator_version: "mrt-schedule-cli 0.1.0".into(),
        feed_sha256: "a".repeat(64),
        feed_timestamp: Some("2026-08-10T00:00:00+08:00".into()),
        timezone: "Asia/Singapore".into(),
        generated_from_cache: false,
        configuration_sha256: "b".repeat(64),
    }
}

fn station(network: &RailNetwork, code: &str) -> StationId {
    network.station_by_code(code).unwrap()
}

fn timetable_html(config: &PublicationConfig) -> String {
    let network = network();
    let document = build_timetable(
        &network,
        station(&network, "TE1"),
        date("20250505"),
        None,
        config,
        &seed(),
    )
    .unwrap();
    render_timetable(&document, config)
}

fn diagram_document(config: &PublicationConfig) -> mrt_publication::DiagramDocument {
    let network = network();
    build_diagram(
        &network,
        &DiagramTarget::Line(network.line_by_route_id("TE").unwrap()),
        date("20250505"),
        time("05:00:00"),
        time("10:00:00"),
        config,
        &seed(),
    )
    .unwrap()
}

// ----------------------------------------------------------------------
// Structure
// ----------------------------------------------------------------------

#[test]
fn the_timetable_is_one_self_contained_document() {
    let html = timetable_html(&PublicationConfig::default());
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.trim_end().ends_with("</html>"));
    assert!(html.contains("Content-Security-Policy"));
    assert!(html.contains("default-src &#39;none&#39;"));

    // Nothing may reach out to the network or to another file.
    for forbidden in ["<link ", "src=\"http", "src=\"//", "@import", "url(http"] {
        assert!(!html.contains(forbidden), "found {forbidden}");
    }
}

#[test]
fn the_timetable_reads_without_a_stylesheet() {
    let html = timetable_html(&PublicationConfig::default());
    // The hour and the minutes are real table cells, not styled divs.
    assert!(html.contains("<th scope=\"row\" class=\"hour-cell\">06</th>"));
    assert!(html.contains("<table class=\"hours\">"));
    assert!(html.contains("<h1>"));
    assert!(html.contains("<h2 class=\"direction\""));
    // Every interactive control is hidden until a script enables it.
    assert!(html.contains("class=\"controls needs-script no-print\""));
}

#[test]
fn every_departure_carries_a_spoken_form() {
    let html = timetable_html(&PublicationConfig::default());
    assert!(html.contains("05:10 \u{2192} Springleaf, Platform 1"));
    // Seconds appear in the spoken form when the feed carries them.
    assert!(html.contains("06:00:30 \u{2192} Springleaf, Platform 1"));
    // The first and the last departure of the panel say so.
    assert!(html.contains("first departure of the service day"));
    // The visual parts are hidden from assistive technology, because
    // the spoken form already carries them.
    assert!(html.contains("<span aria-hidden=\"true\" class=\"min\">10</span>"));
}

#[test]
fn the_print_profiles_cover_a4_and_a3() {
    let timetable = timetable_html(&PublicationConfig::default());
    assert!(timetable.contains("size: A4 portrait"));
    assert!(timetable.contains("print-color-adjust: exact"));
    assert!(timetable.contains("@media print and (monochrome)"));

    let config = PublicationConfig::default();
    let diagram = render_diagram(&diagram_document(&config), &config);
    assert!(diagram.contains("size: A3 landscape"));
}

#[test]
fn the_colophon_carries_the_provenance() {
    let html = timetable_html(&PublicationConfig::default());
    assert!(html.contains("aaaaaaaaaaaa")); // the short feed fingerprint
    assert!(html.contains("mrt-schedule-cli 0.1.0"));
    assert!(html.contains("2026-08-10T00:00:00+08:00"));
    assert!(html.contains("Asia/Singapore"));
}

#[test]
fn a_cached_feed_is_stated_on_the_page() {
    let mut stale = seed();
    stale.generated_from_cache = true;
    let network = network();
    let config = PublicationConfig::default();
    let document = build_timetable(
        &network,
        station(&network, "TE1"),
        date("20250505"),
        None,
        &config,
        &stale,
    )
    .unwrap();
    let html = render_timetable(&document, &config);
    assert!(html.contains("generated from a cached feed"));
    assert!(html.contains("class=\"stale\""));
}

#[test]
fn japanese_labels_switch_the_whole_page() {
    let config = PublicationConfig {
        language: Language::Ja,
        ..Default::default()
    };
    let html = timetable_html(&config);
    assert!(html.contains("<html lang=\"ja\">"));
    assert!(html.contains("発車時刻表"));
    assert!(html.contains("番のりば"));
}

// ----------------------------------------------------------------------
// Diagram and SVG
// ----------------------------------------------------------------------

#[test]
fn the_standalone_svg_is_a_complete_document() {
    let config = PublicationConfig::default();
    let svg = render_diagram_svg(&diagram_document(&config), &config);
    assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(svg.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(svg.contains("width=\""));
    assert!(svg.contains("<title id=\"svg-title\">"));
    assert!(svg.contains("<desc id=\"svg-desc\">"));
    assert!(svg.trim_end().ends_with("</svg>"));
    // Standalone means standalone: no external reference at all.
    for forbidden in [
        "xlink:href",
        "<image",
        "http://www.w3.org/1999/xlink",
        "url(http",
    ] {
        assert!(!svg.contains(forbidden), "found {forbidden}");
    }
    assert_eq!(svg.matches("<svg").count(), 1);
    assert_eq!(svg.matches("</svg>").count(), 1);
}

#[test]
fn the_svg_tags_every_run_for_the_filters() {
    let config = PublicationConfig::default();
    let svg = render_diagram_svg(&diagram_document(&config), &config);
    assert!(svg.contains("data-run=\"20250505:TE_T1\""));
    assert!(svg.contains("data-line=\"TE\""));
    assert!(svg.contains("data-direction=\"0\""));
    assert!(svg.contains("data-exactness=\"exact\""));
    // A public train name reaches the drawing; the internal
    // identifier does not.
    assert!(svg.contains(">T101<"));
    assert!(!svg.contains(">TE_T1<"));
}

#[test]
fn the_internal_trip_identifier_appears_only_when_it_is_asked_for() {
    let mut config = PublicationConfig::default();
    // The identifier is a machine key in `data-run`, which nothing
    // draws. The visible parts of the drawing are the run titles and
    // the `<text>` elements.
    let plain = render_diagram_svg(&diagram_document(&config), &config);
    assert!(!visible_text(&plain).contains("TE_T1"));

    config.diagram.show_internal_trip_ids = true;
    let verbose = render_diagram_svg(&diagram_document(&config), &config);
    assert!(visible_text(&verbose).contains("TE_T1"));
}

/// Collect everything an SVG actually draws or announces: the content
/// of `<title>`, `<desc>`, and `<text>` elements.
fn visible_text(svg: &str) -> String {
    let mut out = String::new();
    for (open, close) in [
        ("<title", "</title>"),
        ("<desc", "</desc>"),
        ("<text", "</text>"),
    ] {
        let mut rest = svg;
        while let Some(start) = rest.find(open) {
            let after = &rest[start..];
            let Some(gt) = after.find('>') else { break };
            let Some(end) = after.find(close) else { break };
            out.push_str(&after[gt + 1..end]);
            out.push('\n');
            rest = &after[end + close.len()..];
        }
    }
    out
}

#[test]
fn the_diagram_page_carries_a_call_table_for_every_run() {
    let config = PublicationConfig::default();
    let document = diagram_document(&config);
    let html = render_diagram(&document, &config);
    assert_eq!(html.matches("<details>").count(), document.runs.len());
    assert!(html.contains("<th scope=\"col\">Arrival</th>"));
    // The tables are the JavaScript-free view of the drawing.
    assert!(html.contains("class=\"call-tables\""));
}

#[test]
fn the_json_island_stays_inert() {
    let config = PublicationConfig::default();
    let html = render_diagram(&diagram_document(&config), &config);
    let start = html.find("id=\"diagram-data\">").unwrap();
    let end = html[start..].find("</script>").unwrap() + start;
    let island = &html[start + "id=\"diagram-data\">".len()..end];
    assert!(!island.contains('<'));
    let parsed: serde_json::Value = serde_json::from_str(island).unwrap();
    assert!(parsed["runs"].as_array().unwrap().len() > 1);
    assert_eq!(parsed["viewBox"]["x"], 0.0);
}

#[test]
fn a_headway_band_is_drawn_dashed_and_named() {
    let network = network();
    let config = PublicationConfig::default();
    let document = build_diagram(
        &network,
        &DiagramTarget::Line(network.line_by_route_id("BP").unwrap()),
        date("20250505"),
        time("05:00:00"),
        time("07:00:00"),
        &config,
        &seed(),
    )
    .unwrap();
    let html = render_diagram(&document, &config);
    assert!(html.contains("class=\"band\""));
    assert!(html.contains("stroke-dasharray"));
    assert!(html.contains("every 10 min approximately"));
    assert!(html.contains("class=\"band-list\""));
}

// ----------------------------------------------------------------------
// Escaping
// ----------------------------------------------------------------------

/// A feed whose text fields try to break out of the markup.
fn hostile_network() -> RailNetwork {
    let feed = GtfsFeed {
        stops: vec![
            Stop {
                stop_id: "A".into(),
                stop_code: Some("<img src=x onerror=alert(1)>".into()),
                stop_name: Some("</h1><script>alert('a')</script> & Co".into()),
                platform_code: Some("\"><script>alert(2)</script>".into()),
                ..Default::default()
            },
            Stop {
                stop_id: "B".into(),
                stop_name: Some("Beta & Co \u{2014} <b>bold</b>".into()),
                ..Default::default()
            },
        ],
        routes: vec![Route {
            route_id: "R".into(),
            agency_id: None,
            route_short_name: Some("</style><script>alert(3)</script>".into()),
            route_long_name: None,
            route_type: 1,
            // A colour that tries to inject a declaration.
            route_color: Some("red;} body {display:none".into()),
            route_text_color: None,
        }],
        trips: vec![Trip {
            route_id: "R".into(),
            service_id: "D".into(),
            trip_id: "T".into(),
            trip_headsign: Some("</title><svg onload=alert(4)>".into()),
            trip_short_name: Some("</text><script>alert(5)</script>".into()),
            direction_id: Some(0),
            ..Default::default()
        }],
        stop_times: vec![
            StopTime {
                trip_id: "T".into(),
                arrival_time: Some(time("06:00:00")),
                departure_time: Some(time("06:00:00")),
                stop_id: "A".into(),
                stop_sequence: 1,
                ..Default::default()
            },
            StopTime {
                trip_id: "T".into(),
                arrival_time: Some(time("06:10:00")),
                departure_time: Some(time("06:10:00")),
                stop_id: "B".into(),
                stop_sequence: 2,
                ..Default::default()
            },
        ],
        calendar: vec![Calendar {
            service_id: "D".into(),
            monday: 1,
            tuesday: 1,
            wednesday: 1,
            thursday: 1,
            friday: 1,
            saturday: 1,
            sunday: 1,
            start_date: date("20250101"),
            end_date: date("20271231"),
        }],
        ..Default::default()
    };
    RailNetwork::from_feed(&feed).unwrap()
}

/// The exact byte sequences that would make injected feed text run.
///
/// The pages carry their own `<style>` and `<script>` elements, so the
/// check looks for the payloads themselves rather than for any tag.
const INJECTIONS: [&str; 8] = [
    "<script>alert",
    "</script>alert",
    "<svg onload",
    "<img src=x",
    "</h1><script",
    "</style><script",
    "</text><script",
    "display:none",
];

#[test]
fn hostile_feed_text_cannot_break_out_of_the_timetable() {
    let network = hostile_network();
    let config = PublicationConfig::default();
    let document = build_timetable(
        &network,
        network.station_by_gtfs_id("A").unwrap(),
        date("20250505"),
        None,
        &config,
        &seed(),
    )
    .unwrap();
    let html = render_timetable(&document, &config);

    for injection in INJECTIONS {
        assert!(!html.contains(injection), "the page contains {injection}");
    }
    // The renderer emits exactly one script element of its own, and
    // no feed text added another.
    assert_eq!(html.matches("<script").count(), 1);
    assert_eq!(html.matches("<style").count(), 1);
    // The text is still there, escaped.
    assert!(html.contains("&lt;script&gt;alert(&#39;a&#39;)&lt;/script&gt; &amp; Co"));
    // The hostile colour never becomes a declaration.
    assert!(!html.contains("--line-color: red"));
}

#[test]
fn hostile_feed_text_cannot_break_out_of_the_diagram() {
    let network = hostile_network();
    let mut config = PublicationConfig::default();
    config.diagram.show_internal_trip_ids = true;
    let document = build_diagram(
        &network,
        &DiagramTarget::Line(network.line_by_route_id("R").unwrap()),
        date("20250505"),
        time("05:00:00"),
        time("08:00:00"),
        &config,
        &seed(),
    )
    .unwrap();

    let html = render_diagram(&document, &config);
    let svg = render_diagram_svg(&document, &config);
    for rendered in [&html, &svg] {
        for injection in INJECTIONS {
            assert!(
                !rendered.contains(injection),
                "the output contains {injection}"
            );
        }
    }
    // The page carries the JSON island plus the interaction script;
    // the standalone drawing carries no script at all.
    assert_eq!(html.matches("<script").count(), 2);
    assert_eq!(svg.matches("<script").count(), 0);
    assert!(svg.contains("&lt;svg onload=alert(4)&gt;"));
}

#[test]
fn a_hostile_configuration_cannot_inject_css() {
    let network = network();
    let mut config = PublicationConfig::default();
    config.theme.font_stack = vec!["Noto Sans\"; } html { display: none } .x { color: red".into()];
    config.theme.hour_cell = "url(https://evil.example/pixel.png)".into();
    let document = build_timetable(
        &network,
        station(&network, "TE1"),
        date("20250505"),
        None,
        &config,
        &seed(),
    )
    .unwrap();
    let html = render_timetable(&document, &config);
    assert!(!html.contains("display: none } .x"));
    assert!(!html.contains("evil.example"));
    assert!(html.contains("--hour-bg: #1b2a5e"));
}

// ----------------------------------------------------------------------
// Determinism
// ----------------------------------------------------------------------

#[test]
fn rendering_is_byte_for_byte_reproducible() {
    let config = PublicationConfig::default();
    assert_eq!(timetable_html(&config), timetable_html(&config));

    let a = render_diagram_svg(&diagram_document(&config), &config);
    let b = render_diagram_svg(&diagram_document(&config), &config);
    assert_eq!(a, b);
}

#[test]
fn the_expanded_frequency_policy_marks_every_generated_departure() {
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
    let html = render_timetable(&document, &config);

    // Three generated departures, each with the approximation mark
    // and the class that dots its underline.
    assert_eq!(html.matches("class=\"dep approximate\"").count(), 3);
    assert_eq!(html.matches("class=\"flag flag-approximate\"").count(), 3);
    assert!(html.contains("approximate time from a headway"));
}
