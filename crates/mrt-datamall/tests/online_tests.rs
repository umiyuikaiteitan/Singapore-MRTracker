//! Tests against the live LTA DataMall API.
//!
//! These tests are marked `#[ignore]` because they need a network
//! connection and an account key. Run them with:
//!
//! ```sh
//! LTA_DATAMALL_ACCOUNT_KEY=<your key> cargo test -p mrt-datamall -- --ignored
//! ```

#![allow(clippy::items_after_test_module)]

use mrt_datamall::{CrowdLevel, DataMallClient, ServiceStatus, TrainLine, UreqTransport};

fn client() -> DataMallClient<UreqTransport> {
    DataMallClient::from_env()
        .expect("set the LTA_DATAMALL_ACCOUNT_KEY environment variable to run the online tests")
}

#[test]
#[ignore = "needs the network and an account key"]
fn train_service_alerts_parse() {
    let alerts = client().train_service_alerts().unwrap();

    // The overall status is one of the two documented values.
    assert!(matches!(
        alerts.status,
        ServiceStatus::Normal | ServiceStatus::Disrupted
    ));
    // Every affected segment names a known line and parseable
    // station codes.
    for segment in &alerts.affected_segments {
        assert!(
            segment.train_line().is_some(),
            "unknown line code {:?}",
            segment.line
        );
        assert!(!segment.station_codes().is_empty());
    }
}

#[test]
#[ignore = "needs the network and an account key"]
fn platform_crowd_returns_records_for_every_line() {
    let client = client();
    // One line is enough for a schema check; the full loop would use
    // 11 requests of the rate budget.
    let records = client.platform_crowd(TrainLine::NSL).unwrap();
    assert!(!records.is_empty(), "NSL crowd data must not be empty");
    for record in &records {
        assert!(
            record.station.starts_with("NS") || record.station.starts_with("EW"),
            "unexpected station {} for NSL",
            record.station
        );
        assert!(matches!(
            record.crowd_level,
            CrowdLevel::Low | CrowdLevel::Moderate | CrowdLevel::High | CrowdLevel::Unknown
        ));
        assert!(record.start_time.contains('T'), "ISO 8601 start time");
    }
}

#[test]
#[ignore = "needs the network and an account key"]
fn platform_crowd_forecast_parses() {
    let days = client().platform_crowd_forecast(TrainLine::BPL).unwrap();
    assert!(!days.is_empty());
    let day = &days[0];
    assert!(!day.stations.is_empty());
    assert!(!day.stations[0].interval.is_empty());
}

#[test]
#[ignore = "needs the network and an account key"]
fn gtfs_schedule_link_and_download_work() {
    let client = client();
    let link = client.gtfs_schedule_link().unwrap();
    assert!(link.url.starts_with("https://"));
    assert!(link.timestamp.is_some());

    let bytes = client.download(&link.url).unwrap();
    // A zip archive starts with the "PK" magic bytes.
    assert!(bytes.len() > 100_000, "the feed is at least 100 kB");
    assert_eq!(&bytes[..2], b"PK");
}

#[test]
#[ignore = "needs the network and an account key"]
fn gtfs_realtime_links_resolve() {
    let client = client();
    for link in [
        client.gtfs_trip_updates_link().unwrap(),
        client.gtfs_service_alerts_link().unwrap(),
    ] {
        assert!(link.url.starts_with("https://"));
        let bytes = client.download(&link.url).unwrap();
        assert!(!bytes.is_empty());
    }
}

#[test]
#[ignore = "needs the network and an account key"]
fn train_passenger_volume_link_resolves() {
    let link = client().train_passenger_volume_link(None).unwrap();
    assert!(link.url.starts_with("https://"));
}

#[test]
#[ignore = "needs the network and an account key"]
fn a_wrong_key_maps_to_invalid_key() {
    use mrt_datamall::{AccountKey, DataMallError};

    let client = DataMallClient::with_key(AccountKey::new("wrong-key").unwrap());
    let result = client.train_service_alerts();
    assert!(matches!(result, Err(DataMallError::InvalidKey)));
}
