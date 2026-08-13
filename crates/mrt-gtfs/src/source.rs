//! Feed sources.
//!
//! A [`FeedSource`] supplies the files of one GTFS feed. The library
//! includes a directory source and a zip source. Implement the trait to
//! ingest feeds from other locations, for example object storage or an
//! HTTP download.

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use crate::error::GtfsError;

/// A source that supplies the files of one GTFS feed.
pub trait FeedSource {
    /// Open the feed file with the given name, for example `stops.txt`.
    ///
    /// Return `Ok(None)` if the file is not in the feed. Return an error
    /// only if the file exists and the source cannot read it.
    fn open<'a>(&'a mut self, name: &str) -> Result<Option<Box<dyn Read + 'a>>, GtfsError>;
}

/// A feed source that reads files from a directory.
///
/// # Examples
///
/// ```no_run
/// use mrt_gtfs::{DirectorySource, GtfsFeed};
///
/// let mut source = DirectorySource::new("data/sg-rail-gtfs");
/// let feed = GtfsFeed::load(&mut source).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct DirectorySource {
    root: PathBuf,
}

impl DirectorySource {
    /// Make a source for the given directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        DirectorySource { root: root.into() }
    }

    /// Get the directory of this source.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl FeedSource for DirectorySource {
    fn open<'a>(&'a mut self, name: &str) -> Result<Option<Box<dyn Read + 'a>>, GtfsError> {
        let path = self.root.join(name);
        match File::open(&path) {
            Ok(file) => Ok(Some(Box::new(file))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(GtfsError::io(name, e)),
        }
    }
}

/// Safety limits for reading a GTFS zip archive.
///
/// A downloaded archive is untrusted input. The limits stop a
/// decompression bomb and an archive with an unreasonable number of
/// entries before the loader spends memory on it.
#[cfg(feature = "zip-source")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipLimits {
    /// The largest number of entries that the archive may contain.
    pub max_entries: usize,
    /// The largest total uncompressed size, in bytes.
    pub max_total_bytes: u64,
    /// The largest uncompressed size of one entry, in bytes.
    pub max_entry_bytes: u64,
}

#[cfg(feature = "zip-source")]
impl Default for ZipLimits {
    /// The defaults hold the official LTA train feed with room to
    /// spare: 4096 entries, 2 GiB in total, 1 GiB per entry.
    fn default() -> Self {
        ZipLimits {
            max_entries: 4096,
            max_total_bytes: 2 * 1024 * 1024 * 1024,
            max_entry_bytes: 1024 * 1024 * 1024,
        }
    }
}

/// How strictly the zip loader reads an archive.
#[cfg(feature = "zip-source")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ZipStrictness {
    /// Accept a feed that sits in a subdirectory of the archive, and
    /// accept a byte-order mark at the start of a file.
    #[default]
    Lenient,
    /// Require the feed files at the root of the archive, as standard
    /// GTFS specifies.
    Strict,
}

/// Options for reading a GTFS zip archive.
#[cfg(feature = "zip-source")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ZipOptions {
    /// The safety limits.
    pub limits: ZipLimits,
    /// How strictly to read the archive layout.
    pub strictness: ZipStrictness,
}

/// A feed source that reads files from a zip archive.
///
/// The source finds a feed file also when the archive keeps the feed in
/// a subdirectory, unless [`ZipStrictness::Strict`] forbids it.
///
/// Every archive passes a safety check before the loader reads a byte
/// of content. The check rejects absolute entry paths, `..` path
/// traversal, symbolic links, ambiguous duplicates of one feed file,
/// and sizes beyond [`ZipLimits`].
#[cfg(feature = "zip-source")]
pub struct ZipSource<R: Read + std::io::Seek> {
    archive: zip::ZipArchive<R>,
    strictness: ZipStrictness,
}

#[cfg(feature = "zip-source")]
impl<R: Read + std::io::Seek> std::fmt::Debug for ZipSource<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZipSource")
            .field("entries", &self.archive.len())
            .field("strictness", &self.strictness)
            .finish()
    }
}

#[cfg(feature = "zip-source")]
impl ZipSource<File> {
    /// Open a zip archive at the given path with the default options.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, GtfsError> {
        Self::from_path_with(path, &ZipOptions::default())
    }

    /// Open a zip archive at the given path with explicit options.
    pub fn from_path_with(path: impl AsRef<Path>, options: &ZipOptions) -> Result<Self, GtfsError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|e| GtfsError::io(&path.display().to_string(), e))?;
        Self::from_reader_with(file, options)
    }
}

/// The GTFS files that this library reads. The duplicate check uses
/// the list, because a second `stops.txt` makes the feed ambiguous.
#[cfg(feature = "zip-source")]
const FEED_FILES: [&str; 10] = [
    "agency.txt",
    "stops.txt",
    "routes.txt",
    "trips.txt",
    "stop_times.txt",
    "calendar.txt",
    "calendar_dates.txt",
    "frequencies.txt",
    "transfers.txt",
    "shapes.txt",
];

#[cfg(feature = "zip-source")]
impl<R: Read + std::io::Seek> ZipSource<R> {
    /// Open a zip archive from a reader with the default options.
    pub fn from_reader(reader: R) -> Result<Self, GtfsError> {
        Self::from_reader_with(reader, &ZipOptions::default())
    }

    /// Open a zip archive from a reader with explicit options.
    pub fn from_reader_with(reader: R, options: &ZipOptions) -> Result<Self, GtfsError> {
        let mut archive =
            zip::ZipArchive::new(reader).map_err(|e| GtfsError::Zip(e.to_string()))?;
        check_archive(&mut archive, options)?;
        Ok(ZipSource {
            archive,
            strictness: options.strictness,
        })
    }

    /// Find the archive entry for a feed file name.
    fn resolve_name(&self, name: &str) -> Option<String> {
        let suffix = format!("/{name}");
        self.archive
            .file_names()
            .find(|entry| {
                *entry == name
                    || (self.strictness == ZipStrictness::Lenient && entry.ends_with(&suffix))
            })
            .map(str::to_string)
    }
}

/// Reject an archive that is unsafe or too large to read.
#[cfg(feature = "zip-source")]
fn check_archive<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    options: &ZipOptions,
) -> Result<(), GtfsError> {
    let limits = options.limits;
    if archive.len() > limits.max_entries {
        return Err(GtfsError::UnsafeZip(format!(
            "the archive has {} entries; the limit is {}",
            archive.len(),
            limits.max_entries
        )));
    }

    let mut total: u64 = 0;
    let mut seen: Vec<(String, String)> = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|e| GtfsError::Zip(e.to_string()))?;
        let raw_name = entry.name().to_string();
        check_entry_name(&raw_name)?;

        // A symbolic link inside an archive can point anywhere on the
        // file system. The loader never follows one.
        const S_IFMT: u32 = 0o170_000;
        const S_IFLNK: u32 = 0o120_000;
        if entry.unix_mode().is_some_and(|m| m & S_IFMT == S_IFLNK) {
            return Err(GtfsError::UnsafeZip(format!(
                "entry \"{raw_name}\" is a symbolic link"
            )));
        }

        if entry.is_dir() {
            continue;
        }
        let size = entry.size();
        if size > limits.max_entry_bytes {
            return Err(GtfsError::UnsafeZip(format!(
                "entry \"{raw_name}\" expands to {size} bytes; the limit is {}",
                limits.max_entry_bytes
            )));
        }
        total = total.saturating_add(size);
        if total > limits.max_total_bytes {
            return Err(GtfsError::UnsafeZip(format!(
                "the archive expands to more than {} bytes",
                limits.max_total_bytes
            )));
        }

        // A feed file that appears twice makes the feed ambiguous.
        let base = raw_name.rsplit('/').next().unwrap_or(&raw_name);
        if FEED_FILES.contains(&base) {
            if options.strictness == ZipStrictness::Strict && raw_name != base {
                return Err(GtfsError::UnsafeZip(format!(
                    "strict mode expects \"{base}\" at the root of the archive, \
                     but the archive stores it as \"{raw_name}\""
                )));
            }
            if let Some((_, first)) = seen.iter().find(|(name, _)| name == base) {
                return Err(GtfsError::UnsafeZip(format!(
                    "the archive contains \"{base}\" twice: \"{first}\" and \"{raw_name}\""
                )));
            }
            seen.push((base.to_string(), raw_name.clone()));
        }
    }
    Ok(())
}

/// Reject an entry name that could escape the extraction directory.
///
/// The library never writes the entries to disk, but a caller might,
/// and a traversal name is a reliable sign of a hostile archive.
#[cfg(feature = "zip-source")]
fn check_entry_name(name: &str) -> Result<(), GtfsError> {
    let unsafe_name =
        |reason: &str| Err(GtfsError::UnsafeZip(format!("entry \"{name}\" {reason}")));
    if name.is_empty() {
        return unsafe_name("has an empty name");
    }
    if name.contains('\0') {
        return unsafe_name("contains a null byte");
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return unsafe_name("is an absolute path");
    }
    // A Windows drive letter, for example "C:\feed\stops.txt".
    let bytes = name.as_bytes();
    if bytes.len() > 1 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return unsafe_name("is an absolute path");
    }
    if name
        .split(['/', '\\'])
        .any(|component| component == ".." || component == "...")
    {
        return unsafe_name("escapes its directory with \"..\"");
    }
    Ok(())
}

#[cfg(feature = "zip-source")]
impl<R: Read + std::io::Seek> FeedSource for ZipSource<R> {
    fn open<'a>(&'a mut self, name: &str) -> Result<Option<Box<dyn Read + 'a>>, GtfsError> {
        let Some(entry) = self.resolve_name(name) else {
            return Ok(None);
        };
        let file = self
            .archive
            .by_name(&entry)
            .map_err(|e| GtfsError::Zip(e.to_string()))?;
        Ok(Some(Box::new(file)))
    }
}

/// Remove a UTF-8 byte-order mark from the start of a stream.
///
/// Some feed publishers add a byte-order mark to the feed files. The
/// mark corrupts the first CSV header name if it stays in the stream.
pub(crate) fn strip_bom<R: Read>(
    mut reader: R,
) -> std::io::Result<std::io::Chain<Cursor<Vec<u8>>, R>> {
    const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
    let mut head = [0u8; 3];
    let mut filled = 0;
    while filled < head.len() {
        match reader.read(&mut head[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    let replay = if filled == 3 && head == BOM {
        Vec::new()
    } else {
        head[..filled].to_vec()
    };
    Ok(Cursor::new(replay).chain(reader))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_bom_removes_the_mark() {
        let data = b"\xEF\xBB\xBFstop_id\n1\n";
        let mut out = String::new();
        strip_bom(&data[..])
            .unwrap()
            .read_to_string(&mut out)
            .unwrap();
        assert_eq!(out, "stop_id\n1\n");
    }

    #[test]
    fn strip_bom_keeps_normal_data() {
        let data = b"stop_id\n1\n";
        let mut out = String::new();
        strip_bom(&data[..])
            .unwrap()
            .read_to_string(&mut out)
            .unwrap();
        assert_eq!(out, "stop_id\n1\n");
    }

    #[test]
    fn strip_bom_keeps_short_data() {
        for data in [&b""[..], &b"a"[..], &b"ab"[..]] {
            let mut out = Vec::new();
            strip_bom(data).unwrap().read_to_end(&mut out).unwrap();
            assert_eq!(out, data);
        }
    }
}
