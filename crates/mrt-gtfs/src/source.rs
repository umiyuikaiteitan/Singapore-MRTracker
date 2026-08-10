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

/// A feed source that reads files from a zip archive.
///
/// The source finds a feed file also when the archive keeps the feed in
/// a subdirectory.
#[cfg(feature = "zip-source")]
pub struct ZipSource<R: Read + std::io::Seek> {
    archive: zip::ZipArchive<R>,
}

#[cfg(feature = "zip-source")]
impl ZipSource<File> {
    /// Open a zip archive at the given path.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, GtfsError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|e| GtfsError::io(&path.display().to_string(), e))?;
        Self::from_reader(file)
    }
}

#[cfg(feature = "zip-source")]
impl<R: Read + std::io::Seek> ZipSource<R> {
    /// Open a zip archive from a reader.
    pub fn from_reader(reader: R) -> Result<Self, GtfsError> {
        let archive = zip::ZipArchive::new(reader).map_err(|e| GtfsError::Zip(e.to_string()))?;
        Ok(ZipSource { archive })
    }

    /// Find the archive entry for a feed file name.
    fn resolve_name(&self, name: &str) -> Option<String> {
        let suffix = format!("/{name}");
        self.archive
            .file_names()
            .find(|entry| *entry == name || entry.ends_with(&suffix))
            .map(str::to_string)
    }
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
