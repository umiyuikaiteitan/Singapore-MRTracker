//! Pure projection from a rail timetable and optional realtime feed into
//! complete LED/e-ink frames. Pixels mean **upcoming station departures**,
//! never train positions. The caller owns fetching, credentials and clocks.
//!
//! A fresh feed alone does not make a departure realtime: a matching,
//! fresh trip update must supply an applicable delay. Unsupported updates
//! retain scheduled provenance. See the crate README for matching limits.

mod layout;
mod project;

pub use layout::{Layout, LayoutPixel};
pub use project::build_frame;

use mrt_gtfs::{GtfsTime, ServiceDate};
use serde::{Deserialize, Serialize};

/// Version of both the layout and frame wire contracts.
pub const SCHEMA_VERSION: u8 = 1;
/// Bound frame allocations and embedded consumer memory requirements.
pub const MAX_LED_COUNT: u16 = 512;

/// Invalid layout, query or projection options.
#[derive(Debug, thiserror::Error)]
pub enum DisplayError {
    /// Invalid or ambiguous physical mapping.
    #[error("invalid display layout: {0}")]
    Layout(String),
    /// Invalid time window, brightness or lifetime setting.
    #[error("invalid display options: {0}")]
    Options(String),
    /// A timetable query failed.
    #[error(transparent)]
    Schedule(#[from] mrt_gtfs::GtfsError),
}

/// Data provenance for the whole frame. Individual rows retain their own
/// `realtime` flag when the frame contains a mixture of sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    /// At least one displayed departure uses a fresh matched delay, or
    /// an active cancellation changes the displayed service.
    Realtime,
    /// Only the supplied timetable contributes departure predictions.
    Schedule,
    /// Realtime input failed freshness checks; timetable fallback is shown.
    Stale,
    /// The caller could not provide usable schedule data.
    Unavailable,
}

/// A complete, non-delta RGB pixel value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pixel {
    pub index: u16,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// A compact destination-board row, rounded up to whole minutes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardRow {
    pub line: String,
    pub destination: String,
    pub minutes: u32,
    pub approximate: bool,
    pub realtime: bool,
}

/// Text for an e-ink or matrix display. Render the notice visibly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    pub station_name: String,
    pub rows: Vec<BoardRow>,
    pub notice: String,
}

/// Stable version-one JSON message. The consumer must reject a different
/// layout/version and blank live pixels when `valid_until` is reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub schema_version: u8,
    pub layout_id: String,
    pub generated_at: u64,
    pub valid_until: u64,
    pub source_state: SourceState,
    pub pixels: Vec<Pixel>,
    pub board: Board,
}

impl Frame {
    /// Produce an explicit unavailable frame without a loaded network.
    /// Geometry still has to be valid; every configured LED is black.
    pub fn unavailable(
        layout: &Layout,
        now_unix: u64,
        ttl_secs: u32,
        notice: &str,
    ) -> Result<Self, DisplayError> {
        layout.validate_geometry()?;
        let valid_until = checked_expiry(now_unix, ttl_secs)?;
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            layout_id: layout.layout_id.clone(),
            generated_at: now_unix,
            valid_until,
            source_state: SourceState::Unavailable,
            pixels: black_pixels(layout.led_count),
            board: Board {
                station_name: compact_text(&layout.board_station, 80),
                rows: Vec::new(),
                notice: compact_text(&format!("Data unavailable. {notice}"), 240),
            },
        })
    }
}

/// Explicit wall clock and freshness policy. `service_date` and `clock`
/// describe the current Singapore calendar date and 24-hour local clock;
/// previous-day GTFS times above 24:00 are queried automatically.
#[derive(Debug, Clone)]
pub struct FrameOptions {
    pub now_unix: u64,
    pub service_date: ServiceDate,
    pub clock: GtfsTime,
    pub lookahead_secs: u32,
    pub ttl_secs: u32,
    pub max_rt_age_secs: u32,
    pub max_rows: usize,
}

impl FrameOptions {
    /// A 30-minute horizon, six rows and 120-second frame/RT lifetime.
    pub fn new(now_unix: u64, service_date: ServiceDate, clock: GtfsTime) -> Self {
        Self {
            now_unix,
            service_date,
            clock,
            lookahead_secs: 1800,
            ttl_secs: 120,
            max_rt_age_secs: 120,
            max_rows: 6,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), DisplayError> {
        if self.clock.seconds() >= 86_400 {
            return Err(DisplayError::Options(
                "clock must be a local 24-hour clock".into(),
            ));
        }
        if !(1..=21_600).contains(&self.lookahead_secs) {
            return Err(DisplayError::Options(
                "lookahead_secs must be 1..=21600".into(),
            ));
        }
        if !(1..=3600).contains(&self.max_rt_age_secs) {
            return Err(DisplayError::Options(
                "max_rt_age_secs must be 1..=3600".into(),
            ));
        }
        if !(1..=8).contains(&self.max_rows) {
            return Err(DisplayError::Options("max_rows must be 1..=8".into()));
        }
        checked_expiry(self.now_unix, self.ttl_secs)?;
        Ok(())
    }
}

pub(crate) fn checked_expiry(now: u64, ttl: u32) -> Result<u64, DisplayError> {
    if !(1..=300).contains(&ttl) {
        return Err(DisplayError::Options("ttl_secs must be 1..=300".into()));
    }
    now.checked_add(u64::from(ttl))
        .ok_or_else(|| DisplayError::Options("timestamp overflow".into()))
}

pub(crate) fn black_pixels(count: u16) -> Vec<Pixel> {
    (0..count)
        .map(|index| Pixel {
            index,
            r: 0,
            g: 0,
            b: 0,
        })
        .collect()
}

pub(crate) fn compact_text(text: &str, max_bytes: usize) -> String {
    let mut text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.len() > max_bytes {
        let mut end = max_bytes;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}
