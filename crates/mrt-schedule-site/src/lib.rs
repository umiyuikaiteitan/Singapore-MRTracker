//! # mrt-schedule-site
//!
//! Generate a browsable static site of station departure timetables
//! and train diagrams, for GitHub Pages or any file host.
//!
//! The site is a section that sits beside the live board, under the
//! same domain:
//!
//! ```text
//! site/
//!   index.html                    the live departure board
//!   timetables/
//!     index.html                  the hub: today
//!     day-<YYYYMMDD>.html         the hub: another service date
//!     t/<code>-<date>.html        one station timetable
//!     d/<line>-<date>-<w>.html    one train diagram
//!     d/<line>-<date>-<w>.svg     the drawing on its own
//!     data/index.json             the machine-readable index
//! ```
//!
//! Every page is the same self-contained document that
//! `mrt-schedule-cli` writes, plus a navigation block. Every link is
//! relative, because a GitHub Pages project site lives under
//! `/<repository>/`.
//!
//! # Why pre-generated pages
//!
//! The pages are the product. Rendering them once in the build keeps
//! one renderer, one set of tests, and one escaping discipline; a
//! browser-side renderer would mirror the markup and drift from it.
//! It also means a visitor loads one file and can then print it, save
//! it, or read it on a train with no signal.

#![warn(missing_docs)]

pub mod build;
pub mod hub;
mod page;
pub mod plan;

pub use build::{accepts_partial, today_at_offset, BuildReport, SiteBuild, Verdict, WrittenPages};
pub use hub::SiteInfo;
pub use plan::{default_windows, DateEntry, LineEntry, SitePlan, StationEntry, WindowEntry};

/// The offset of Singapore Standard Time from UTC, in seconds.
pub const SGT_OFFSET_SECS: i64 = 8 * 3600;
