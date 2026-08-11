//! Generate a static dot-matrix board site.
//!
//! The generator downloads the official GTFS Schedule feed, computes
//! the departures for every station, and writes a static site that
//! any file host can serve, including GitHub Pages. The browser page
//! computes the wait times from the visitor's clock, so the board
//! stays accurate between site refreshes.
//!
//! Usage:
//!
//! ```sh
//! export LTA_DATAMALL_ACCOUNT_KEY=<your key>
//! cargo run -p mrt-board-static -- <output-dir> [feed.zip]
//! ```
//!
//! Without an account key the generator needs a local feed path and
//! skips the live layer (alerts and crowd levels).
//!
//! Output layout:
//!
//! ```text
//! <output-dir>/
//!   index.html               the board page
//!   .nojekyll                serve files as they are
//!   assets/lta-identity.ttf  the header typeface
//!   data/stations.json       all stations with their codes
//!   data/live.json           alerts and crowd levels, if a key is set
//!   data/board/<CODE>.json   departures per station, POSIX seconds
//! ```

use std::io::Cursor;
use std::path::Path;

use mrt_datamall::DataMallClient;
use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork, ServiceDate, StationId, ZipSource};

/// The offset of Singapore Standard Time from UTC, in seconds.
const SGT_OFFSET_SECS: i64 = 8 * 3600;

/// How far ahead the site carries departures, in seconds.
const HORIZON_SECS: i64 = 26 * 3600;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| {
        eprintln!("usage: mrt-board-static <output-dir> [feed.zip]");
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
            let mut source = ZipSource::from_reader(Cursor::new(bytes)).expect("bad zip archive");
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

    // Step 2: write the static skeleton.
    let data_dir = out.join("data");
    let board_dir = data_dir.join("board");
    std::fs::create_dir_all(out.join("assets")).expect("cannot create the output directory");
    std::fs::create_dir_all(&board_dir).expect("cannot create the data directory");
    std::fs::write(out.join("index.html"), include_str!("../assets/index.html"))
        .expect("cannot write index.html");
    std::fs::write(out.join(".nojekyll"), "").expect("cannot write .nojekyll");
    std::fs::write(
        out.join("assets/lta-identity.ttf"),
        include_bytes!("../../mrt-board-web/assets/lta-identity.ttf"),
    )
    .expect("cannot write the font");

    // Step 3: write the station list.
    let now_unix = unix_now();
    let mut stations: Vec<serde_json::Value> = network
        .stations()
        .iter()
        .filter(|s| !s.codes.is_empty())
        .map(|s| serde_json::json!({ "name": s.name, "codes": s.codes }))
        .collect();
    stations.sort_by_key(|v| v["name"].as_str().unwrap_or("").to_string());
    write_json(
        &data_dir.join("stations.json"),
        &serde_json::json!(stations),
    );

    // Step 4: write one departure file per station code. A station
    // with several codes, for example an interchange, gets one alias
    // file per code with the same content.
    let mut files = 0usize;
    for (index, station) in network.stations().iter().enumerate() {
        if station.codes.is_empty() {
            continue;
        }
        let rows = departures_json(&network, StationId(index), now_unix);
        let body = serde_json::json!({
            "name": station.name,
            "codes": station.codes,
            "stops": station.platform_stop_ids,
            "generated": now_unix,
            "rows": rows,
        });
        for code in &station.codes {
            write_json(&board_dir.join(format!("{code}.json")), &body);
            files += 1;
        }
    }
    eprintln!("Wrote {files} station files.");

    // Step 5: write the live layer, if a key is available.
    let live = match &client {
        Some(client) => mrt_board_static::live_snapshot(client, now_unix),
        None => {
            eprintln!("LTA_DATAMALL_ACCOUNT_KEY is not set; live.json carries no data.");
            serde_json::json!({ "generated": now_unix, "live": false })
        }
    };
    write_json(&data_dir.join("live.json"), &live);

    // Step 6: point the page at the fast-refresh snapshot, when the
    // caller names one. The page falls back to data/live.json.
    let live_url = std::env::var("MRT_DELAYS_URL")
        .ok()
        .filter(|u| !u.is_empty());
    let fallback_url = std::env::var("MRT_DELAYS_FALLBACK_URL")
        .ok()
        .filter(|u| !u.is_empty());
    write_json(
        &data_dir.join("config.json"),
        &serde_json::json!({ "live_url": live_url, "fallback_url": fallback_url }),
    );
    eprintln!("Site ready in {}.", out.display());
}

/// Get the departures of one station as compact rows.
///
/// Each row is `[posix_seconds, line_code, destination, exact]`. The
/// rows cover the time from shortly before now until the horizon.
fn departures_json(
    network: &RailNetwork,
    station: StationId,
    now_unix: i64,
) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    let today = sgt_date(now_unix);
    // A trip that started yesterday can depart after midnight today,
    // so examine both service days over a wide clock window.
    for (date, from, until) in [
        (today.previous_day(), 24 * 3600u32, 32 * 3600u32),
        (today, 0, 32 * 3600),
    ] {
        for dep in network.departures(
            station,
            date,
            GtfsTime::from_seconds(from),
            GtfsTime::from_seconds(until),
        ) {
            let unix = date_to_unix(date) + i64::from(dep.time.seconds()) - SGT_OFFSET_SECS;
            if unix < now_unix - 120 || unix > now_unix + HORIZON_SECS {
                continue;
            }
            let line = network.line(dep.line);
            let destination = dep
                .headsign
                .clone()
                .unwrap_or_else(|| network.station(dep.terminus).name.clone());
            rows.push(serde_json::json!([
                unix,
                line.name,
                destination,
                if dep.exact { 1 } else { 0 },
                dep.trip_id,
            ]));
        }
    }
    rows.sort_by_key(|r| r[0].as_i64().unwrap_or(0));
    rows
}

fn write_json(path: &Path, value: &serde_json::Value) {
    std::fs::write(path, value.to_string()).expect("cannot write a data file");
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs() as i64
}

/// Get the Singapore civil date for a POSIX timestamp.
fn sgt_date(unix: i64) -> ServiceDate {
    let epoch: ServiceDate = "19700101".parse().expect("valid epoch date");
    epoch.plus_days((unix + SGT_OFFSET_SECS).div_euclid(86_400))
}

/// Get the POSIX timestamp of midnight SGT on the given date.
///
/// Adding a GTFS time and subtracting the SGT offset gives the exact
/// instant of a departure.
fn date_to_unix(date: ServiceDate) -> i64 {
    // Days from the epoch, via the civil calendar algorithm by
    // Howard Hinnant (the same one that mrt-gtfs uses internally).
    let y = i64::from(date.year()) - i64::from(date.month() <= 2);
    let m = i64::from(date.month());
    let d = i64::from(date.day());
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_convert_to_posix_midnights() {
        let date: ServiceDate = "19700101".parse().unwrap();
        assert_eq!(date_to_unix(date), 0);
        // 2026-08-11 00:00 UTC.
        let date: ServiceDate = "20260811".parse().unwrap();
        assert_eq!(date_to_unix(date), 1_786_406_400);
    }

    #[test]
    fn sgt_dates_flip_at_sgt_midnight() {
        // 2026-08-10 15:59:59 UTC is 23:59:59 SGT on the same day.
        assert_eq!(sgt_date(1_786_377_599).to_string(), "20260810");
        assert_eq!(sgt_date(1_786_377_600).to_string(), "20260811");
    }

    #[test]
    fn departure_instants_combine_date_and_time() {
        // 08:30:00 SGT on 2026-08-11.
        let date: ServiceDate = "20260811".parse().unwrap();
        let unix = date_to_unix(date) + i64::from(GtfsTime::from_hms(8, 30, 0).seconds())
            - SGT_OFFSET_SECS;
        assert_eq!(unix, 1_786_406_400 + 8 * 3600 + 1800 - 8 * 3600);
    }
}
