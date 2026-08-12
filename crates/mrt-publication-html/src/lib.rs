//! # mrt-publication-html
//!
//! Render the view models of `mrt-publication` as self-contained HTML
//! and SVG.
//!
//! The crate takes a finished document and returns a `String`. It
//! reads no file, writes no file, and never touches the network or a
//! `RailNetwork`: everything it needs is already in the document.
//!
//! # What the generated pages guarantee
//!
//! - **Self-contained.** One file, no external stylesheet, script,
//!   font, or image. A `Content-Security-Policy` of `default-src
//!   'none'` is embedded, so the page cannot make a request even if
//!   one were added by accident.
//! - **Readable without JavaScript.** The timetable is a table. The
//!   diagram is an SVG plus a call table for every run. The scripts
//!   only add zoom, filters, and highlighting.
//! - **Escaped.** Every string from the feed goes through
//!   [`escape::text`] or [`escape::attr`]. Colors and font names pass
//!   a strict filter before they reach the stylesheet, so a hostile
//!   `route_color` cannot inject a declaration.
//! - **No borrowed branding.** The pages use the configured font
//!   stack, which names system and openly licensed families. The
//!   crate embeds no font file and no logo.
//! - **Printable.** A4 portrait and landscape for the timetable, A3
//!   landscape for the diagram, plus a monochrome profile.
//!
//! # Example
//!
//! ```no_run
//! use mrt_gtfs::{GtfsFeed, RailNetwork};
//! use mrt_publication::{build_timetable, DocumentSeed, PublicationConfig};
//! use mrt_publication_html::render_timetable;
//!
//! let feed = GtfsFeed::from_zip_path("data/singapore-gtfs.zip").unwrap();
//! let network = RailNetwork::from_feed(&feed).unwrap();
//! let station = network.station_by_alias("ns1").unwrap();
//! let config = PublicationConfig::default();
//!
//! let document = build_timetable(
//!     &network,
//!     station,
//!     "20260810".parse().unwrap(),
//!     None,
//!     &config,
//!     &DocumentSeed::default(),
//! )
//! .unwrap();
//! std::fs::write("timetable.html", render_timetable(&document, &config)).unwrap();
//! ```

#![warn(missing_docs)]

mod diagram;
pub mod escape;
pub mod nav;
mod page;
mod svg;
mod timetable;

pub use diagram::{render_diagram, render_diagram_svg, render_diagram_with_nav};
pub use nav::{NavGroup, NavLink, PageNav};
pub use page::CSP;
pub use svg::{render_svg, SvgMode};
pub use timetable::{render_timetable, render_timetable_with_nav};

/// Format a service-day time as `HH:MM`, keeping hours past 24.
pub(crate) fn common_time(time: mrt_gtfs::GtfsTime) -> String {
    format!("{:02}:{:02}", time.hours(), time.minutes())
}
