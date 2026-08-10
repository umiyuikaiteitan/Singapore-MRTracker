//! Print a summary of a rail GTFS feed.
//!
//! Usage:
//!   cargo run -p mrt-gtfs --example inspect_feed -- <feed.zip | feed-directory>

use mrt_gtfs::{GtfsFeed, RailNetwork};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: inspect_feed <feed.zip | feed-directory>");

    let feed = if path.ends_with(".zip") {
        GtfsFeed::from_zip_path(&path).expect("cannot load the zip feed")
    } else {
        GtfsFeed::from_dir(&path).expect("cannot load the feed directory")
    };
    println!(
        "Feed: {} stops, {} routes, {} trips, {} stop times",
        feed.stops.len(),
        feed.routes.len(),
        feed.trips.len(),
        feed.stop_times.len()
    );

    let network = RailNetwork::from_feed(&feed).expect("cannot build the rail network");
    println!(
        "Rail network: {} lines, {} stations, {} trips",
        network.lines().len(),
        network.stations().len(),
        network.trip_count()
    );

    for line in network.lines() {
        println!(
            "  line {} ({}), route type {}",
            line.name,
            line.long_name.as_deref().unwrap_or("-"),
            line.route_type
        );
    }

    println!("Interchanges:");
    for id in network.interchanges() {
        let station = network.station(id);
        println!("  {} [{}]", station.name, station.codes.join(", "));
    }
}
