//! A dot-matrix rail destination board in the browser.
//!
//! The server builds the rail network from the official GTFS Schedule
//! feed, refreshes the live layers from LTA DataMall, and serves a
//! RATIS-style dot-matrix board as a single web page.
//!
//! Usage:
//!
//! ```sh
//! export LTA_DATAMALL_ACCOUNT_KEY=<your key>
//! cargo run -p mrt-board-web                        # download the feed
//! cargo run -p mrt-board-web -- gtfs_schedule.zip   # use a local feed
//! ```
//!
//! Then open <http://127.0.0.1:8600>. Without an account key the
//! server still works: the board then shows the static schedule.
//!
//! Endpoints:
//!
//! - `GET /` — the board page.
//! - `GET /api/stations` — all stations with their codes.
//! - `GET /api/board?station=NS1&rows=4` — the live board as JSON.
//!
//! The `station` parameter takes any code of a station, in any
//! spelling: `NS1`, `ns-1`, and `EW24` all name Jurong East.
//!
//! A board response carries a `live` member with the actual state of
//! the upstream DataMall layers — `live`, `stale` with the cache age,
//! `down`, or `off` without an account key — so the page's freshness
//! lamp reflects the data, not the presence of a key.

mod clock;

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use mrt_datamall::{DataMallClient, PlatformCrowd, TrainLine, TrainServiceAlerts, UreqTransport};
use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork, ServiceDate, ZipSource};
use mrt_gtfs_rt::RailRtFeed;
use mrt_live::{match_train_line, LiveBoardBuilder};

/// How long the server reuses fetched live data.
const LIVE_TTL: Duration = Duration::from_secs(20);

/// Cached live data this many times [`LIVE_TTL`] old is stale: the
/// board still serves it, but it no longer counts as live.
const STALE_TTL_MULTIPLE: u32 = 3;

/// Cached live data this many times [`LIVE_TTL`] old carries no live
/// signal at all any more. A layer whose upstream keeps failing for
/// this long counts as down.
const DOWN_TTL_MULTIPLE: u32 = 15;

/// The age at which one layer's cache stops counting as live. See
/// [`STALE_TTL_MULTIPLE`].
const STALE_AFTER: Duration = Duration::from_secs(LIVE_TTL.as_secs() * STALE_TTL_MULTIPLE as u64);

/// The age at which one layer's cache dies. See [`DOWN_TTL_MULTIPLE`].
///
/// # Stale applies, dead does not
///
/// The two thresholds split what a cached layer may still do, and
/// [`upstream_state`] and [`collect_layers`] read that split the same
/// way — the lamp has to describe the data actually on the board:
///
/// - Under [`STALE_AFTER`], with the last fetch successful: the layer
///   is live. It applies, and the lamp says `live`.
/// - Between [`STALE_AFTER`] and [`DOWN_AFTER`], or with a failing
///   upstream at any age under [`DOWN_AFTER`]: the layer is *stale*.
///   It still applies — the operator's most recent statement about a
///   delay or a cancellation is the best the board has, and a minute
///   of aged data beats no data — and the lamp says `stale` with the
///   age, so a reader knows what they are looking at.
/// - Over [`DOWN_AFTER`], or never fetched at all: the layer is
///   *dead*. It contributes nothing to the board: no alert, no trip
///   update, no crowd value. A board that reported `down` while still
///   applying a five-minute-old cancellation would say one thing and
///   show another, and the cancellation is the reading that matters
///   most — it deletes a train the schedule says is coming.
///
/// Each layer expires on its own, because each has its own upstream:
/// one dead layer does not blank the layers still arriving, and one
/// live layer does not revive a dead one.
const DOWN_AFTER: Duration = Duration::from_secs(LIVE_TTL.as_secs() * DOWN_TTL_MULTIPLE as u64);

/// How long the live forwarder reuses one snapshot. DataMall sees at
/// most one snapshot refresh per minute, whatever the request rate.
const FORWARDER_TTL: Duration = Duration::from_secs(60);

/// The default listen address. Override with `MRT_BOARD_ADDR`.
const DEFAULT_ADDR: &str = "127.0.0.1:8600";

/// One cached live layer: the last successful fetch, and whether the
/// latest fetch attempt failed.
struct Layer<T> {
    /// The last successful fetch: when it landed and what it carried.
    data: Option<(Instant, T)>,
    /// `true` when the latest fetch attempt failed. The cached data
    /// stays on screen, but it is no longer live.
    failing: bool,
}

impl<T> Default for Layer<T> {
    fn default() -> Self {
        Layer {
            data: None,
            failing: false,
        }
    }
}

impl<T> Layer<T> {
    /// Report whether the cached data is still within the TTL.
    fn fresh(&self, now: Instant) -> bool {
        self.data
            .as_ref()
            .is_some_and(|(at, _)| now.duration_since(*at) < LIVE_TTL)
    }

    /// Record one fetch attempt: a success replaces the data, a
    /// failure keeps it and marks the layer failing.
    fn record(&mut self, now: Instant, result: Option<T>) {
        match result {
            Some(value) => {
                self.data = Some((now, value));
                self.failing = false;
            }
            None => self.failing = true,
        }
    }

    /// The data this layer contributes to the board, or nothing once
    /// the layer is dead.
    ///
    /// A live layer and a stale one both apply; a layer whose cache has
    /// outlived [`DOWN_AFTER`] applies nothing, so the board never
    /// shows data that [`upstream_state`] has already declared down.
    /// The two read the same threshold, and the boundary belongs to the
    /// living side in both.
    fn applied(&self, now: Instant) -> Option<&T> {
        self.data
            .as_ref()
            .filter(|(at, _)| now.duration_since(*at) <= DOWN_AFTER)
            .map(|(_, value)| value)
    }

    /// The freshness facts of this layer, for the lamp.
    fn health(&self, now: Instant) -> LayerHealth {
        LayerHealth {
            age_secs: self
                .data
                .as_ref()
                .map(|(at, _)| now.duration_since(*at).as_secs()),
            failing: self.failing,
        }
    }
}

/// Cached live data with its fetch times and failure marks.
#[derive(Default)]
struct LiveCache {
    alerts: Layer<TrainServiceAlerts>,
    realtime: Layer<RailRtFeed>,
    rt_alerts: Layer<Vec<mrt_gtfs_rt::Alert>>,
    crowd: HashMap<&'static str, Layer<Vec<PlatformCrowd>>>,
}

struct App {
    network: RailNetwork,
    client: Option<DataMallClient<UreqTransport>>,
    cache: Mutex<LiveCache>,
    /// Origins that may read `/api/live`, from `MRT_ALLOWED_ORIGINS`.
    allowed_origins: Vec<String>,
    /// The cached forwarder snapshot and its fetch time.
    forwarder: Mutex<Option<(Instant, String)>>,
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
        eprintln!("LTA_DATAMALL_ACCOUNT_KEY is not set; the board shows the static schedule.");
    }

    let allowed_origins: Vec<String> = std::env::var("MRT_ALLOWED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .map(|o| o.trim().trim_end_matches('/').to_string())
        .filter(|o| !o.is_empty())
        .collect();
    if allowed_origins.is_empty() {
        eprintln!("MRT_ALLOWED_ORIGINS is not set; /api/live answers 403.");
    }

    let app = App {
        network,
        client,
        cache: Mutex::new(LiveCache::default()),
        allowed_origins,
        forwarder: Mutex::new(None),
    };

    let addr = std::env::var("MRT_BOARD_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let server = tiny_http::Server::http(&addr).expect("cannot bind the listen address");
    eprintln!("Board ready on http://{addr}/");

    for request in server.incoming_requests() {
        let (path, query) = split_query(request.url());
        let response = match path {
            "/" => page(
                include_str!("../assets/index.html"),
                "text/html; charset=utf-8",
            ),
            "/assets/lta-identity.ttf" => bytes(
                include_bytes!("../assets/lta-identity.ttf").to_vec(),
                "font/ttf",
            ),
            "/api/stations" => json(stations_json(&app)),
            "/api/board" => match board_json(&app, &query) {
                Ok(body) => json(body),
                Err(message) => error_json(404, &message),
            },
            "/api/live" => {
                let origin = header_value(&request, "Origin");
                let referer = header_value(&request, "Referer");
                match allowed_origin(&app.allowed_origins, origin.as_deref(), referer.as_deref()) {
                    Some(origin) => {
                        let cors = tiny_http::Header::from_bytes(
                            &b"Access-Control-Allow-Origin"[..],
                            origin.as_bytes(),
                        )
                        .expect("valid header");
                        let vary = tiny_http::Header::from_bytes(&b"Vary"[..], &b"Origin"[..])
                            .expect("valid header");
                        json(forwarder_json(&app))
                            .with_header(cors)
                            .with_header(vary)
                    }
                    None => error_json(403, "origin not allowed"),
                }
            }
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

fn stations_json(app: &App) -> String {
    let mut stations: Vec<_> = app
        .network
        .stations()
        .iter()
        .filter(|s| !s.codes.is_empty())
        .map(|s| serde_json::json!({ "name": s.name, "codes": s.codes }))
        .collect();
    stations.sort_by_key(|v| v["name"].as_str().unwrap_or("").to_string());
    serde_json::json!(stations).to_string()
}

fn board_json(app: &App, query: &HashMap<String, String>) -> Result<String, String> {
    let code = query
        .get("station")
        .ok_or_else(|| "missing station parameter".to_string())?;
    // The parameter carries any code of the station, in any
    // spelling: NS1, ns-1, or EW24. A station name would be
    // ambiguous, because several stations share one.
    let station = app
        .network
        .station_by_alias(code)
        .ok_or_else(|| format!("unknown station \"{code}\""))?;
    let rows: usize = query
        .get("rows")
        .and_then(|r| r.parse().ok())
        .unwrap_or(4)
        .min(12);

    // Refresh the live layers, then build the board.
    let layers = live_layers(app, station);
    let (date, now) = clock::sgt_now();
    let board = build_board(
        &app.network,
        station,
        rows,
        &layers,
        date,
        now,
        clock::unix_now() as u64,
    );
    Ok(serde_json::json!({
        "board": board,
        "date": date,
        "clock": now,
        "live": live_json(app, &layers.health),
    })
    .to_string())
}

/// Merge the live layers this response applies into the board for one
/// station.
///
/// A layer that [`collect_layers`] left out never reaches the builder,
/// so a dead layer decorates nothing.
fn build_board(
    network: &RailNetwork,
    station: mrt_gtfs::StationId,
    rows: usize,
    layers: &LiveLayers,
    date: ServiceDate,
    clock: GtfsTime,
    now_unix: u64,
) -> mrt_live::LiveBoard {
    let mut builder = LiveBoardBuilder::new(network).max_rows(rows);
    if let Some(alerts) = &layers.alerts {
        builder = builder.with_alerts(alerts);
    }
    if let Some(realtime) = &layers.realtime {
        builder = builder.with_realtime(realtime);
    }
    builder
        .with_crowd(&layers.crowd)
        .with_rt_alerts(&layers.rt_alerts, now_unix)
        .build(station, date, clock, 3600)
}

/// The `live` member of a board response: the actual freshness of the
/// upstream layers, not the presence of an account key.
fn live_json(app: &App, health: &[LayerHealth]) -> serde_json::Value {
    if app.client.is_none() {
        return serde_json::json!({ "state": "off" });
    }
    match upstream_state(health) {
        UpstreamState::Live => serde_json::json!({ "state": "live" }),
        UpstreamState::Stale { age_secs } => {
            serde_json::json!({ "state": "stale", "age_secs": age_secs })
        }
        UpstreamState::Down => serde_json::json!({ "state": "down" }),
    }
}

// ----------------------------------------------------------------------
// Upstream freshness
// ----------------------------------------------------------------------

/// The freshness facts of one upstream layer.
#[derive(Debug, Clone, Copy)]
struct LayerHealth {
    /// Seconds since the last successful fetch. `None` when no fetch
    /// has ever succeeded.
    age_secs: Option<u64>,
    /// `true` when the latest fetch attempt failed.
    failing: bool,
}

/// The state of the live data behind one board response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpstreamState {
    /// Every layer holds data from a recent successful fetch.
    Live,
    /// Some layer serves aged cache or has a failing upstream, but
    /// live data is still on the board. `age_secs` is the age of the
    /// oldest cache still served.
    Stale {
        /// Seconds since the oldest served layer was fetched.
        age_secs: u64,
    },
    /// Nothing live remains: no layer ever fetched, or every failing
    /// layer's cache has outlived [`DOWN_TTL_MULTIPLE`].
    Down,
}

/// Decide the lamp state from the freshness of the upstream layers.
///
/// A layer is live while its latest attempt succeeded and its data is
/// at most [`STALE_TTL_MULTIPLE`] TTLs old. A layer with older data,
/// or with a failing upstream, is stale: the board serves its cache
/// and says so. A layer counts as dead once it has no data, or once
/// its data outlives [`DOWN_TTL_MULTIPLE`] TTLs. All layers live is
/// live; all layers dead is down; anything between is stale, aged by
/// the oldest cache still served.
fn upstream_state(layers: &[LayerHealth]) -> UpstreamState {
    let stale_after = STALE_AFTER.as_secs();
    let down_after = DOWN_AFTER.as_secs();
    let mut all_live = true;
    let mut any_alive = false;
    let mut oldest: u64 = 0;
    for layer in layers {
        match layer.age_secs {
            Some(age) if age <= down_after => {
                any_alive = true;
                oldest = oldest.max(age);
                if layer.failing || age > stale_after {
                    all_live = false;
                }
            }
            // Never fetched, or too old to trust at all.
            _ => all_live = false,
        }
    }
    if !any_alive {
        UpstreamState::Down
    } else if all_live {
        UpstreamState::Live
    } else {
        UpstreamState::Stale { age_secs: oldest }
    }
}

/// The live layers behind one board response, with their freshness.
#[derive(Default)]
struct LiveLayers {
    alerts: Option<TrainServiceAlerts>,
    realtime: Option<RailRtFeed>,
    rt_alerts: Vec<mrt_gtfs_rt::Alert>,
    crowd: Vec<PlatformCrowd>,
    /// One entry per layer this response consulted.
    health: Vec<LayerHealth>,
}

/// Get the live layers for one station, from the cache or from the
/// API, recording success and failure per layer.
fn live_layers(app: &App, station: mrt_gtfs::StationId) -> LiveLayers {
    let Some(client) = &app.client else {
        return LiveLayers::default();
    };
    let mut cache = app.cache.lock().expect("cache lock");
    let now = Instant::now();
    // Crowd data comes per line, so this response consults one crowd
    // layer per line of the station.
    let lines: Vec<TrainLine> = app
        .network
        .station(station)
        .lines
        .iter()
        .filter_map(|&id| match_train_line(app.network.line(id)))
        .collect();

    refresh_layers(client, &mut cache, &lines, now);
    collect_layers(&cache, &lines, now)
}

/// Refresh every layer this response consults whose cache has passed
/// [`LIVE_TTL`], recording success and failure per layer.
fn refresh_layers(
    client: &DataMallClient<UreqTransport>,
    cache: &mut LiveCache,
    lines: &[TrainLine],
    now: Instant,
) {
    if !cache.alerts.fresh(now) {
        cache.alerts.record(now, client.train_service_alerts().ok());
    }
    if !cache.realtime.fresh(now) {
        cache.realtime.record(
            now,
            client
                .fetch_trip_updates()
                .ok()
                .and_then(|bytes| RailRtFeed::decode(&bytes).ok()),
        );
    }
    if !cache.rt_alerts.fresh(now) {
        cache.rt_alerts.record(
            now,
            client
                .fetch_service_alerts()
                .ok()
                .and_then(|bytes| RailRtFeed::decode(&bytes).ok())
                .map(|feed| feed.alerts),
        );
    }
    for &line in lines {
        let layer = cache.crowd.entry(line.code()).or_default();
        if !layer.fresh(now) {
            layer.record(now, client.platform_crowd(line).ok());
        }
    }
}

/// Assemble the layers this response applies, out of the cache.
///
/// Every layer expires on its own, at [`DOWN_AFTER`]: a dead layer
/// contributes nothing, whatever the others hold. That is the same
/// threshold [`upstream_state`] reads, so the board never applies a
/// delay or a cancellation from data the lamp has already called down.
///
/// [`LiveLayers::health`] still carries one entry per layer consulted,
/// dead ones included: an outage is exactly what the lamp has to
/// report.
fn collect_layers(cache: &LiveCache, lines: &[TrainLine], now: Instant) -> LiveLayers {
    let mut health = vec![
        cache.alerts.health(now),
        cache.realtime.health(now),
        cache.rt_alerts.health(now),
    ];

    let missing: Layer<Vec<PlatformCrowd>> = Layer::default();
    let mut crowd = Vec::new();
    for &line in lines {
        let layer = cache.crowd.get(line.code()).unwrap_or(&missing);
        if let Some(records) = layer.applied(now) {
            crowd.extend(records.iter().cloned());
        }
        health.push(layer.health(now));
    }

    LiveLayers {
        alerts: cache.alerts.applied(now).cloned(),
        realtime: cache.realtime.applied(now).cloned(),
        rt_alerts: cache.rt_alerts.applied(now).cloned().unwrap_or_default(),
        crowd,
        health,
    }
}

/// Get one request header value.
fn header_value(request: &tiny_http::Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

/// Check the request origin against the allowlist.
///
/// The function accepts a matching `Origin` header, or a `Referer`
/// that starts with an allowed origin. It returns the origin value
/// for the `Access-Control-Allow-Origin` response header.
///
/// Browsers enforce this check; command-line clients can forge the
/// headers. The forwarder therefore protects the DataMall quota and
/// deters casual reuse, not determined actors.
fn allowed_origin(
    allowlist: &[String],
    origin: Option<&str>,
    referer: Option<&str>,
) -> Option<String> {
    if let Some(origin) = origin {
        let origin = origin.trim_end_matches('/');
        return allowlist
            .iter()
            .find(|allowed| allowed.eq_ignore_ascii_case(origin))
            .cloned();
    }
    if let Some(referer) = referer {
        return allowlist
            .iter()
            .find(|allowed| {
                referer.len() > allowed.len()
                    && referer[..allowed.len()].eq_ignore_ascii_case(allowed)
                    && referer.as_bytes().get(allowed.len()) == Some(&b'/')
            })
            .cloned();
    }
    None
}

/// Get the cached live snapshot, or fetch a fresh one after the TTL.
fn forwarder_json(app: &App) -> String {
    let mut cache = app.forwarder.lock().expect("forwarder lock");
    if let Some((at, body)) = cache.as_ref() {
        if at.elapsed() < FORWARDER_TTL {
            return body.clone();
        }
    }
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs() as i64;
    let snapshot = match &app.client {
        Some(client) => mrt_board_static::live_snapshot(client, now_unix),
        None => serde_json::json!({ "generated": now_unix, "live": false }),
    };
    let body = snapshot.to_string();
    *cache = Some((Instant::now(), body.clone()));
    body
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

fn page(body: &str, content_type: &str) -> HttpResponse {
    with_type(body.as_bytes().to_vec(), content_type)
}

fn bytes(body: Vec<u8>, content_type: &str) -> HttpResponse {
    with_type(body, content_type)
}

fn json(body: String) -> HttpResponse {
    with_type(body.into_bytes(), "application/json")
}

fn error_json(status: u16, message: &str) -> HttpResponse {
    json(serde_json::json!({ "error": message }).to_string()).with_status_code(status)
}

/// Split a request URL into the path and the decoded query pairs.
fn split_query(url: &str) -> (&str, HashMap<String, String>) {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let mut pairs = HashMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        pairs.insert(percent_decode(key), percent_decode(value));
    }
    (path, pairs)
}

/// Decode percent escapes and `+` in a query component.
fn percent_decode(component: &str) -> String {
    let bytes = component.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_split_into_pairs() {
        let (path, query) = split_query("/api/board?station=NS1&rows=4");
        assert_eq!(path, "/api/board");
        assert_eq!(query["station"], "NS1");
        assert_eq!(query["rows"], "4");
    }

    #[test]
    fn queries_are_optional() {
        let (path, query) = split_query("/api/stations");
        assert_eq!(path, "/api/stations");
        assert!(query.is_empty());
    }

    #[test]
    fn query_values_are_decoded() {
        let (_, query) = split_query("/api/board?station=Jurong+East&x=a%20b");
        assert_eq!(query["station"], "Jurong East");
        assert_eq!(query["x"], "a b");
    }

    #[test]
    fn broken_percent_escapes_stay_literal() {
        let (_, query) = split_query("/x?a=100%&b=%zz");
        assert_eq!(query["a"], "100%");
        assert_eq!(query["b"], "%zz");
    }

    fn ok(age_secs: u64) -> LayerHealth {
        LayerHealth {
            age_secs: Some(age_secs),
            failing: false,
        }
    }

    fn failing(age_secs: Option<u64>) -> LayerHealth {
        LayerHealth {
            age_secs,
            failing: true,
        }
    }

    #[test]
    fn fresh_successful_layers_are_live() {
        assert_eq!(
            upstream_state(&[ok(0), ok(15), ok(59)]),
            UpstreamState::Live
        );
    }

    #[test]
    fn a_failing_upstream_is_not_live() {
        // The cache is seconds old, but the latest attempt failed:
        // the board serves cache and must say so.
        assert_eq!(
            upstream_state(&[ok(5), failing(Some(25)), ok(5)]),
            UpstreamState::Stale { age_secs: 25 }
        );
    }

    #[test]
    fn aged_cache_is_stale_even_without_a_failure() {
        // Data beyond STALE_TTL_MULTIPLE * LIVE_TTL (60 s) is no
        // longer live, and the state carries the oldest age.
        assert_eq!(
            upstream_state(&[ok(10), ok(120)]),
            UpstreamState::Stale { age_secs: 120 }
        );
    }

    #[test]
    fn a_layer_that_never_fetched_is_not_live() {
        assert_eq!(
            upstream_state(&[ok(5), failing(None)]),
            UpstreamState::Stale { age_secs: 5 }
        );
    }

    #[test]
    fn nothing_ever_fetched_is_down() {
        assert_eq!(
            upstream_state(&[failing(None), failing(None)]),
            UpstreamState::Down
        );
        assert_eq!(upstream_state(&[]), UpstreamState::Down);
    }

    #[test]
    fn a_long_outage_turns_the_lamp_down() {
        // Beyond DOWN_TTL_MULTIPLE * LIVE_TTL (300 s) the cache
        // carries no live signal at all any more.
        assert_eq!(
            upstream_state(&[failing(Some(301)), failing(Some(400))]),
            UpstreamState::Down
        );
        // One layer still inside the horizon keeps the board stale
        // rather than down.
        assert_eq!(
            upstream_state(&[failing(Some(301)), failing(Some(200))]),
            UpstreamState::Stale { age_secs: 200 }
        );
    }

    // ------------------------------------------------------------------
    // A dead layer reaches no board
    // ------------------------------------------------------------------

    /// A one-line network: three North South Line stations, daily
    /// service, one trip calling NS4 at 08:10.
    fn tiny_network() -> RailNetwork {
        use mrt_gtfs::{Calendar, Route, Stop, StopTime, Trip};

        let stop = |id: &str, code: &str, name: &str| Stop {
            stop_id: id.to_string(),
            stop_code: Some(code.to_string()),
            stop_name: Some(name.to_string()),
            location_type: Some(0),
            ..Default::default()
        };
        let stop_time = |time: &str, at: &str, seq: u32| StopTime {
            trip_id: "T1".to_string(),
            arrival_time: Some(time.parse().unwrap()),
            departure_time: Some(time.parse().unwrap()),
            stop_id: at.to_string(),
            stop_sequence: seq,
            ..Default::default()
        };
        let feed = GtfsFeed {
            stops: vec![
                stop("S_NS1", "NS1", "Jurong East"),
                stop("S_NS4", "NS4", "Choa Chu Kang"),
                stop("S_NS27", "NS27", "Marina Bay"),
            ],
            routes: vec![Route {
                route_id: "NS".to_string(),
                agency_id: None,
                route_short_name: Some("NSL".to_string()),
                route_long_name: Some("North South Line".to_string()),
                route_type: 1,
                route_color: None,
                route_text_color: None,
            }],
            trips: vec![Trip {
                route_id: "NS".to_string(),
                service_id: "DAILY".to_string(),
                trip_id: "T1".to_string(),
                trip_headsign: Some("Marina Bay".to_string()),
                direction_id: Some(0),
                ..Default::default()
            }],
            stop_times: vec![
                stop_time("08:00:00", "S_NS1", 1),
                stop_time("08:10:00", "S_NS4", 2),
                stop_time("08:30:00", "S_NS27", 3),
            ],
            calendar: vec![Calendar {
                service_id: "DAILY".to_string(),
                monday: 1,
                tuesday: 1,
                wednesday: 1,
                thursday: 1,
                friday: 1,
                saturday: 1,
                sunday: 1,
                start_date: "20250101".parse().unwrap(),
                end_date: "20271231".parse().unwrap(),
            }],
            ..Default::default()
        };
        RailNetwork::from_feed(&feed).expect("the test feed builds")
    }

    /// A cache whose realtime layer landed at `at`, carrying the
    /// operator's cancellation of T1, with the upstream failing since.
    ///
    /// The crowd layer of the NSL landed at the same instant.
    fn cache_fetched_at(at: Instant) -> LiveCache {
        let mut cache = LiveCache::default();
        cache.realtime.data = Some((
            at,
            RailRtFeed {
                trip_updates: vec![mrt_gtfs_rt::TripUpdate {
                    trip_id: Some("T1".to_string()),
                    canceled: true,
                    ..Default::default()
                }],
                ..Default::default()
            },
        ));
        cache.realtime.failing = true;
        let crowd = cache.crowd.entry(TrainLine::NSL.code()).or_default();
        crowd.data = Some((
            at,
            vec![PlatformCrowd {
                station: "NS4".to_string(),
                start_time: String::new(),
                end_time: String::new(),
                crowd_level: mrt_datamall::CrowdLevel::High,
            }],
        ));
        crowd.failing = true;
        cache
    }

    /// The first board row for Choa Chu Kang, built from the given
    /// layers exactly as a board response builds it.
    fn first_row(network: &RailNetwork, layers: &LiveLayers) -> mrt_live::LiveBoardRow {
        let station = network.station_by_code("NS4").expect("NS4 exists");
        let board = build_board(
            network,
            station,
            4,
            layers,
            "20260810".parse().unwrap(),
            "08:00:00".parse().unwrap(),
            1_000,
        );
        board.rows.into_iter().next().expect("one scheduled row")
    }

    #[test]
    fn a_stale_layer_still_applies_to_the_board() {
        let network = tiny_network();
        let at = Instant::now();
        // One second inside the horizon: the lamp says stale, and the
        // operator's cancellation is still the best the board has.
        let layers = collect_layers(
            &cache_fetched_at(at),
            &[TrainLine::NSL],
            at + DOWN_AFTER - Duration::from_secs(1),
        );
        assert!(matches!(
            upstream_state(&layers.health),
            UpstreamState::Stale { .. }
        ));
        assert!(first_row(&network, &layers).canceled);
    }

    #[test]
    fn an_old_cancellation_leaves_the_board_once_its_layer_is_down() {
        let network = tiny_network();
        let at = Instant::now();
        // One second past the horizon: the lamp says down, so the
        // cancellation must be off the board too. A response that
        // reported `down` and still deleted this train would be
        // showing data it had just declared unusable.
        let layers = collect_layers(
            &cache_fetched_at(at),
            &[TrainLine::NSL],
            at + DOWN_AFTER + Duration::from_secs(1),
        );
        assert_eq!(upstream_state(&layers.health), UpstreamState::Down);
        assert!(layers.realtime.is_none());
        assert!(!first_row(&network, &layers).canceled);
    }

    #[test]
    fn every_layer_expires_at_the_down_threshold() {
        let at = Instant::now();
        let cache = cache_fetched_at(at);
        let lines = [TrainLine::NSL];

        // The boundary belongs to the living side, exactly as
        // upstream_state reads it.
        let alive = collect_layers(&cache, &lines, at + DOWN_AFTER);
        assert!(alive.realtime.is_some());
        assert_eq!(alive.crowd.len(), 1);

        let dead = collect_layers(&cache, &lines, at + DOWN_AFTER + Duration::from_secs(1));
        assert!(dead.realtime.is_none());
        assert!(dead.crowd.is_empty());
        // The lamp still reports every layer this response consulted,
        // so an outage is visible rather than silent.
        assert_eq!(dead.health.len(), 4);
    }

    #[test]
    fn layers_expire_one_by_one() {
        // Each layer has its own upstream, so a dead trip-update layer
        // must not blank a crowd layer that is still arriving.
        let at = Instant::now();
        let mut cache = cache_fetched_at(at);
        let now = at + DOWN_AFTER + Duration::from_secs(1);
        cache
            .crowd
            .entry(TrainLine::NSL.code())
            .or_default()
            .record(now, Some(Vec::new()));

        let layers = collect_layers(&cache, &[TrainLine::NSL], now);
        assert!(layers.realtime.is_none());
        assert!(matches!(
            upstream_state(&layers.health),
            UpstreamState::Stale { .. }
        ));
    }

    #[test]
    fn origins_match_the_allowlist() {
        let allow = vec!["https://example.github.io".to_string()];

        // A matching Origin header passes, in any case.
        assert_eq!(
            allowed_origin(&allow, Some("https://example.github.io"), None).as_deref(),
            Some("https://example.github.io")
        );
        assert!(allowed_origin(&allow, Some("HTTPS://EXAMPLE.GITHUB.IO"), None).is_some());
        // A trailing slash on the Origin value is tolerated.
        assert!(allowed_origin(&allow, Some("https://example.github.io/"), None).is_some());

        // Other origins fail, including prefix tricks.
        assert!(allowed_origin(&allow, Some("https://evil.example"), None).is_none());
        assert!(
            allowed_origin(&allow, Some("https://example.github.io.evil.example"), None).is_none()
        );

        // A Referer passes only with a path boundary.
        assert!(allowed_origin(&allow, None, Some("https://example.github.io/repo/")).is_some());
        assert!(allowed_origin(
            &allow,
            None,
            Some("https://example.github.io.evil.example/")
        )
        .is_none());

        // No headers, or an empty allowlist, fail.
        assert!(allowed_origin(&allow, None, None).is_none());
        assert!(allowed_origin(&[], Some("https://example.github.io"), None).is_none());
    }
}
