//! Decode a GTFS-Realtime file and print a summary.
//!
//! Usage:
//!   cargo run -p mrt-gtfs-rt --example dump_feed -- <feed.pb>

use mrt_gtfs_rt::RailRtFeed;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_feed <feed.pb>");
    let bytes = std::fs::read(&path).expect("cannot read the file");
    let feed = RailRtFeed::decode(&bytes).expect("cannot decode the message");

    println!(
        "Feed timestamp: {:?}. {} trip updates, {} alerts, {} vehicle positions.",
        feed.feed_timestamp,
        feed.trip_updates.len(),
        feed.alerts.len(),
        feed.vehicle_positions.len()
    );
    for update in feed.trip_updates.iter().take(5) {
        println!(
            "  trip {:?} route {:?} delay {:?} stops {}{}",
            update.trip_id,
            update.route_id,
            update.delay_secs,
            update.stop_updates.len(),
            if update.canceled { " CANCELED" } else { "" }
        );
    }
    for alert in feed.alerts.iter().take(5) {
        println!("  alert [{:?}] {:?}", alert.effect, alert.header);
    }
}
