//! The live train map in the browser.
//!
//! The server builds the rail network from the official GTFS Schedule
//! feed, refreshes the live layers from LTA DataMall, and serves the
//! whole network as one schematic page with trains moving along it.
//! The map is its own site, separate from the board: the two servers
//! share nothing but their view-model crates, so each deploys on its
//! own subdomain.
//!
//! Usage:
//!
//! ```sh
//! export LTA_DATAMALL_ACCOUNT_KEY=<your key>
//! cargo run -p mrt-map-web                        # download the feed
//! cargo run -p mrt-map-web -- gtfs_schedule.zip   # use a local feed
//! ```
//!
//! Then open <http://127.0.0.1:8601>. Without an account key the
//! server still works: there is no realtime layer, the freshness state
//! is `unavailable`, and every train is schedule-only.
//!
//! Endpoints:
//!
//! - `GET /` — the map page.
//! - `GET /api/map-snapshot` — the whole network as JSON, for the
//!   page's poll.
//!
//! The map needs a schematic layout. It reads the OpenFantasyMap
//! GeoJSON export named by `MRT_MAP_LAYOUT`, and falls back to
//! `config/layout-mini.geojson`. A layout that does not match the feed
//! is not an error: the page draws what bound and lists the rest under
//! its diagnostics.

use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mrt_datamall::{DataMallClient, TrainServiceAlerts, UreqTransport};
use mrt_gtfs::{GtfsFeed, RailNetwork, ZipSource};
use mrt_gtfs_rt::RailRtFeed;
use mrt_live::{clock, BoundLayout, NetworkSnapshot, NetworkSnapshotBuilder};
use mrt_map_web::{load_layout, map_snapshot_json, render_map_page, MapPageInput, POLL_SECS};

/// How long the server reuses fetched live data, the map snapshot,
/// and the page built from it.
///
/// Building the snapshot walks every running trip of the network, so
/// it is the expensive request here. The page polls every [`POLL_SECS`]
/// seconds behind this, exactly as the board polls `/api/board` behind
/// its own 20-second TTL.
const MAP_TTL: Duration = Duration::from_secs(20);

/// The default listen address. Override with `MRT_MAP_ADDR`.
///
/// The port is one above the board's 8600, so both servers run side by
/// side on one machine.
const DEFAULT_ADDR: &str = "127.0.0.1:8601";

/// Cached live data with its fetch time.
///
/// The map needs the alerts and the trip updates, and no crowd data: a
/// crowd level belongs to one platform, and the map draws none.
#[derive(Default)]
struct LiveCache {
    alerts: Option<(Instant, TrainServiceAlerts)>,
    realtime: Option<(Instant, RailRtFeed)>,
}

/// One built snapshot, with the two stamps that identify it.
///
/// The page and the endpoint are two views of one snapshot, so they
/// share it: a reader who loads the page and then watches it poll sees
/// the same trains in both, and DataMall sees one build per interval
/// rather than two.
struct CachedSnapshot {
    /// When the snapshot was built, for the TTL.
    at: Instant,
    /// The POSIX second it was built at, which the transported document
    /// carries as `generated` and the page ages against.
    generated: i64,
    /// The snapshot itself.
    snapshot: Arc<NetworkSnapshot>,
}

struct App {
    network: RailNetwork,
    client: Option<DataMallClient<UreqTransport>>,
    cache: Mutex<LiveCache>,
    /// The schematic layout, bound to the network.
    layout: BoundLayout,
    /// The one snapshot both handlers read.
    snapshot: Mutex<Option<CachedSnapshot>>,
    /// The rendered snapshot body, keyed by the `generated` stamp of
    /// the snapshot it was rendered from.
    body: Mutex<Option<(i64, String)>>,
    /// The rendered map page, keyed the same way.
    page: Mutex<Option<(i64, String)>>,
}

fn main() {
    let network = load_network();
    eprintln!(
        "Network ready: {} lines, {} stations, {} trips.",
        network.lines().len(),
        network.stations().len(),
        network.trip_count()
    );

    let client = DataMallClient::from_env().ok();
    if client.is_none() {
        eprintln!("LTA_DATAMALL_ACCOUNT_KEY is not set; the map shows the static schedule.");
    }

    let layout = load_layout(&network);
    let app = App {
        network,
        client,
        cache: Mutex::new(LiveCache::default()),
        layout,
        snapshot: Mutex::new(None),
        body: Mutex::new(None),
        page: Mutex::new(None),
    };

    let addr = std::env::var("MRT_MAP_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let server = tiny_http::Server::http(&addr).expect("cannot bind the listen address");
    eprintln!("Map ready on http://{addr}/");

    for request in server.incoming_requests() {
        let path = request.url().split('?').next().unwrap_or("");
        let response = match path {
            "/" => page(map_page(&app), "text/html; charset=utf-8"),
            "/api/map-snapshot" => json(snapshot_body(&app)),
            _ => error_json(404, "not found"),
        };
        let _ = request.respond(response);
    }
}

/// Build the network from a local zip path or from DataMall.
fn load_network() -> RailNetwork {
    let feed = match std::env::args().nth(1) {
        Some(path) => {
            eprintln!("Reading the GTFS Schedule feed from {path} ...");
            GtfsFeed::from_zip_path(&path).expect("cannot read the local feed")
        }
        None => {
            eprintln!("Downloading the GTFS Schedule feed from DataMall ...");
            let client = DataMallClient::from_env()
                .expect("set LTA_DATAMALL_ACCOUNT_KEY, or pass a path to a GTFS zip archive");
            let archive = client
                .fetch_gtfs_schedule()
                .expect("cannot download the feed");
            let mut source = ZipSource::from_reader(Cursor::new(archive)).expect("bad zip archive");
            GtfsFeed::load(&mut source).expect("cannot parse the feed")
        }
    };
    RailNetwork::from_feed(&feed).expect("cannot build the rail network")
}

// ----------------------------------------------------------------------
// Handlers
// ----------------------------------------------------------------------

/// Build the whole-network snapshot.
///
/// The builder reads no clock of its own: this function passes the
/// Singapore date and clock and the POSIX time in, exactly as the
/// board does. Without an account key there is no realtime layer, the
/// freshness state is `unavailable`, and every train is schedule-only.
fn build_snapshot(app: &App, generated: i64) -> NetworkSnapshot {
    let (alerts, realtime) = live_layers(app);
    let mut builder = NetworkSnapshotBuilder::new(&app.network);
    if let Some(alerts) = &alerts {
        builder = builder.with_alerts(alerts);
    }
    if let Some(realtime) = &realtime {
        builder = builder.with_realtime(realtime, generated.max(0) as u64);
    }
    let (date, now) = clock::sgt_from_unix(generated);
    builder.build(date, now)
}

/// Get the current snapshot, from the cache or fresh, with the POSIX
/// second it was built at.
///
/// The same lazy build and TTL as every live layer of the board:
/// DataMall sees one refresh per interval whatever the request rate.
/// Both handlers come through here, so the page and the endpoint never
/// describe two different instants.
fn current_snapshot(app: &App) -> (Arc<NetworkSnapshot>, i64) {
    let mut cache = app.snapshot.lock().expect("snapshot lock");
    if let Some(cached) = cache.as_ref() {
        if cached.at.elapsed() < MAP_TTL {
            return (Arc::clone(&cached.snapshot), cached.generated);
        }
    }
    let generated = clock::unix_now();
    let snapshot = Arc::new(build_snapshot(app, generated));
    *cache = Some(CachedSnapshot {
        at: Instant::now(),
        generated,
        snapshot: Arc::clone(&snapshot),
    });
    (snapshot, generated)
}

/// Get the body of `/api/map-snapshot`, rendered from the shared
/// snapshot and kept until that snapshot is replaced.
fn snapshot_body(app: &App) -> String {
    let (snapshot, generated) = current_snapshot(app);
    let mut cache = app.body.lock().expect("body lock");
    if let Some((stamp, body)) = cache.as_ref() {
        if *stamp == generated {
            return body.clone();
        }
    }
    let body = map_snapshot_json(&snapshot, app.client.is_some(), generated).to_string();
    *cache = Some((generated, body.clone()));
    body
}

/// Get the map page, rendered from the same shared snapshot.
///
/// The page embeds the whole network as SVG, so it changes with the
/// service day rather than with the minute; it is kept until the
/// snapshot it draws is replaced.
fn map_page(app: &App) -> String {
    let (snapshot, generated) = current_snapshot(app);
    let mut cache = app.page.lock().expect("page lock");
    if let Some((stamp, body)) = cache.as_ref() {
        if *stamp == generated {
            return body.clone();
        }
    }
    let body = render_map_page(&MapPageInput {
        snapshot: &snapshot,
        layout: &app.layout,
        snapshot_url: "/api/map-snapshot",
        deployment: &format!("served live \u{00B7} the page re-polls every {POLL_SECS} s"),
    });
    *cache = Some((generated, body.clone()));
    body
}

/// Get the network-wide live layers, from the cache or from the API.
fn live_layers(app: &App) -> (Option<TrainServiceAlerts>, Option<RailRtFeed>) {
    let Some(client) = &app.client else {
        return (None, None);
    };
    let mut cache = app.cache.lock().expect("cache lock");
    let now = Instant::now();
    let fresh = |at: Instant| now.duration_since(at) < MAP_TTL;

    if !cache.alerts.as_ref().is_some_and(|(at, _)| fresh(*at)) {
        if let Ok(alerts) = client.train_service_alerts() {
            cache.alerts = Some((now, alerts));
        }
    }
    if !cache.realtime.as_ref().is_some_and(|(at, _)| fresh(*at)) {
        if let Some(feed) = client
            .fetch_trip_updates()
            .ok()
            .and_then(|bytes| RailRtFeed::decode(&bytes).ok())
        {
            cache.realtime = Some((now, feed));
        }
    }
    (
        cache.alerts.as_ref().map(|(_, a)| a.clone()),
        cache.realtime.as_ref().map(|(_, r)| r.clone()),
    )
}

// ----------------------------------------------------------------------
// HTTP plumbing
// ----------------------------------------------------------------------

type HttpResponse = tiny_http::Response<Cursor<Vec<u8>>>;

fn with_type(body: Vec<u8>, content_type: &str) -> HttpResponse {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
        .expect("valid header");
    tiny_http::Response::from_data(body).with_header(header)
}

fn page(body: String, content_type: &str) -> HttpResponse {
    with_type(body.into_bytes(), content_type)
}

fn json(body: String) -> HttpResponse {
    with_type(body.into_bytes(), "application/json")
}

fn error_json(status: u16, message: &str) -> HttpResponse {
    json(serde_json::json!({ "error": message }).to_string()).with_status_code(status)
}
