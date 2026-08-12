//! # mrt-publication
//!
//! Turn a GTFS rail schedule into the view models of two printed
//! products:
//!
//! 1. A Japanese-style station departure timetable, 発車時刻表.
//! 2. A planning-style time–distance train diagram, 列車ダイヤグラム.
//!
//! The crate is pure. It performs no input or output, opens no
//! socket, reads no file, and knows nothing about HTML. It takes a
//! [`mrt_gtfs::RailNetwork`], a [`PublicationConfig`], and a
//! [`DocumentSeed`], and returns serializable documents that a
//! renderer can draw.
//!
//! ```text
//! mrt-gtfs  ->  mrt-publication  ->  mrt-publication-html  ->  mrt-schedule-cli
//! ```
//!
//! # Determinism
//!
//! The same feed, configuration, service date, and generator version
//! produce byte-identical documents. Nothing in a document reads a
//! clock. Generation time belongs in the manifest that the command
//! line writes, not in the document.
//!
//! # Honesty rules
//!
//! The projections never invent schedule data:
//!
//! - Destination text comes from `stop_headsign`, `trip_headsign`,
//!   the real terminus of the run, or an explicit configuration
//!   override — in that order.
//! - Platform text comes from the platform that the run really uses,
//!   or from an explicit override. A direction never implies one.
//! - Headway service that GTFS marks `exact_times=0` becomes a band
//!   with an approximation mark, never a list of exact minutes,
//!   unless the caller selects
//!   [`mrt_gtfs::FrequencyPolicy::ExpandApproximate`] — and then every
//!   entry carries the mark.
//! - A time that the library computed is marked as computed.
//! - A run that does not fit the station axis of a corridor is left
//!   out with a diagnostic, never bent onto it.
//!
//! # Example
//!
//! ```no_run
//! use mrt_gtfs::{GtfsFeed, RailNetwork};
//! use mrt_publication::{build_timetable, DocumentSeed, PublicationConfig};
//!
//! let feed = GtfsFeed::from_zip_path("data/singapore-gtfs.zip").unwrap();
//! let network = RailNetwork::from_feed(&feed).unwrap();
//! let station = network.station_by_alias("ns1").unwrap();
//!
//! let document = build_timetable(
//!     &network,
//!     station,
//!     "20260810".parse().unwrap(),
//!     None,
//!     &PublicationConfig::default(),
//!     &DocumentSeed::default(),
//! )
//! .unwrap();
//! println!("{} departures", document.departure_count());
//! ```

#![warn(missing_docs)]

pub mod common;
pub mod config;
pub mod corridor;
pub mod diagram;
mod error;
pub mod text;
pub mod timetable;

pub use common::{
    css_color, css_key, DepartureFlag, DocumentSeed, LegendItem, LineView, PublicationMetadata,
    StationView, SCHEMA_VERSION,
};
pub use config::{
    BranchConfig, ColumnLayout, CorridorConfig, DiagramConfig, LabelConfig, PublicationConfig,
    SecondsDisplay, StationSpacing, ThemeConfig, TimetableConfig, TripLabelMode, CONFIG_VERSION,
};
pub use corridor::{
    resolve_corridor, AxisDirection, Corridor, CorridorNode, CorridorPanel, CorridorPlan,
    DiagramTarget, RunMapping,
};
pub use diagram::{
    build_diagram, DiagramCallView, DiagramDocument, DiagramFrequencyBand, DiagramLayout,
    DiagramPoint, DiagramRun, LabelPlacement, TickLevel, TimeAxis, TimeTick,
};
pub use error::PublicationError;
pub use text::{Labels, Language, LocalizedText};
pub use timetable::{
    build_timetable, FrequencyNote, HourGroup, TimetableDeparture, TimetableDocument,
    TimetablePanel,
};
