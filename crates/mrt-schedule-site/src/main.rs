//! Generate the browsable timetable and diagram site.
//!
//! ```sh
//! # From a local feed.
//! cargo run --release -p mrt-schedule-site -- site/timetables data/gtfs_schedule.zip
//!
//! # From DataMall, with the key in the environment.
//! export LTA_DATAMALL_ACCOUNT_KEY=<your key>
//! cargo run --release -p mrt-schedule-site -- site/timetables
//! ```
//!
//! Options come from the environment, so the Pages workflow can set
//! them without a shell quoting dance:
//!
//! | Variable | Meaning | Default |
//! |----------|---------|---------|
//! | `MRT_SITE_DAYS` | How many service dates to cover | `3` |
//! | `MRT_SITE_CONFIG` | A publication configuration file | none |
//! | `MRT_SITE_TITLE` | The name in the masthead | `Singapore rail timetables` |
//! | `MRT_SITE_BOARD_HREF` | Relative link back to the board | `../index.html` |
//! | `MRT_SITE_LINES` | Only these route identifiers, comma separated | all |
//! | `MRT_SITE_ALLOW_PARTIAL` | `1` accepts a build with failed pages; the hubs omit them | unset: fail |
//!
//! A page that cannot be built is dropped from every hub and listed on
//! standard error, and the run exits with code 7 — unless
//! `MRT_SITE_ALLOW_PARTIAL=1` explicitly accepts the partial site.

use std::io::Cursor;
use std::path::PathBuf;

use mrt_datamall::{sha256_hex, DataMallClient};
use mrt_gtfs::{GtfsFeed, RailNetwork, ZipSource};
use mrt_publication::{DocumentSeed, PublicationConfig};
use mrt_schedule_site::{
    default_windows, today_at_offset, SiteBuild, SiteInfo, SitePlan, SGT_OFFSET_SECS,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next() else {
        eprintln!("usage: mrt-schedule-site <output-dir> [feed.zip]");
        std::process::exit(2);
    };
    let out = PathBuf::from(out);
    let feed_path = args.next();

    // Step 1: the feed, from a local archive or from DataMall.
    let (bytes, timestamp) = match &feed_path {
        Some(path) => {
            eprintln!("Reading the GTFS Schedule feed from {path} ...");
            let bytes = std::fs::read(path).unwrap_or_else(|e| {
                eprintln!("error: cannot read {path}: {e}");
                std::process::exit(3);
            });
            (bytes, None)
        }
        None => {
            eprintln!("Downloading the GTFS Schedule feed from DataMall ...");
            let client = DataMallClient::from_env().unwrap_or_else(|e| {
                eprintln!("error: {e}; set the key, or pass a path to a GTFS zip archive");
                std::process::exit(3);
            });
            let snapshot = client.fetch_gtfs_schedule_snapshot().unwrap_or_else(|e| {
                eprintln!("error: cannot download the feed: {e}");
                std::process::exit(3);
            });
            (snapshot.bytes, snapshot.dataset_timestamp)
        }
    };
    let feed_sha256 = sha256_hex(&bytes);

    let mut source = ZipSource::from_reader(Cursor::new(bytes)).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(4);
    });
    let feed = GtfsFeed::load(&mut source).unwrap_or_else(|e| {
        eprintln!("error: cannot parse the feed: {e}");
        std::process::exit(4);
    });
    let network = RailNetwork::from_feed(&feed).unwrap_or_else(|e| {
        eprintln!("error: cannot build the rail network: {e}");
        std::process::exit(4);
    });

    // Step 2: the configuration.
    let config = match std::env::var("MRT_SITE_CONFIG")
        .ok()
        .filter(|p| !p.is_empty())
    {
        Some(path) => load_config(&path),
        None => PublicationConfig::default(),
    };
    let config_sha = sha256_hex(
        serde_json::to_string(&config)
            .unwrap_or_default()
            .as_bytes(),
    );
    let seed = DocumentSeed {
        generator_version: format!("mrt-schedule-site {}", env!("CARGO_PKG_VERSION")),
        feed_sha256,
        feed_timestamp: timestamp,
        timezone: config
            .timezone
            .clone()
            .or_else(|| mrt_gtfs::feed_timezone(&feed).map(str::to_string))
            .unwrap_or_else(|| "Asia/Singapore".to_string()),
        generated_from_cache: false,
        configuration_sha256: config_sha,
    };

    // Step 3: the plan.
    let days: u32 = std::env::var("MRT_SITE_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let today = today_at_offset(unix_now(), SGT_OFFSET_SECS);
    let mut plan = SitePlan::build(&network, today, days, default_windows());

    if let Some(only) = std::env::var("MRT_SITE_LINES")
        .ok()
        .filter(|v| !v.is_empty())
    {
        let wanted: Vec<String> = only.split(',').map(|s| s.trim().to_string()).collect();
        plan.lines.retain(|line| {
            wanted
                .iter()
                .any(|w| w == &line.route_id || w == &line.name)
        });
    }

    let info = SiteInfo {
        title: std::env::var("MRT_SITE_TITLE")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| SiteInfo::default().title),
        board_href: std::env::var("MRT_SITE_BOARD_HREF")
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| SiteInfo::default().board_href),
        board_label: SiteInfo::default().board_label,
    };

    eprintln!(
        "Building {} pages: {} stations and {} lines over {} day(s).",
        plan.page_count(),
        plan.stations.len(),
        plan.lines.len(),
        plan.dates.len()
    );

    // Step 4: write everything.
    let build = SiteBuild {
        network: &network,
        config: &config,
        seed: &seed,
        info: &info,
        plan: &plan,
    };
    let report = build.write(&out).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(7);
    });

    eprintln!(
        "Wrote {} files ({:.1} MiB) into {}.",
        report.files,
        report.bytes as f64 / (1024.0 * 1024.0),
        out.display()
    );
    // A site with no pages is a failure, however cleanly it ran, and
    // no opt-in makes it acceptable.
    if report.files == 0 {
        eprintln!("error: the site is empty");
        std::process::exit(7);
    }
    // A build with failed pages must not look like success. The hubs
    // already omit the missing pages, so what was written is
    // self-consistent, but only an explicit opt-in deploys it.
    if !report.failures.is_empty() {
        for failure in report.failures.iter().take(20) {
            eprintln!("warning: {failure}");
        }
        if report.failures.len() > 20 {
            eprintln!("warning: and {} more.", report.failures.len() - 20);
        }
        eprintln!(
            "warning: {} planned page(s) are missing; no hub links to them",
            report.missing.len()
        );
        if mrt_schedule_site::accepts_partial(
            std::env::var("MRT_SITE_ALLOW_PARTIAL").ok().as_deref(),
        ) {
            eprintln!("warning: MRT_SITE_ALLOW_PARTIAL=1 is set, so the partial site is accepted");
        } else {
            eprintln!(
                "error: the site is incomplete; \
                 set MRT_SITE_ALLOW_PARTIAL=1 to accept a partial site"
            );
            std::process::exit(7);
        }
    }
}

fn load_config(path: &str) -> PublicationConfig {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {path}: {e}");
        std::process::exit(2);
    });
    let value = mrt_schedule_cli::yaml::parse(&text).unwrap_or_else(|e| {
        eprintln!("error: {path}: {e}");
        std::process::exit(2);
    });
    let config: PublicationConfig = serde_json::from_value(value).unwrap_or_else(|e| {
        eprintln!("error: {path}: {e}");
        std::process::exit(2);
    });
    if let Err(message) = config.check() {
        eprintln!("error: {path}: {message}");
        std::process::exit(2);
    }
    config
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
