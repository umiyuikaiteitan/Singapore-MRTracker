//! # mrt-gtfs
//!
//! Ingest GTFS static feeds and build a rail network model for the
//! Singapore MRT and LRT.
//!
//! This crate is the static-data core of the Singapore-MRTracker
//! project. It reads a GTFS feed, selects the rail subset, and builds a
//! linked model of lines, stations, and schedules. Applications use
//! the model to draw network maps, destination boards, and LED panels.
//!
//! The crate has four layers. Each layer is usable on its own:
//!
//! 1. **Sources** ([`FeedSource`], [`DirectorySource`], [`ZipSource`]) —
//!    supply the bytes of the feed files.
//! 2. **Feed** ([`GtfsFeed`]) — parse the files into raw record tables.
//! 3. **Filter** ([`RailFilter`]) — select the rail subset by GTFS
//!    route type.
//! 4. **Network** ([`RailNetwork`]) — link the records into stations,
//!    lines, patterns, and schedules, and answer queries such as
//!    [`RailNetwork::departure_board`].
//!
//! # Example
//!
//! ```no_run
//! use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork, ServiceDate};
//!
//! // Step 1: load the feed.
//! let feed = GtfsFeed::from_zip_path("data/singapore-gtfs.zip").unwrap();
//!
//! // Step 2: build the rail network model.
//! let network = RailNetwork::from_feed(&feed).unwrap();
//!
//! // Step 3: query a destination board.
//! let station = network.station_by_code("NS1").unwrap();
//! let date: ServiceDate = "20260810".parse().unwrap();
//! let clock: GtfsTime = "08:00:00".parse().unwrap();
//! for entry in network.departure_board(station, date, clock, 1800) {
//!     let terminus = network.station(entry.departure.terminus);
//!     println!("{} to {}", entry.clock_time(), terminus.name);
//! }
//! ```
//!
//! # Design notes
//!
//! - The crate does no network input/output. A [`FeedSource`] supplies
//!   the feed bytes. This keeps the crate testable and portable.
//! - The model uses plain index identifiers ([`StationId`], [`LineId`],
//!   [`PatternId`]). A port to another language can keep the same
//!   design.
//! - All public model types serialize with `serde`, so applications
//!   can export the model as JSON.

#![warn(missing_docs)]

mod date;
mod error;
mod feed;
mod filter;
mod model;
mod network;
mod schedule;
mod source;
mod time;

pub use date::{ServiceDate, Weekday};
pub use error::GtfsError;
pub use feed::GtfsFeed;
pub use filter::RailFilter;
pub use model::{
    Agency, Calendar, CalendarDate, Frequency, Route, ShapePoint, Stop, StopTime, Transfer, Trip,
    EXCEPTION_SERVICE_ADDED, EXCEPTION_SERVICE_REMOVED,
};
pub use network::{
    Line, LineId, PatternId, RailNetwork, Station, StationId, StationTransfer, StopPattern,
};
pub use schedule::{BoardEntry, Departure};
#[cfg(feature = "zip-source")]
pub use source::ZipSource;
pub use source::{DirectorySource, FeedSource};
pub use time::GtfsTime;
