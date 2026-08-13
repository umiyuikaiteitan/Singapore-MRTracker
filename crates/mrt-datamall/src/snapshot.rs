//! Dataset snapshots.
//!
//! A DataMall dataset endpoint does not return the file. It returns a
//! pre-signed link that expires after about fifteen minutes. The
//! snapshot type wraps the whole exchange — request a link, download
//! it at once, fingerprint the bytes — and records enough provenance
//! for a generated document to say where its data came from.
//!
//! # Rules that this module enforces
//!
//! - The account key travels only in the `AccountKey` header of a
//!   request to the DataMall host. The pre-signed link carries its own
//!   signature, and the download therefore sends no key.
//! - A download link must use HTTPS.
//! - An expired link is not retried. The client asks for a new link,
//!   a bounded number of times.
//! - Nothing that this module writes to a log or a manifest contains
//!   the key or the signed query parameters. Use [`redact_url`].

use std::time::SystemTime;

use crate::client::DataMallClient;
use crate::error::DataMallError;
use crate::transport::Transport;

/// The bytes of one dataset, with their provenance.
#[derive(Clone)]
pub struct DataMallSnapshot {
    /// The timestamp that DataMall reported for the dataset.
    pub dataset_timestamp: Option<String>,
    /// When this process downloaded the file.
    pub fetched_at: SystemTime,
    /// The DataMall endpoint, without the account key and without the
    /// signed query of the download link.
    pub source_endpoint: String,
    /// The file.
    pub bytes: Vec<u8>,
    /// The SHA-256 of the file, in lowercase hexadecimal.
    pub sha256: String,
}

impl DataMallSnapshot {
    /// Get the size of the file in bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Report whether the file is empty.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Get the first twelve characters of the fingerprint.
    pub fn short_sha256(&self) -> &str {
        let end = self.sha256.len().min(12);
        &self.sha256[..end]
    }
}

/// The debug output never shows the bytes and never shows a signed
/// URL.
impl std::fmt::Debug for DataMallSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataMallSnapshot")
            .field("dataset_timestamp", &self.dataset_timestamp)
            .field("source_endpoint", &self.source_endpoint)
            .field("bytes", &format_args!("{} bytes", self.bytes.len()))
            .field("sha256", &self.sha256)
            .finish()
    }
}

/// How many times the client asks for a fresh link after a download
/// fails with a status that an expired signature produces.
const LINK_ATTEMPTS: usize = 3;

/// The largest dataset that the client accepts, in bytes.
///
/// The train GTFS archive is a few megabytes. The cap stops a
/// misbehaving host from filling memory.
pub const MAX_DATASET_BYTES: usize = 256 * 1024 * 1024;

impl<T: Transport> DataMallClient<T> {
    /// Fetch the train GTFS Schedule archive as a snapshot.
    ///
    /// The function asks for a link and downloads it at once, because
    /// the link expires after about fifteen minutes. If the signature
    /// has already expired, it asks for a new link rather than
    /// retrying the same URL.
    pub fn fetch_gtfs_schedule_snapshot(&self) -> Result<DataMallSnapshot, DataMallError> {
        self.fetch_snapshot("GTFSScheduleTrain")
    }

    /// Fetch the GTFS-Realtime trip updates as a snapshot.
    pub fn fetch_trip_updates_snapshot(&self) -> Result<DataMallSnapshot, DataMallError> {
        self.fetch_snapshot("GTFSRealtimeTrainTripUpdates")
    }

    /// Fetch the GTFS-Realtime service alerts as a snapshot.
    pub fn fetch_service_alerts_snapshot(&self) -> Result<DataMallSnapshot, DataMallError> {
        self.fetch_snapshot("GTFSRealTimeTrainServiceAlerts")
    }

    /// Fetch any dataset endpoint as a snapshot.
    pub fn fetch_snapshot(&self, endpoint: &str) -> Result<DataMallSnapshot, DataMallError> {
        let mut last: Option<DataMallError> = None;
        for _ in 0..LINK_ATTEMPTS {
            let link = self.dataset_link_for(endpoint)?;
            require_https(&link.url)?;
            match self.download_limited(&link.url, MAX_DATASET_BYTES) {
                Ok(bytes) => {
                    return Ok(DataMallSnapshot {
                        dataset_timestamp: link.timestamp.clone(),
                        fetched_at: SystemTime::now(),
                        source_endpoint: endpoint.to_string(),
                        sha256: sha256_hex(&bytes),
                        bytes,
                    });
                }
                // A pre-signed link that has expired answers with 403,
                // and sometimes with 400. Asking for a new link is the
                // fix; repeating the same URL never is.
                Err(DataMallError::Http { status, url }) if status == 403 || status == 400 => {
                    last = Some(DataMallError::Http { status, url });
                }
                Err(other) => return Err(other),
            }
        }
        Err(last.unwrap_or(DataMallError::NoLink {
            url: endpoint.to_string(),
        }))
    }

    /// Download a file, refusing a body beyond `limit` bytes.
    pub fn download_limited(&self, url: &str, limit: usize) -> Result<Vec<u8>, DataMallError> {
        require_https(url)?;
        let bytes = self.download(url)?;
        if bytes.len() > limit {
            return Err(DataMallError::Decode {
                url: redact_url(url),
                message: format!(
                    "the dataset is {} bytes; the limit is {limit} bytes",
                    bytes.len()
                ),
            });
        }
        Ok(bytes)
    }
}

/// Reject a download URL that is not HTTPS.
///
/// A pre-signed link carries a signature but no confidentiality. Plain
/// HTTP would expose it, and any redirect to HTTP would too.
fn require_https(url: &str) -> Result<(), DataMallError> {
    if url.starts_with("https://") {
        Ok(())
    } else {
        Err(DataMallError::Decode {
            url: redact_url(url),
            message: "a dataset download link must use HTTPS".to_string(),
        })
    }
}

/// Remove the secret parts of a URL, so it is safe to log.
///
/// A pre-signed S3 link carries `X-Amz-Signature` and friends in its
/// query. The whole query goes, because a partial redaction is a
/// promise that is easy to break when a provider adds a parameter.
///
/// # Examples
///
/// ```
/// use mrt_datamall::redact_url;
///
/// let link = "https://host.example/gtfs.zip?X-Amz-Signature=abc&X-Amz-Expires=900";
/// assert_eq!(redact_url(link), "https://host.example/gtfs.zip?<redacted>");
/// ```
pub fn redact_url(url: &str) -> String {
    match url.split_once('?') {
        Some((base, _)) => format!("{base}?<redacted>"),
        None => url.to_string(),
    }
}

/// Get the SHA-256 of some bytes, in lowercase hexadecimal.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_query_never_survives_redaction() {
        let link = "https://dmprod-datasets.s3.ap-southeast-1.amazonaws.com/train-gtfs-schedule/\
                    gtfs_schedule.zip?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Signature=deadbeef";
        let redacted = redact_url(link);
        assert!(!redacted.contains("deadbeef"));
        assert!(!redacted.contains("X-Amz"));
        assert!(redacted.ends_with("gtfs_schedule.zip?<redacted>"));
    }

    #[test]
    fn a_url_without_a_query_survives_unchanged() {
        let plain = "https://host.example/file.zip";
        assert_eq!(redact_url(plain), plain);
    }

    #[test]
    fn only_https_links_are_downloaded() {
        assert!(require_https("https://host.example/x.zip").is_ok());
        let error = require_https("http://host.example/x.zip?sig=abc").unwrap_err();
        assert!(error.to_string().contains("HTTPS"));
        assert!(!error.to_string().contains("sig=abc"));
    }

    #[test]
    fn sha256_matches_the_published_vectors() {
        // The NIST examples for SHA-256.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn the_debug_output_hides_the_bytes() {
        let snapshot = DataMallSnapshot {
            dataset_timestamp: None,
            fetched_at: SystemTime::UNIX_EPOCH,
            source_endpoint: "GTFSScheduleTrain".into(),
            bytes: b"secretish payload".to_vec(),
            sha256: sha256_hex(b"secretish payload"),
        };
        let debug = format!("{snapshot:?}");
        assert!(!debug.contains("secretish"));
        assert!(debug.contains("17 bytes"));
        assert_eq!(snapshot.short_sha256().len(), 12);
    }
}
