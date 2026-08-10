//! Print a destination board for one station.
//!
//! Usage:
//!   cargo run -p mrt-gtfs --example departure_board -- <feed> <station-code> <YYYYMMDD> <HH:MM:SS>
//!
//! Example:
//!   cargo run -p mrt-gtfs --example departure_board -- data/gtfs_schedule.zip NS1 20260810 08:00:00

use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork, ServiceDate};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, code, date, clock] = args.as_slice() else {
        eprintln!("usage: departure_board <feed> <station-code> <YYYYMMDD> <HH:MM:SS>");
        std::process::exit(2);
    };

    let feed = if path.ends_with(".zip") {
        GtfsFeed::from_zip_path(path).expect("cannot load the zip feed")
    } else {
        GtfsFeed::from_dir(path).expect("cannot load the feed directory")
    };
    let network = RailNetwork::from_feed(&feed).expect("cannot build the rail network");

    let station = network
        .station_by_code(code)
        .or_else(|| network.station_by_name(code))
        .expect("unknown station");
    let date: ServiceDate = date.parse().expect("bad date");
    let clock: GtfsTime = clock.parse().expect("bad time");

    let station_data = network.station(station);
    println!(
        "Departures at {} [{}] on {} from {}:",
        station_data.name,
        station_data.codes.join(", "),
        date,
        clock
    );
    for entry in network.departure_board(station, date, clock, 3600) {
        let departure = &entry.departure;
        let line = network.line(departure.line);
        let terminus = network.station(departure.terminus);
        println!(
            "  {}  {:<4} to {:<20} in {:>4} s{}",
            entry.clock_time(),
            line.name,
            departure.headsign.as_deref().unwrap_or(&terminus.name),
            entry.wait_secs,
            if departure.exact {
                ""
            } else {
                " (approximate)"
            }
        );
    }
}
