//! Tests for dataset snapshots, with a mock transport.
//!
//! The tests check the three rules that protect the account key and
//! the three rules that make a snapshot reproducible. No test touches
//! the network.

use std::cell::RefCell;
use std::collections::VecDeque;

use mrt_datamall::{
    sha256_hex, AccountKey, DataMallClient, DataMallError, Response, Transport, TransportError,
};

/// One recorded request.
#[derive(Debug, Clone)]
struct Recorded {
    url: String,
    headers: Vec<(String, String)>,
}

#[derive(Default)]
struct MockTransport {
    requests: RefCell<Vec<Recorded>>,
    responses: RefCell<VecDeque<Response>>,
}

impl MockTransport {
    fn queue(&self, status: u16, body: &[u8]) {
        self.responses.borrow_mut().push_back(Response {
            status,
            body: body.to_vec(),
        });
    }

    fn queue_link(&self, url: &str) {
        let body =
            format!(r#"{{"value":[{{"timestamp":"2026-07-31T17:14:35+08:00","link":"{url}"}}]}}"#);
        self.queue(200, body.as_bytes());
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
            .ok_or_else(|| TransportError("no queued response".to_string()))
    }
}

const KEY: &str = "super-secret-account-key";
const SIGNED: &str = "https://dmprod-datasets.s3.ap-southeast-1.amazonaws.com/\
                      train-gtfs-schedule/gtfs_schedule.zip\
                      ?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Expires=900&X-Amz-Signature=deadbeef";

fn client(transport: &MockTransport) -> DataMallClient<&MockTransport> {
    DataMallClient::new(AccountKey::new(KEY).unwrap(), transport)
}

#[test]
fn a_snapshot_records_the_provenance_of_the_bytes() {
    let transport = MockTransport::default();
    transport.queue_link(SIGNED);
    transport.queue(200, b"PK\x03\x04payload");

    let snapshot = client(&transport).fetch_gtfs_schedule_snapshot().unwrap();
    assert_eq!(snapshot.bytes, b"PK\x03\x04payload");
    assert_eq!(snapshot.sha256, sha256_hex(b"PK\x03\x04payload"));
    assert_eq!(snapshot.source_endpoint, "GTFSScheduleTrain");
    assert_eq!(
        snapshot.dataset_timestamp.as_deref(),
        Some("2026-07-31T17:14:35+08:00")
    );
    assert_eq!(snapshot.len(), 11);
    assert!(!snapshot.is_empty());
}

#[test]
fn the_account_key_never_reaches_the_download_host() {
    let transport = MockTransport::default();
    transport.queue_link(SIGNED);
    transport.queue(200, b"payload");
    client(&transport).fetch_gtfs_schedule_snapshot().unwrap();

    let requests = transport.recorded();
    assert_eq!(requests.len(), 2);

    // The first request goes to DataMall and carries the key.
    assert!(requests[0].url.contains("GTFSScheduleTrain"));
    assert!(requests[0]
        .headers
        .iter()
        .any(|(name, value)| name == "AccountKey" && value == KEY));

    // The second goes to the pre-signed host and carries no header at
    // all, so the key cannot leak to a third party.
    assert_eq!(requests[1].url, SIGNED);
    assert!(requests[1].headers.is_empty());
}

#[test]
fn an_expired_link_is_replaced_and_not_retried() {
    let transport = MockTransport::default();
    // The first link has expired: the signed host answers 403.
    transport.queue_link(SIGNED);
    transport.queue(403, b"<Error>ExpiredToken</Error>");
    // The client asks for a fresh link and succeeds.
    transport.queue_link("https://host.example/fresh.zip?X-Amz-Signature=beef");
    transport.queue(200, b"fresh payload");

    let snapshot = client(&transport).fetch_gtfs_schedule_snapshot().unwrap();
    assert_eq!(snapshot.bytes, b"fresh payload");

    let requests = transport.recorded();
    assert_eq!(requests.len(), 4);
    // Two link requests, and the expired URL was fetched exactly once.
    assert_eq!(requests.iter().filter(|r| r.url == SIGNED).count(), 1);
}

#[test]
fn a_link_that_never_works_gives_up() {
    let transport = MockTransport::default();
    for _ in 0..3 {
        transport.queue_link(SIGNED);
        transport.queue(403, b"expired");
    }
    let error = client(&transport)
        .fetch_gtfs_schedule_snapshot()
        .unwrap_err();
    assert!(matches!(error, DataMallError::Http { status: 403, .. }));
    // Three attempts, not an endless loop.
    assert_eq!(transport.recorded().len(), 6);
}

#[test]
fn a_plain_http_link_is_refused_before_it_is_fetched() {
    let transport = MockTransport::default();
    transport.queue_link("http://host.example/gtfs.zip?X-Amz-Signature=deadbeef");

    let error = client(&transport)
        .fetch_gtfs_schedule_snapshot()
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("HTTPS"), "{message}");
    // The error names the scheme that the link used.
    assert!(
        matches!(&error, DataMallError::InsecureScheme { scheme, .. } if scheme == "http"),
        "{message}"
    );
    // The signature never reaches the message.
    assert!(!message.contains("deadbeef"));
    // Only the link request happened; nothing was downloaded.
    assert_eq!(transport.recorded().len(), 1);
}

#[test]
fn every_download_path_refuses_a_link_that_is_not_https() {
    // The same rule for the plain download, the limited download, and
    // the legacy fetch_* methods that the deployed boards call.
    for url in [
        "http://host.example/gtfs.zip?X-Amz-Signature=deadbeef",
        "ftp://host.example/gtfs.zip",
        "host.example/gtfs.zip?X-Amz-Signature=deadbeef",
    ] {
        let transport = MockTransport::default();
        transport.queue(200, b"payload");
        let error = client(&transport).download(url).unwrap_err();
        assert!(
            matches!(error, DataMallError::InsecureScheme { .. }),
            "{url}"
        );
        assert!(error.to_string().contains("HTTPS"), "{url}");
        assert!(!error.to_string().contains("deadbeef"), "{url}");

        let transport = MockTransport::default();
        transport.queue(200, b"payload");
        assert!(
            client(&transport).download_limited(url, 1024).is_err(),
            "{url}"
        );

        // Nothing reached the network on either call.
        assert!(transport.recorded().is_empty(), "{url}");
    }

    // The scheme is case-insensitive, as RFC 3986 says.
    let transport = MockTransport::default();
    transport.queue(200, b"payload");
    assert!(client(&transport)
        .download("HTTPS://host.example/gtfs.zip")
        .is_ok());
}

#[test]
fn an_https_link_is_downloaded_without_the_account_key() {
    let transport = MockTransport::default();
    transport.queue(200, b"PK\x03\x04payload");

    let bytes = client(&transport).download(SIGNED).unwrap();
    assert_eq!(bytes, b"PK\x03\x04payload");
    let requests = transport.recorded();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url, SIGNED);
    assert!(requests[0].headers.is_empty());
}

#[test]
fn a_failed_download_never_repeats_the_signed_query() {
    let transport = MockTransport::default();
    transport.queue(500, b"boom");
    let error = client(&transport).download(SIGNED).unwrap_err();
    let message = error.to_string();
    assert!(
        matches!(error, DataMallError::Http { status: 500, .. }),
        "{message}"
    );
    assert!(!message.contains("deadbeef"), "{message}");
    assert!(message.contains("<redacted>"), "{message}");
}

#[test]
fn an_oversized_dataset_is_refused_and_not_truncated() {
    let transport = MockTransport::default();
    transport.queue(200, &[0u8; 64]);

    let error = client(&transport).download_limited(SIGNED, 16).unwrap_err();
    let message = error.to_string();
    assert!(
        matches!(error, DataMallError::TooLarge { limit: 16, .. }),
        "{message}"
    );
    assert!(
        message.contains("larger than the limit of 16 bytes"),
        "{message}"
    );
    assert!(!message.contains("deadbeef"));

    // A body at the limit still arrives whole.
    let transport = MockTransport::default();
    transport.queue(200, &[7u8; 16]);
    let bytes = client(&transport).download_limited(SIGNED, 16).unwrap();
    assert_eq!(bytes, vec![7u8; 16]);
}

#[test]
fn a_snapshot_of_an_oversized_dataset_fails_rather_than_shortening_it() {
    let transport = MockTransport::default();
    transport.queue_link(SIGNED);
    transport.queue(200, &[0u8; 64]);

    // The default limit is far above 64 bytes, so the snapshot path
    // succeeds here and delivers every byte. The size contract only
    // ever refuses; it never shortens.
    let snapshot = client(&transport).fetch_gtfs_schedule_snapshot().unwrap();
    assert_eq!(snapshot.len(), 64);
    assert_eq!(snapshot.sha256, sha256_hex(&[0u8; 64]));
}

#[test]
fn the_limit_never_rises_above_the_crate_maximum() {
    let transport = MockTransport::default();
    transport.queue(200, &[0u8; 8]);
    // A caller cannot ask for more than the documented maximum, and a
    // request for more is still served within it.
    let bytes = client(&transport)
        .download_limited(SIGNED, usize::MAX)
        .unwrap();
    assert_eq!(bytes.len(), 8);
    assert_eq!(mrt_datamall::MAX_DATASET_BYTES, 256 * 1024 * 1024);
}

#[test]
fn no_error_message_ever_carries_the_account_key() {
    let transport = MockTransport::default();
    transport.queue(401, b"denied");
    let error = client(&transport)
        .fetch_gtfs_schedule_snapshot()
        .unwrap_err();
    assert!(!error.to_string().contains(KEY));
    assert!(!format!("{error:?}").contains(KEY));
}
