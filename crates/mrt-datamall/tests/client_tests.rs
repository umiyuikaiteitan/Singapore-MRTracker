//! Tests for the DataMall client, with a mock transport.
//!
//! The response fixtures come from the official LTA DataMall sample
//! files and from the API User Guide. No test touches the network.

use std::cell::RefCell;
use std::collections::VecDeque;

use mrt_datamall::{
    AccountKey, CrowdLevel, DataMallClient, DataMallError, Response, ServiceStatus, TrainLine,
    Transport, TransportError,
};

/// One recorded request.
#[derive(Debug, Clone)]
struct Recorded {
    url: String,
    headers: Vec<(String, String)>,
}

/// A transport that replays queued responses and records requests.
#[derive(Default)]
struct MockTransport {
    requests: RefCell<Vec<Recorded>>,
    responses: RefCell<VecDeque<Result<Response, TransportError>>>,
}

impl MockTransport {
    fn queue_json(&self, status: u16, body: &str) {
        self.responses.borrow_mut().push_back(Ok(Response {
            status,
            body: body.as_bytes().to_vec(),
        }));
    }

    fn queue_bytes(&self, status: u16, body: &[u8]) {
        self.responses.borrow_mut().push_back(Ok(Response {
            status,
            body: body.to_vec(),
        }));
    }

    fn recorded(&self) -> Vec<Recorded> {
        self.requests.borrow().clone()
    }
}

impl Transport for &MockTransport {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, TransportError> {
        self.requests.borrow_mut().push(Recorded {
            url: url.to_string(),
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        });
        self.responses
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err(TransportError("no queued response".to_string())))
    }
}

fn client(transport: &MockTransport) -> DataMallClient<&MockTransport> {
    DataMallClient::new(AccountKey::new("test-key-123").unwrap(), transport)
}

/// The official PCDRealTime sample, shortened.
const PCD_SAMPLE: &str = r#"{
    "odata.metadata": "http://datamall2.mytransport.sg/ltaodataservice/$metadata#PcdRealTime",
    "value": [
        {
            "Station": "BP11",
            "StartTime": "2021-11-02T13:20:00+08:00",
            "EndTime": "2021-11-02T13:30:00+08:00",
            "CrowdLevel": "l"
        },
        {
            "Station": "BP6",
            "StartTime": "2021-11-02T13:20:00+08:00",
            "EndTime": "2021-11-02T13:30:00+08:00",
            "CrowdLevel": "h"
        },
        {
            "Station": "BP8",
            "StartTime": "2021-11-02T13:20:00+08:00",
            "EndTime": "2021-11-02T13:30:00+08:00",
            "CrowdLevel": "NA"
        }
    ]
}"#;

/// The official GTFSScheduleTrain sample, with a shortened link.
const GTFS_SCHEDULE_SAMPLE: &str = r#"{
    "odata.metadata": "https://datamall2.mytransport.sg/ltaodataservice/GTFSScheduleTrain",
    "value": [
        {
            "timestamp": "2026-07-31T17:14:35+08:00",
            "link": "https://dmprod-datasets.s3.ap-southeast-1.amazonaws.com/train-gtfs-schedule/gtfs_schedule.zip?X-Amz-Signature=abc"
        }
    ]
}"#;

/// A disrupted alert, in the shape of the API User Guide example.
const ALERTS_DISRUPTED: &str = r#"{
    "value": {
        "Status": 2,
        "AffectedSegments": [
            {
                "Line": "NEL",
                "Direction": "HarbourFront",
                "Stations": "NE1,NE3,NE4",
                "FreePublicBus": "NE1,NE3,NE4",
                "FreeMRTShuttle": "NE1,NE3,NE4",
                "MRTShuttleDirection": "HarbourFront"
            }
        ],
        "Message": [
            {
                "Content": "1710hrs: NEL - No train service between NE1 and NE4 stations.",
                "CreatedDate": "2026-01-21 17:17:11"
            }
        ]
    }
}"#;

const ALERTS_NORMAL: &str = r#"{
    "value": {
        "Status": 1,
        "AffectedSegments": [],
        "Message": []
    }
}"#;

#[test]
fn requests_carry_the_account_key_header() {
    let transport = MockTransport::default();
    transport.queue_json(200, ALERTS_NORMAL);
    let client = client(&transport);

    client.train_service_alerts().unwrap();

    let recorded = transport.recorded();
    assert_eq!(
        recorded[0].url,
        "https://datamall2.mytransport.sg/ltaodataservice/TrainServiceAlerts"
    );
    assert!(recorded[0]
        .headers
        .contains(&("AccountKey".to_string(), "test-key-123".to_string())));
    assert!(recorded[0]
        .headers
        .contains(&("accept".to_string(), "application/json".to_string())));
}

#[test]
fn normal_alerts_parse() {
    let transport = MockTransport::default();
    transport.queue_json(200, ALERTS_NORMAL);

    let alerts = client(&transport).train_service_alerts().unwrap();
    assert_eq!(alerts.status, ServiceStatus::Normal);
    assert!(alerts.affected_segments.is_empty());
    assert!(alerts.messages.is_empty());
}

#[test]
fn disrupted_alerts_parse() {
    let transport = MockTransport::default();
    transport.queue_json(200, ALERTS_DISRUPTED);

    let alerts = client(&transport).train_service_alerts().unwrap();
    assert_eq!(alerts.status, ServiceStatus::Disrupted);
    let segment = &alerts.affected_segments[0];
    assert_eq!(segment.train_line(), Some(TrainLine::NEL));
    assert_eq!(segment.station_codes(), vec!["NE1", "NE3", "NE4"]);
    assert_eq!(segment.direction, "HarbourFront");
    assert!(alerts.messages[0].content.contains("No train service"));
}

#[test]
fn platform_crowd_parses_the_official_sample() {
    let transport = MockTransport::default();
    transport.queue_json(200, PCD_SAMPLE);

    let crowd = client(&transport).platform_crowd(TrainLine::BPL).unwrap();
    assert_eq!(
        transport.recorded()[0].url,
        "https://datamall2.mytransport.sg/ltaodataservice/PCDRealTime?TrainLine=BPL"
    );
    assert_eq!(crowd.len(), 3);
    assert_eq!(crowd[0].station, "BP11");
    assert_eq!(crowd[0].crowd_level, CrowdLevel::Low);
    assert_eq!(crowd[1].crowd_level, CrowdLevel::High);
    assert_eq!(crowd[2].crowd_level, CrowdLevel::Unknown);
    assert_eq!(crowd[0].start_time, "2021-11-02T13:20:00+08:00");
}

#[test]
fn gtfs_schedule_link_parses_the_official_sample() {
    let transport = MockTransport::default();
    transport.queue_json(200, GTFS_SCHEDULE_SAMPLE);

    let link = client(&transport).gtfs_schedule_link().unwrap();
    assert_eq!(
        transport.recorded()[0].url,
        "https://datamall2.mytransport.sg/ltaodataservice/GTFSScheduleTrain"
    );
    assert_eq!(link.timestamp.as_deref(), Some("2026-07-31T17:14:35+08:00"));
    assert!(link.url.starts_with("https://dmprod-datasets.s3"));
}

#[test]
fn fetch_gtfs_schedule_downloads_without_the_key() {
    let transport = MockTransport::default();
    transport.queue_json(200, GTFS_SCHEDULE_SAMPLE);
    transport.queue_bytes(200, b"PK\x03\x04fake-zip");

    let bytes = client(&transport).fetch_gtfs_schedule().unwrap();
    assert_eq!(&bytes[..2], b"PK");

    let recorded = transport.recorded();
    assert_eq!(recorded.len(), 2);
    // The pre-signed link carries its own signature. The download
    // request must not leak the account key.
    assert!(recorded[1].url.contains("dmprod-datasets"));
    assert!(recorded[1].headers.is_empty());
}

#[test]
fn empty_link_lists_are_an_error() {
    let transport = MockTransport::default();
    transport.queue_json(200, r#"{"value": []}"#);

    let result = client(&transport).gtfs_schedule_link();
    assert!(matches!(result, Err(DataMallError::NoLink { .. })));
}

#[test]
fn http_401_maps_to_invalid_key() {
    let transport = MockTransport::default();
    transport.queue_json(401, "");
    let result = client(&transport).train_service_alerts();
    assert!(matches!(result, Err(DataMallError::InvalidKey)));
}

#[test]
fn http_429_maps_to_rate_limited() {
    let transport = MockTransport::default();
    transport.queue_json(429, "");
    let result = client(&transport).platform_crowd(TrainLine::NSL);
    assert!(matches!(result, Err(DataMallError::RateLimited)));
}

#[test]
fn other_http_errors_keep_the_status_and_url() {
    let transport = MockTransport::default();
    transport.queue_json(503, "");
    let result = client(&transport).train_service_alerts();
    match result {
        Err(DataMallError::Http { status, url }) => {
            assert_eq!(status, 503);
            assert!(url.ends_with("/TrainServiceAlerts"));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn bad_json_maps_to_decode_error() {
    let transport = MockTransport::default();
    transport.queue_json(200, "this is not json");
    let result = client(&transport).train_service_alerts();
    assert!(matches!(result, Err(DataMallError::Decode { .. })));
}

#[test]
fn custom_base_urls_are_respected() {
    let transport = MockTransport::default();
    transport.queue_json(200, ALERTS_NORMAL);
    let client = client(&transport).with_base_url("http://localhost:9999/api/");

    client.train_service_alerts().unwrap();
    assert_eq!(
        transport.recorded()[0].url,
        "http://localhost:9999/api/TrainServiceAlerts"
    );
}

#[test]
fn forecast_parses_nested_intervals() {
    let transport = MockTransport::default();
    transport.queue_json(
        200,
        r#"{
            "value": [
                {
                    "Date": "2026-08-10T00:00:00+08:00",
                    "Stations": [
                        {
                            "Station": "CC1",
                            "Interval": [
                                {"Start": "2026-08-10T05:30:00+08:00", "CrowdLevel": "l"},
                                {"Start": "2026-08-10T06:00:00+08:00", "CrowdLevel": "m"}
                            ]
                        }
                    ]
                }
            ]
        }"#,
    );

    let days = client(&transport)
        .platform_crowd_forecast(TrainLine::CCL)
        .unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].stations[0].station, "CC1");
    assert_eq!(days[0].stations[0].interval.len(), 2);
    assert_eq!(
        days[0].stations[0].interval[1].crowd_level,
        CrowdLevel::Moderate
    );
}

#[test]
fn passenger_volume_link_uses_the_month_parameter() {
    let transport = MockTransport::default();
    transport.queue_json(
        200,
        r#"{"value": [{"Link": "https://example.org/pv.zip"}]}"#,
    );

    let link = client(&transport)
        .train_passenger_volume_link(Some("202607"))
        .unwrap();
    assert_eq!(
        transport.recorded()[0].url,
        "https://datamall2.mytransport.sg/ltaodataservice/PV/Train?Date=202607"
    );
    assert_eq!(link.url, "https://example.org/pv.zip");
}
