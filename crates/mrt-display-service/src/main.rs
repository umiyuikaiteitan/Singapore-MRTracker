//! LAN bridge: DataMall stays on the host; devices receive only display frames.
use std::error::Error;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mrt_datamall::DataMallClient;
use mrt_display::{build_frame, Frame, FrameOptions, Layout};
use mrt_gtfs::{DirectorySource, GtfsFeed, GtfsTime, RailNetwork, ServiceDate};
use mrt_gtfs_rt::RailRtFeed;

const USAGE: &str = "mrt-display-service --feed <GTFS.zip|directory> --layout <layout.json> [--addr 127.0.0.1:8787] [--offline] [--snapshot [--at <unix-seconds>] [--rt <feed.pb>]]\n\nWithout --offline, LTA_DATAMALL_ACCOUNT_KEY enables host-side trip-update polling.\nGET /v1/frame, /v1/layout, /healthz, and / (preview). --at and --rt require --snapshot.";
const POLL_SECS: u64 = 30;

#[derive(Debug)]
struct Args {
    feed: PathBuf,
    layout: PathBuf,
    addr: String,
    offline: bool,
    snapshot: bool,
    at: Option<u64>,
    rt: Option<PathBuf>,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let (mut feed, mut layout, mut at, mut rt) = (None, None, None, None);
        let (mut offline, mut snapshot) = (false, false);
        let mut addr = "127.0.0.1:8787".to_string();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--offline" => offline = true,
                "--snapshot" => snapshot = true,
                "--feed" => feed = Some(PathBuf::from(args.next().ok_or("missing --feed value")?)),
                "--layout" => {
                    layout = Some(PathBuf::from(args.next().ok_or("missing --layout value")?))
                }
                "--addr" => addr = args.next().ok_or("missing --addr value")?,
                "--at" => {
                    at = Some(
                        args.next()
                            .ok_or("missing --at value")?
                            .parse::<u64>()
                            .map_err(|_| "invalid --at")?,
                    )
                }
                "--rt" => rt = Some(PathBuf::from(args.next().ok_or("missing --rt value")?)),
                _ => return Err(format!("unknown argument: {arg}")),
            }
        }
        if !snapshot && (at.is_some() || rt.is_some()) {
            return Err(
                "--at and --rt require --snapshot; a running display must use current time".into(),
            );
        }
        // Bound to year 9999 so date conversion and downstream arithmetic stay meaningful.
        if at.is_some_and(|at| at > 253_402_271_999) {
            return Err("--at is outside the supported calendar".into());
        }
        Ok(Self {
            feed: feed.ok_or("--feed is required")?,
            layout: layout.ok_or("--layout is required")?,
            addr,
            offline,
            snapshot,
            at,
            rt,
        })
    }
}

struct App {
    network: RailNetwork,
    layout: Layout,
    layout_json: String,
    // Retaining only the calendar avoids duplicating the complete source feed.
    coverage: Vec<(ServiceDate, ServiceDate)>,
}

impl App {
    fn load(feed_path: &Path, layout_path: &Path) -> Result<Self, Box<dyn Error>> {
        let feed = if feed_path.is_dir() {
            GtfsFeed::load(&mut DirectorySource::new(feed_path))?
        } else {
            GtfsFeed::from_zip_path(feed_path)?
        };
        let mut coverage: Vec<_> = feed
            .calendar
            .iter()
            .map(|c| (c.start_date, c.end_date))
            .collect();
        coverage.extend(
            feed.calendar_dates
                .iter()
                .filter(|c| c.exception_type == 1)
                .map(|c| (c.date, c.date)),
        );
        let network = RailNetwork::from_feed(&feed)?;
        let layout_json = std::fs::read_to_string(layout_path)?;
        let layout: Layout = serde_json::from_str(&layout_json)?;
        layout.validate(&network)?;
        for warning in layout.warnings(&network)? {
            eprintln!("Layout: {warning}");
        }
        Ok(Self {
            network,
            layout,
            layout_json,
            coverage,
        })
    }

    fn frame(&self, now: u64, rt: Option<&RailRtFeed>) -> Result<Frame, Box<dyn Error>> {
        let (date, clock) = sgt_from_unix(now);
        // Previous service date can still have trips after 24:00.
        if !self
            .coverage
            .iter()
            .any(|&(start, end)| date >= start && date.previous_day() <= end)
        {
            return Ok(Frame::unavailable(
                &self.layout,
                now,
                45,
                "Schedule outside calendar coverage; refresh the GTFS feed.",
            )?);
        }
        let mut options = FrameOptions::new(now, date, clock);
        options.ttl_secs = 45;
        options.max_rt_age_secs = 120;
        options.max_rows = 6;
        Ok(build_frame(&self.network, &self.layout, rt, &options)?)
    }
}

#[derive(Default)]
struct LiveState {
    feed: Option<RailRtFeed>,
    last_success: Option<u64>,
    last_attempt: Option<u64>,
    last_attempt_ok: bool,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before Unix epoch")
        .as_secs()
}

fn decode_snapshot(bytes: &[u8]) -> Result<RailRtFeed, Box<dyn Error>> {
    let message = mrt_gtfs_rt::decode_feed(bytes)?;
    normalize_snapshot(message)
}

fn normalize_snapshot(
    message: mrt_gtfs_rt::transit_realtime::FeedMessage,
) -> Result<RailRtFeed, Box<dyn Error>> {
    if message.header.incrementality.unwrap_or(0) != 0
        || message.entity.iter().any(|e| e.is_deleted.unwrap_or(false))
    {
        return Err("physical displays require a complete GTFS-Realtime snapshot".into());
    }
    // The flat model cannot preserve modified-trip identities. Do not apply
    // them to an unrelated scheduled trip with a coincidentally equal ID.
    if message
        .entity
        .iter()
        .filter_map(|e| e.trip_update.as_ref())
        .any(|u| !matches!(u.trip.schedule_relationship.unwrap_or(0), 0 | 3))
    {
        return Err("unsupported GTFS-Realtime trip relationship".into());
    }
    Ok(RailRtFeed::from_message(&message))
}

fn sgt_from_unix(now: u64) -> (ServiceDate, GtfsTime) {
    let local = now + 8 * 3600;
    let epoch: ServiceDate = "19700101".parse().expect("epoch date");
    (
        epoch.plus_days((local / 86400) as i64),
        GtfsTime::from_seconds((local % 86400) as u32),
    )
}

fn main() {
    if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return;
    }
    if let Err(error) = run() {
        eprintln!("display service: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse(std::env::args().skip(1)).map_err(|e| format!("{e}\n{USAGE}"))?;
    let app = App::load(&args.feed, &args.layout)?;
    if args.snapshot {
        let rt = args
            .rt
            .map(std::fs::read)
            .transpose()?
            .map(|b| decode_snapshot(&b))
            .transpose()?;
        let frame = app.frame(args.at.unwrap_or_else(unix_now), rt.as_ref())?;
        println!("{}", serde_json::to_string_pretty(&frame)?);
        return Ok(());
    }
    let client = if args.offline {
        None
    } else {
        DataMallClient::from_env().ok()
    };
    let configured = client.is_some();
    let live = Arc::new(Mutex::new(LiveState::default()));
    if let Some(client) = client {
        let live = Arc::clone(&live);
        std::thread::spawn(move || loop {
            // Never hold the lock while making network requests; device requests stay responsive.
            let result = client
                .fetch_trip_updates()
                .ok()
                .and_then(|b| decode_snapshot(&b).ok());
            let mut state = live.lock().expect("live state lock");
            state.last_attempt = Some(unix_now());
            state.last_attempt_ok = result.is_some();
            if let Some(feed) = result {
                state.last_success = state.last_attempt;
                state.feed = Some(feed);
            } else {
                // Do not print upstream URLs (which may contain temporary signed credentials).
                eprintln!("Trip-update refresh failed; freshness rules apply to the cached feed.");
            }
            drop(state);
            std::thread::sleep(Duration::from_secs(POLL_SECS));
        });
    }
    let server = tiny_http::Server::http(&args.addr).map_err(|e| e.to_string())?;
    eprintln!(
        "Physical display on http://{}; live source configured: {configured}",
        args.addr
    );
    let missing_live = RailRtFeed::default();
    for request in server.incoming_requests() {
        let path = request.url().split('?').next().unwrap_or("/");
        let response = if request.method() != &tiny_http::Method::Get {
            response(
                405,
                "application/json",
                br#"{"error":"GET required"}"#.to_vec(),
            )
        } else {
            match path {
                "/" => response(
                    200,
                    "text/html; charset=utf-8",
                    include_bytes!("../assets/index.html").to_vec(),
                ),
                "/v1/layout" => {
                    response(200, "application/json", app.layout_json.as_bytes().to_vec())
                }
                "/v1/frame" => {
                    let state = live.lock().expect("live state lock");
                    // Configured but never fetched: explicitly stale, rather than pretending live is disabled.
                    let rt = state.feed.as_ref().or(if configured {
                        Some(&missing_live)
                    } else {
                        None
                    });
                    match app
                        .frame(unix_now(), rt)
                        .and_then(|f| Ok(serde_json::to_vec(&f)?))
                    {
                        Ok(body) => response(200, "application/json", body),
                        Err(_) => response(
                            503,
                            "application/json",
                            br#"{"error":"frame unavailable"}"#.to_vec(),
                        ),
                    }
                }
                "/healthz" => {
                    let state = live.lock().expect("live state lock");
                    response(
                        200,
                        "application/json",
                        serde_json::to_vec(&serde_json::json!({
                            "status":"ok", "schema_version":1, "live_configured":configured,
                            "last_attempt":state.last_attempt, "last_success":state.last_success,
                            "last_attempt_ok":state.last_attempt_ok,
                            "feed_timestamp":state.feed.as_ref().and_then(|f| f.feed_timestamp)
                        }))?,
                    )
                }
                _ => response(
                    404,
                    "application/json",
                    br#"{"error":"not found"}"#.to_vec(),
                ),
            }
        };
        let _ = request.respond(response);
    }
    Ok(())
}

fn response(status: u16, mime: &str, body: Vec<u8>) -> tiny_http::Response<Cursor<Vec<u8>>> {
    tiny_http::Response::from_data(body)
        .with_status_code(status)
        .with_header(tiny_http::Header::from_bytes("Content-Type", mime).expect("header"))
        .with_header(tiny_http::Header::from_bytes("Cache-Control", "no-store").expect("header"))
        .with_header(
            tiny_http::Header::from_bytes("X-Content-Type-Options", "nosniff").expect("header"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_cannot_freeze_a_server_clock() {
        let args = ["--feed", "x", "--layout", "y", "--at", "42"].map(str::to_string);
        assert!(Args::parse(args)
            .unwrap_err()
            .contains("require --snapshot"));
    }

    #[test]
    fn singapore_midnight_and_extreme_input() {
        let (date, clock) = sgt_from_unix(1_786_377_600);
        assert_eq!(date.to_string(), "20260811");
        assert_eq!(clock.to_string(), "00:00:00");
        let args = [
            "--feed",
            "x",
            "--layout",
            "y",
            "--snapshot",
            "--at",
            "18446744073709551615",
        ]
        .map(str::to_string);
        assert!(Args::parse(args).is_err());
    }

    #[test]
    fn differential_snapshots_and_modified_trips_fail_closed() {
        use mrt_gtfs_rt::transit_realtime as pb;
        let mut message = pb::FeedMessage::default();
        message.header.incrementality = Some(1);
        assert!(normalize_snapshot(message.clone()).is_err());
        message.header.incrementality = Some(0);
        assert!(normalize_snapshot(message.clone()).is_ok());
        message.entity.push(pb::FeedEntity {
            id: "modified".into(),
            trip_update: Some(pb::TripUpdate {
                trip: pb::TripDescriptor {
                    schedule_relationship: Some(6),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        });
        assert!(normalize_snapshot(message).is_err());
    }
}
