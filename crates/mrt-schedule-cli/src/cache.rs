//! The content-addressed feed cache.
//!
//! A DataMall download link lives for about fifteen minutes, and the
//! feed itself changes rarely. The cache stores every archive under
//! its own SHA-256, so a repeat run reuses the bytes and a generated
//! document can name exactly which feed it came from.
//!
//! ```text
//! cache/
//!   current.json          the newest object and its metadata
//!   objects/<sha256>.zip  the archive
//!   metadata/<sha256>.json  when it arrived and what DataMall said
//! ```
//!
//! After a failed download the cache can serve the last good object,
//! but only when the caller passes `--allow-stale`, and then every
//! generated page says so.

use std::path::{Path, PathBuf};

use mrt_datamall::sha256_hex;
use serde::{Deserialize, Serialize};

use crate::error::{CliError, ExitCode};
use crate::fsutil::write_atomic;

/// What the cache knows about one stored archive.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CacheEntry {
    /// The SHA-256 of the archive, in lowercase hexadecimal.
    pub sha256: String,
    /// The timestamp that DataMall reported for the dataset.
    #[serde(default)]
    pub dataset_timestamp: Option<String>,
    /// When this machine downloaded the archive, in POSIX seconds.
    pub fetched_at: u64,
    /// The DataMall endpoint. Never a signed URL.
    pub source_endpoint: String,
    /// The size of the archive in bytes.
    pub bytes: u64,
}

/// A directory of cached feed archives.
#[derive(Clone, Debug)]
pub struct FeedCache {
    root: PathBuf,
}

impl FeedCache {
    /// Open, and if necessary create, a cache directory.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, CliError> {
        let root = root.into();
        for directory in [root.join("objects"), root.join("metadata")] {
            std::fs::create_dir_all(&directory).map_err(|e| {
                CliError::new(
                    ExitCode::SourceFailure,
                    format!(
                        "cannot create the cache directory {}: {e}",
                        directory.display()
                    ),
                )
            })?;
        }
        Ok(FeedCache { root })
    }

    /// Get the path of the cache directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get the path of the object with the given fingerprint.
    pub fn object_path(&self, sha256: &str) -> PathBuf {
        self.root.join("objects").join(format!("{sha256}.zip"))
    }

    fn metadata_path(&self, sha256: &str) -> PathBuf {
        self.root.join("metadata").join(format!("{sha256}.json"))
    }

    fn current_path(&self) -> PathBuf {
        self.root.join("current.json")
    }

    /// Store an archive and make it the current one.
    ///
    /// Storing the same bytes twice is cheap: the object path already
    /// exists, so only the metadata and the pointer are rewritten.
    pub fn store(
        &self,
        bytes: &[u8],
        dataset_timestamp: Option<String>,
        source_endpoint: &str,
        fetched_at: u64,
    ) -> Result<CacheEntry, CliError> {
        let sha256 = sha256_hex(bytes);
        let entry = CacheEntry {
            sha256: sha256.clone(),
            dataset_timestamp,
            fetched_at,
            source_endpoint: source_endpoint.to_string(),
            bytes: bytes.len() as u64,
        };
        let object = self.object_path(&sha256);
        if !object.exists() {
            write_atomic(&object, bytes)?;
        }
        let json = serde_json::to_vec_pretty(&entry).expect("the entry serializes");
        write_atomic(&self.metadata_path(&sha256), &json)?;
        write_atomic(&self.current_path(), &json)?;
        Ok(entry)
    }

    /// Get the entry that the cache last stored.
    pub fn current(&self) -> Option<CacheEntry> {
        let bytes = std::fs::read(self.current_path()).ok()?;
        let entry: CacheEntry = serde_json::from_slice(&bytes).ok()?;
        self.object_path(&entry.sha256).exists().then_some(entry)
    }

    /// Read the bytes of a stored object.
    pub fn read(&self, sha256: &str) -> Result<Vec<u8>, CliError> {
        let path = self.object_path(sha256);
        std::fs::read(&path).map_err(|e| {
            CliError::new(
                ExitCode::SourceFailure,
                format!("cannot read the cached feed {}: {e}", path.display()),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storing_and_reading_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FeedCache::open(dir.path()).unwrap();
        assert!(cache.current().is_none());

        let entry = cache
            .store(
                b"feed bytes",
                Some("2026-08-10T00:00:00+08:00".into()),
                "GTFSScheduleTrain",
                100,
            )
            .unwrap();
        assert_eq!(entry.sha256, sha256_hex(b"feed bytes"));
        assert_eq!(entry.bytes, 10);

        let current = cache.current().unwrap();
        assert_eq!(current.sha256, entry.sha256);
        assert_eq!(current.fetched_at, 100);
        assert_eq!(cache.read(&entry.sha256).unwrap(), b"feed bytes");
    }

    #[test]
    fn the_same_bytes_reuse_one_object() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FeedCache::open(dir.path()).unwrap();
        cache.store(b"same", None, "E", 1).unwrap();
        cache.store(b"same", None, "E", 2).unwrap();
        let objects: Vec<_> = std::fs::read_dir(dir.path().join("objects"))
            .unwrap()
            .collect();
        assert_eq!(objects.len(), 1);
        // The pointer still carries the newer fetch time.
        assert_eq!(cache.current().unwrap().fetched_at, 2);
    }

    #[test]
    fn different_bytes_keep_both_objects() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FeedCache::open(dir.path()).unwrap();
        let first = cache.store(b"one", None, "E", 1).unwrap();
        let second = cache.store(b"two", None, "E", 2).unwrap();
        assert_ne!(first.sha256, second.sha256);
        assert_eq!(cache.read(&first.sha256).unwrap(), b"one");
        assert_eq!(cache.current().unwrap().sha256, second.sha256);
    }

    #[test]
    fn a_pointer_without_its_object_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FeedCache::open(dir.path()).unwrap();
        let entry = cache.store(b"gone", None, "E", 1).unwrap();
        std::fs::remove_file(cache.object_path(&entry.sha256)).unwrap();
        assert!(cache.current().is_none());
    }
}
