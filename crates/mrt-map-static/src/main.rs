//! Generate the live map as a static site.
//!
//! The generator downloads the official GTFS Schedule feed, builds one
//! whole-network snapshot, and writes a self-contained map site that
//! any file host can serve, including GitHub Pages. The map is its own
//! site, separate from the board, so it deploys on its own subdomain
//! or repository path.
//!
//! Usage:
//!
//! ```sh
//! export LTA_DATAMALL_ACCOUNT_KEY=<your key>
//! cargo run -p mrt-map-static -- <output-dir> [feed.zip]
//! ```
//!
//! Without an account key the generator needs a local feed path and
//! skips the realtime layer: the snapshot is then the schedule, the
//! freshness state is `unavailable`, and every train is schedule-only.
//!
//! Output layout:
//!
//! ```text
//! <output-dir>/
//!   index.html      the map page
//!   .nojekyll       serve files as they are
//!   data/map.json   the whole network, the page's one fetch
//! ```
//!
//! The map draws the OpenFantasyMap GeoJSON layout named by
//! `MRT_MAP_LAYOUT`, and falls back to `config/layout-mini.geojson`.
//!
//! # Where the page polls
//!
//! By default the page polls the bundled `data/map.json`, which is
//! refreshed only when the site is rebuilt. On GitHub Pages that makes
//! the map a schedule animation with a delay hint, and the page says
//! so. The board's runtime `config.json` fallback chain does not
//! transfer here — this page makes exactly one request, and its
//! Content-Security-Policy allows exactly one — so the deployment
//! story is simpler: set `MRT_MAP_SNAPSHOT_URL` at build time to point
//! the page at a fast-refresh snapshot (for example one a scheduled
//! workflow publishes, in the manner of `mrt-rt-snapshot`), and the
//! renderer allows that one origin in the policy.

use std::path::Path;

use mrt_datamall::DataMallClient;
use mrt_gtfs::{GtfsFeed, RailNetwork};
use mrt_gtfs_rt::RailRtFeed;
use mrt_live::{clock, NetworkSnapshotBuilder};
use mrt_map_web::{load_layout, map_snapshot_json, render_map_page, MapPageInput};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| {
        eprintln!("usage: mrt-map-static <output-dir> [feed.zip]");
        std::process::exit(2);
    });
    let out = Path::new(&out);
    let feed_path = args.next();

    let client = DataMallClient::from_env().ok();

    // Step 1: load the feed and build the network.
    let feed = match &feed_path {
        Some(path) => {
            eprintln!("Reading the GTFS Schedule feed from {path} ...");
            GtfsFeed::from_zip_path(path).expect("cannot read the local feed")
        }
        None => {
            eprintln!("Downloading the GTFS Schedule feed from DataMall ...");
            let client = client
                .as_ref()
                .expect("set LTA_DATAMALL_ACCOUNT_KEY, or pass a path to a GTFS zip archive");
            let bytes = client
                .fetch_gtfs_schedule()
                .expect("cannot download the feed");
            let mut source = mrt_gtfs::ZipSource::from_reader(std::io::Cursor::new(bytes))
                .expect("bad zip archive");
            GtfsFeed::load(&mut source).expect("cannot parse the feed")
        }
    };
    let network = RailNetwork::from_feed(&feed).expect("cannot build the rail network");
    eprintln!(
        "Network ready: {} lines, {} stations, {} trips.",
        network.lines().len(),
        network.stations().len(),
        network.trip_count()
    );

    // Step 2: the live layers, where a key reaches them. Without one
    // the snapshot is the schedule and says so.
    let alerts = client.as_ref().and_then(|c| c.train_service_alerts().ok());
    let realtime = client.as_ref().and_then(|c| {
        c.fetch_trip_updates()
            .ok()
            .and_then(|bytes| RailRtFeed::decode(&bytes).ok())
    });
    if client.is_none() {
        eprintln!("LTA_DATAMALL_ACCOUNT_KEY is not set; the map shows the static schedule.");
    }

    // Step 3: one snapshot, at Singapore time. The builder reads no
    // clock; this generator is the caller that does.
    let now_unix = clock::unix_now();
    let mut builder = NetworkSnapshotBuilder::new(&network);
    if let Some(alerts) = &alerts {
        builder = builder.with_alerts(alerts);
    }
    if let Some(realtime) = &realtime {
        builder = builder.with_realtime(realtime, now_unix.max(0) as u64);
    }
    let (date, clock_now) = clock::sgt_from_unix(now_unix);
    let snapshot = builder.build(date, clock_now);

    // Step 4: the layout, bound to the network.
    let layout = load_layout(&network);

    // Step 5: write the site. The page polls the bundled snapshot
    // unless the caller names a fast-refresh URL; see the module
    // documentation for the trade-off.
    let snapshot_url = std::env::var("MRT_MAP_SNAPSHOT_URL")
        .ok()
        .filter(|url| !url.is_empty())
        // Relative, because a GitHub Pages project site lives under
        // /<repository>/.
        .unwrap_or_else(|| "data/map.json".to_string());
    let deployment = if snapshot_url == "data/map.json" {
        "static build \u{00B7} data/map.json is refreshed only when the site is rebuilt, \
         so this is a schedule animation with a delay hint"
            .to_string()
    } else {
        "static build \u{00B7} the page polls a separately refreshed snapshot".to_string()
    };
    let page = render_map_page(&MapPageInput {
        snapshot: &snapshot,
        layout: &layout,
        snapshot_url: &snapshot_url,
        deployment: &deployment,
    });

    let data_dir = out.join("data");
    std::fs::create_dir_all(&data_dir).expect("cannot create the output directory");
    std::fs::write(out.join("index.html"), page).expect("cannot write index.html");
    std::fs::write(out.join(".nojekyll"), "").expect("cannot write .nojekyll");
    std::fs::write(
        data_dir.join("map.json"),
        map_snapshot_json(&snapshot, realtime.is_some(), now_unix).to_string(),
    )
    .expect("cannot write data/map.json");

    eprintln!(
        "Map site ready in {}: {} train(s), {} band(s), {} diagnostic(s).",
        out.display(),
        snapshot.trains.len(),
        snapshot.bands.len(),
        snapshot.diagnostics.len() + layout.diagnostics.len()
    );
}
