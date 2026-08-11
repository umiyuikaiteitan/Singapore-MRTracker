//! End-to-end tests against the live LTA DataMall API.
//!
//! These tests are marked `#[ignore]` because they need a network
//! connection and an account key. Run them with:
//!
//! ```sh
//! LTA_DATAMALL_ACCOUNT_KEY=<your key> cargo test -p mrt-live -- --ignored
//! ```

use std::io::Cursor;

use mrt_datamall::DataMallClient;
use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork, ServiceDate, ZipSource};
use mrt_gtfs_rt::RailRtFeed;
use mrt_live::LiveBoardBuilder;

fn client() -> DataMallClient<mrt_datamall::UreqTransport> {
    DataMallClient::from_env()
        .expect("set the LTA_DATAMALL_ACCOUNT_KEY environment variable to run the online tests")
}

/// Get the current date and clock time in Singapore (UTC+08:00).
fn sgt_now() -> (ServiceDate, GtfsTime) {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let local = unix + 8 * 3600;
    let epoch: ServiceDate = "19700101".parse().unwrap();
    let date = epoch.plus_days(local.div_euclid(86_400));
    let clock = GtfsTime::from_seconds(local.rem_euclid(86_400) as u32);
    (date, clock)
}

#[test]
#[ignore = "needs the network and an account key"]
fn the_official_feed_builds_a_network_and_a_board() {
    let client = client();

    // Download and parse the official GTFS Schedule feed.
    let bytes = client.fetch_gtfs_schedule().unwrap();
    let mut source = ZipSource::from_reader(Cursor::new(bytes)).unwrap();
    let feed = GtfsFeed::load(&mut source).unwrap();
    let network = RailNetwork::from_feed(&feed).unwrap();

    // The Singapore network has well-known properties.
    assert!(network.stations().len() > 100, "over 100 rail stations");
    assert!(network.lines().len() >= 6, "at least six line entries");
    let jurong_east = network.station_by_code("NS1").expect("NS1 exists");
    assert_eq!(network.station(jurong_east).name, "Jurong East");
    let dhoby_ghaut = network.station_by_code("NS24").expect("NS24 exists");
    assert!(
        network.is_interchange(dhoby_ghaut),
        "Dhoby Ghaut is an interchange"
    );

    // A live board for Jurong East builds without an error. During
    // operating hours it lists departures; the schema holds either
    // way.
    let alerts = client.train_service_alerts().unwrap();
    let (date, clock) = sgt_now();
    let board =
        LiveBoardBuilder::new(&network)
            .with_alerts(&alerts)
            .build(jurong_east, date, clock, 3600);
    assert_eq!(board.station_name, "Jurong East");
    assert!(board.station_codes.contains(&"NS1".to_string()));
    for row in &board.rows {
        assert!(!row.destination.is_empty());
        assert!(row.departs_in_secs <= 3600);
    }
}

#[test]
#[ignore = "needs the network and an account key"]
fn the_realtime_feeds_decode() {
    let client = client();

    let trip_updates = RailRtFeed::decode(&client.fetch_trip_updates().unwrap()).unwrap();
    assert!(trip_updates.feed_timestamp.is_some());
    for update in &trip_updates.trip_updates {
        assert!(update.trip_id.is_some() || update.route_id.is_some());
    }

    let alerts = RailRtFeed::decode(&client.fetch_service_alerts().unwrap()).unwrap();
    assert!(alerts.feed_timestamp.is_some());
}
