//! Error types for GTFS ingestion.

/// An error that occurs when the library reads or interprets a GTFS feed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GtfsError {
    /// An input or output operation failed.
    #[error("input/output error on \"{file}\": {source}")]
    Io {
        /// The feed file that the library tried to read.
        file: String,
        /// The cause of the failure.
        #[source]
        source: std::io::Error,
    },

    /// A required feed file is not in the feed.
    #[error("required feed file \"{0}\" is not in the feed")]
    MissingFile(String),

    /// A feed file contains a record that the library cannot parse.
    #[error("cannot parse \"{file}\": {message}")]
    Parse {
        /// The feed file that contains the bad record.
        file: String,
        /// A description of the bad record.
        message: String,
    },

    /// A value in a feed file is not valid.
    #[error("invalid value: {0}")]
    InvalidValue(String),

    /// A record refers to an identifier that is not in the feed.
    #[error("unknown {kind} identifier \"{id}\"")]
    UnknownId {
        /// The kind of identifier, for example "stop" or "route".
        kind: &'static str,
        /// The identifier that the library cannot find.
        id: String,
    },

    /// The feed does not contain service calendar data.
    ///
    /// A valid feed must contain `calendar.txt`, `calendar_dates.txt`,
    /// or both.
    #[error("the feed contains no calendar.txt and no calendar_dates.txt")]
    NoCalendar,

    /// The requested output cannot be produced under the selected
    /// policy.
    ///
    /// The library raises this error instead of quietly presenting
    /// approximate data as exact, or dropping service that the caller
    /// asked for.
    #[error("the requested output cannot be produced: {0}")]
    PolicyViolation(String),

    /// The feed fails validation.
    #[error("the feed is not valid: {0}")]
    Invalid(String),

    /// The zip archive is not valid.
    #[cfg(feature = "zip-source")]
    #[error("zip archive error: {0}")]
    Zip(String),

    /// The zip archive is unsafe to extract.
    ///
    /// The loader refuses archives with absolute paths, `..` path
    /// traversal, symbolic links, ambiguous duplicate feed files, or
    /// sizes beyond the configured limits.
    #[cfg(feature = "zip-source")]
    #[error("the zip archive is not safe to read: {0}")]
    UnsafeZip(String),
}

impl GtfsError {
    /// Make an `Io` error for the given feed file.
    pub(crate) fn io(file: &str, source: std::io::Error) -> Self {
        GtfsError::Io {
            file: file.to_string(),
            source,
        }
    }

    /// Make a `Parse` error for the given feed file.
    pub(crate) fn parse(file: &str, message: impl std::fmt::Display) -> Self {
        GtfsError::Parse {
            file: file.to_string(),
            message: message.to_string(),
        }
    }
}
