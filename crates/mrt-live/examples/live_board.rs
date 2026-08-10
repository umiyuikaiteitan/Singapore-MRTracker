//! Show a live destination board for one station.
//!
//! The example downloads the official GTFS Schedule feed, reads the
//! live alerts and crowd data, and merges everything into one board.
//!
//! Set the account key first:
//!   export LTA_DATAMALL_ACCOUNT_KEY=<your key>
//!
//! Usage:
//!   cargo run -p mrt-live --example live_board -- <station-code> <YYYYMMDD> <HH:MM:SS>

use std::io::Cursor;

use mrt_datamall::{DataMallClient, TrainLine};
use mrt_gtfs::{GtfsFeed, RailNetwork, ZipSource};
use mrt_gtfs_rt::RailRtFeed;
use mrt_live::{match_train_line, LiveBoardBuilder};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [code, date, clock] = args.as_slice() else {
        eprintln!("usage: live_board <station-code> <YYYYMMDD> <HH:MM:SS>");
        std::process::exit(2);
    };

    let client = DataMallClient::from_env().expect("set LTA_DATAMALL_ACCOUNT_KEY first");

    // Static layer: the official GTFS Schedule feed for trains.
    println!("Downloading the GTFS Schedule feed ...");
    let bytes = client
        .fetch_gtfs_schedule()
        .expect("cannot download the feed");
    let mut source = ZipSource::from_reader(Cursor::new(bytes)).expect("bad zip archive");
    let feed = GtfsFeed::load(&mut source).expect("cannot parse the feed");
    let network = RailNetwork::from_feed(&feed).expect("cannot build the network");

    let station = network
        .station_by_code(code)
        .or_else(|| network.station_by_name(code))
        .expect("unknown station");

    // Live layers.
    let alerts = client.train_service_alerts().expect("cannot read alerts");
    let line: Option<TrainLine> = network
        .station(station)
        .lines
        .first()
        .and_then(|&id| match_train_line(network.line(id)));
    let crowd = match line {
        Some(line) => client.platform_crowd(line).unwrap_or_default(),
        None => Vec::new(),
    };
    let realtime = client
        .fetch_trip_updates()
        .ok()
        .and_then(|bytes| RailRtFeed::decode(&bytes).ok())
        .unwrap_or_default();

    let board = LiveBoardBuilder::new(&network)
        .with_alerts(&alerts)
        .with_crowd(&crowd)
        .with_realtime(&realtime)
        .build(station, date.parse().unwrap(), clock.parse().unwrap(), 3600);

    println!(
        "\n{} [{}]",
        board.station_name,
        board.station_codes.join(", ")
    );
    for notice in &board.notices {
        println!("! {notice}");
    }
    for row in &board.rows {
        println!(
            "  {}  {:<4} to {:<20} in {:>4} s  crowd: {:?}{}{}",
            row.clock_time,
            row.line_code,
            row.destination,
            row.departs_in_secs,
            row.crowd,
            row.delay_secs
                .map(|d| format!("  delay: {d} s"))
                .unwrap_or_default(),
            if row.canceled { "  CANCELED" } else { "" },
        );
    }
}
