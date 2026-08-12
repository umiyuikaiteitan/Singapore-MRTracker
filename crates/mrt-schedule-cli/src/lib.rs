//! # mrt-schedule-cli
//!
//! Generate station timetables and train diagrams from a GTFS
//! Schedule feed.
//!
//! The crate is the orchestration layer of the publication pipeline:
//!
//! ```text
//! DataMall or a local archive
//!   -> mrt-gtfs            parse, filter, link, query
//!   -> mrt-publication     timetable and diagram view models
//!   -> mrt-publication-html  HTML and SVG
//!   -> mrt-schedule-cli    files, cache, manifest, exit codes
//! ```
//!
//! Nothing below this crate reads a file, opens a socket, or knows
//! about the command line, so a future HTTP server can call the same
//! library functions instead of duplicating schedule logic.
//!
//! # The account key
//!
//! The DataMall account key is read from an environment variable and
//! never written anywhere: not to a log line, not to the cache, not
//! to the manifest, and not to a generated page. Signed download URLs
//! are redacted before they reach any output.

#![warn(missing_docs)]

pub mod args;
pub mod cache;
pub mod error;
pub mod fsutil;
pub mod manifest;
pub mod run;
pub mod yaml;

pub use args::{Args, Command, Format, Parsed};
pub use error::{CliError, ExitCode};
pub use run::run;

/// The version of the generator, as it appears in every document.
pub fn generator_version() -> String {
    format!("mrt-schedule-cli {}", env!("CARGO_PKG_VERSION"))
}
