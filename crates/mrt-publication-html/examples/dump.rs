//! Write example pages into a directory, for manual inspection.
//!
//! ```sh
//! cargo run -p mrt-publication-html --example dump -- out/
//! ```

use std::path::PathBuf;

use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork};
use mrt_publication::{
    build_diagram, build_timetable, DiagramTarget, DocumentSeed, PublicationConfig,
};
use mrt_publication_html::{render_diagram, render_diagram_svg, render_timetable};

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "out".into()));
    std::fs::create_dir_all(&out).unwrap();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mrt-gtfs/tests/fixtures/mini");
    let network = RailNetwork::from_feed(&GtfsFeed::from_dir(dir).unwrap()).unwrap();
    let config = PublicationConfig::default();
    let seed = DocumentSeed {
        generator_version: "example".into(),
        feed_sha256: "0".repeat(64),
        feed_timestamp: None,
        timezone: "Asia/Singapore".into(),
        generated_from_cache: false,
        configuration_sha256: "0".repeat(64),
    };
    let date = "20250505".parse().unwrap();
    let station = network.station_by_code("TE1").unwrap();
    let timetable = build_timetable(&network, station, date, None, &config, &seed).unwrap();
    std::fs::write(
        out.join("timetable.html"),
        render_timetable(&timetable, &config),
    )
    .unwrap();

    let target = DiagramTarget::Line(network.line_by_route_id("TE").unwrap());
    let diagram = build_diagram(
        &network,
        &target,
        date,
        GtfsTime::from_hms(5, 0, 0),
        GtfsTime::from_hms(10, 0, 0),
        &config,
        &seed,
    )
    .unwrap();
    std::fs::write(out.join("diagram.html"), render_diagram(&diagram, &config)).unwrap();
    std::fs::write(
        out.join("diagram.svg"),
        render_diagram_svg(&diagram, &config),
    )
    .unwrap();
    println!("wrote {}", out.display());
}
