//! The generation manifest.
//!
//! A manifest answers "where did this file come from?" without opening
//! it. It carries the feed fingerprint, the feed timestamp, the
//! configuration fingerprint, the generator version, the service date,
//! the time zone, the artifacts, and the diagnostics.
//!
//! The manifest is the only artifact that records the generation time,
//! which keeps the documents themselves byte-for-byte reproducible.

use std::path::Path;

use mrt_gtfs::Diagnostic;
use serde::{Deserialize, Serialize};

use crate::error::CliError;
use crate::fsutil::write_atomic;

/// One generated file.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ArtifactRecord {
    /// The path that the generator wrote.
    pub path: String,
    /// The kind of artifact: `timetable`, `diagram`, or `feed`.
    pub kind: String,
    /// The format: `html`, `svg`, `json`, or `zip`.
    pub format: String,
    /// The size in bytes.
    pub bytes: u64,
    /// The SHA-256 of the file.
    pub sha256: String,
}

/// The manifest of one run.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    /// The schema version of the manifest itself.
    pub manifest_version: String,
    /// The version of the program that generated the artifacts.
    pub generator_version: String,
    /// When the run happened, in POSIX seconds. This is the only
    /// non-deterministic value in any output.
    pub generated_at: u64,
    /// The command that produced the artifacts.
    pub command: String,
    /// The SHA-256 of the feed.
    pub feed_sha256: String,
    /// The timestamp that the feed publisher stated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feed_timestamp: Option<String>,
    /// Where the feed came from: a path, or the DataMall endpoint name.
    /// Never a signed URL.
    pub feed_source: String,
    /// Whether the feed came from the cache after a failed download.
    pub feed_from_cache: bool,
    /// The SHA-256 of the configuration.
    pub configuration_sha256: String,
    /// The configuration file, when one was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration_path: Option<String>,
    /// The service date of the documents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_date: Option<String>,
    /// The time zone of the schedule.
    pub timezone: String,
    /// The view-model schema version.
    pub schema_version: String,
    /// The generated files.
    pub artifacts: Vec<ArtifactRecord>,
    /// The diagnostics of the run.
    pub diagnostics: Vec<Diagnostic>,
}

/// The schema version of the manifest format.
pub const MANIFEST_VERSION: &str = "1.0";

impl Manifest {
    /// Write the manifest atomically.
    pub fn write(&self, path: &Path) -> Result<(), CliError> {
        let mut json = serde_json::to_vec_pretty(self).map_err(|e| {
            CliError::new(
                crate::error::ExitCode::OutputFailure,
                format!("cannot serialize the manifest: {e}"),
            )
        })?;
        json.push(b'\n');
        write_atomic(path, &json)
    }

    /// Report whether any diagnostic reaches warning severity.
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity >= mrt_gtfs::Severity::Warning)
    }
}

/// Record one written artifact.
pub fn record(path: &str, kind: &str, format: &str, bytes: &[u8]) -> ArtifactRecord {
    ArtifactRecord {
        path: path.to_string(),
        kind: kind.to_string(),
        format: format.to_string(),
        bytes: bytes.len() as u64,
        sha256: mrt_datamall::sha256_hex(bytes),
    }
}

/// Get the current POSIX time in seconds.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            manifest_version: MANIFEST_VERSION.to_string(),
            generator_version: "mrt-schedule-cli 0.1.0".into(),
            generated_at: 1_786_406_400,
            command: "timetable".into(),
            feed_sha256: "a".repeat(64),
            feed_timestamp: Some("2026-08-10T00:00:00+08:00".into()),
            feed_source: "GTFSScheduleTrain".into(),
            feed_from_cache: false,
            configuration_sha256: "b".repeat(64),
            configuration_path: Some("config/singapore.yaml".into()),
            service_date: Some("20260810".into()),
            timezone: "Asia/Singapore".into(),
            schema_version: mrt_publication::SCHEMA_VERSION.to_string(),
            artifacts: vec![record("dist/ns1.html", "timetable", "html", b"<html>")],
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn a_manifest_round_trips_through_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        sample().write(&path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with('\n'));
        let parsed: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.feed_sha256, "a".repeat(64));
        assert_eq!(parsed.artifacts[0].bytes, 6);
        assert_eq!(
            parsed.artifacts[0].sha256,
            mrt_datamall::sha256_hex(b"<html>")
        );
    }

    #[test]
    fn a_manifest_never_carries_a_signed_url_or_a_key() {
        let text = serde_json::to_string(&sample()).unwrap();
        assert!(!text.contains("X-Amz"));
        assert!(!text.contains("AccountKey"));
        assert!(!text.contains("https://"));
    }

    #[test]
    fn warnings_are_visible_to_the_caller() {
        let mut manifest = sample();
        assert!(!manifest.has_warnings());
        manifest
            .diagnostics
            .push(Diagnostic::info("note", "nothing important"));
        assert!(!manifest.has_warnings());
        manifest
            .diagnostics
            .push(Diagnostic::warning("time-missing", "a call has no time"));
        assert!(manifest.has_warnings());
    }
}
