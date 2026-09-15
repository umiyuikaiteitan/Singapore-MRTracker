use std::collections::HashSet;

use mrt_gtfs::{LineId, RailNetwork, StationId};
use serde::{Deserialize, Serialize};

use crate::{DisplayError, MAX_LED_COUNT, SCHEMA_VERSION};

/// Physical LED order and station assignments. Do not infer mappings from
/// file order: `index` is the wired address in the addressable-LED chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub schema_version: u8,
    pub layout_id: String,
    pub led_count: u16,
    pub pixels: Vec<LayoutPixel>,
    pub board_station: String,
}

/// One station marker with millimetre coordinates for fabrication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutPixel {
    pub index: u16,
    /// A public station code or exact GTFS station identifier; never a name.
    pub station: String,
    /// Explicit additional interchange or historical identifiers. Resolved
    /// stations are merged; unresolved identifiers generate warnings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub station_aliases: Vec<String>,
    /// Optional route identifier or line display name (case-insensitive).
    pub line: Option<String>,
    pub x_mm: f64,
    pub y_mm: f64,
}

pub(crate) struct ResolvedPixel {
    pub index: u16,
    pub stations: Vec<StationId>,
    pub lines: Vec<LineId>,
}

/// Board station group, wired pixels, and missing-alias diagnostics.
pub(crate) type ResolvedLayout = (Vec<StationId>, Vec<ResolvedPixel>, Vec<String>);

impl Layout {
    /// Validate all electrical indices, coordinates and network mappings.
    /// Unknown or ambiguous stations fail the complete layout; none silently
    /// disappear or move to a different LED.
    pub fn validate(&self, network: &RailNetwork) -> Result<(), DisplayError> {
        self.resolve(network).map(|_| ())
    }

    /// Report identifiers absent from this feed when another explicit alias
    /// resolved their physical marker. Log these complete diagnostics; frame
    /// text also reports the count but has a fixed embedded-display limit.
    pub fn warnings(&self, network: &RailNetwork) -> Result<Vec<String>, DisplayError> {
        self.resolve(network).map(|(_, _, warnings)| warnings)
    }

    /// Validate a layout before network data is available.
    pub fn validate_geometry(&self) -> Result<(), DisplayError> {
        let invalid = |message: &str| DisplayError::Layout(message.into());
        if self.schema_version != SCHEMA_VERSION {
            return Err(invalid("unsupported schema_version"));
        }
        if self.layout_id.is_empty()
            || self.layout_id.len() > 64
            || !self
                .layout_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(invalid(
                "layout_id must contain 1..=64 ASCII letters, digits, '-' or '_'",
            ));
        }
        if self.led_count == 0 || self.led_count > MAX_LED_COUNT {
            return Err(invalid("led_count must be 1..=512"));
        }
        if self.pixels.len() != usize::from(self.led_count) {
            return Err(invalid(
                "pixels must cover every index from 0 to led_count - 1",
            ));
        }
        let mut seen = HashSet::new();
        for pixel in &self.pixels {
            if pixel.index >= self.led_count || !seen.insert(pixel.index) {
                return Err(invalid("duplicate or out-of-range pixel index"));
            }
            if !valid_identifier(&pixel.station)
                || pixel
                    .line
                    .as_ref()
                    .is_some_and(|line| !valid_identifier(line))
                || pixel.station_aliases.len() > 16
                || pixel
                    .station_aliases
                    .iter()
                    .any(|alias| !valid_identifier(alias))
            {
                return Err(invalid(
                    "station and line identifiers must be nonblank, bounded and unpadded",
                ));
            }
            if !pixel.x_mm.is_finite()
                || !pixel.y_mm.is_finite()
                || !(0.0..=2000.0).contains(&pixel.x_mm)
                || !(0.0..=2000.0).contains(&pixel.y_mm)
            {
                return Err(invalid(
                    "pixel coordinates must be finite millimetres in 0..=2000",
                ));
            }
        }
        if !valid_identifier(&self.board_station) {
            return Err(invalid(
                "board_station must be a nonblank, bounded, unpadded identifier",
            ));
        }
        Ok(())
    }

    pub(crate) fn resolve(&self, network: &RailNetwork) -> Result<ResolvedLayout, DisplayError> {
        self.validate_geometry()?;
        let mut warnings = Vec::new();
        let mut board = Vec::new();
        let mut pixels = Vec::with_capacity(self.pixels.len());
        for pixel in &self.pixels {
            let mut stations = Vec::new();
            for key in std::iter::once(&pixel.station).chain(&pixel.station_aliases) {
                match resolve_station(network, key)? {
                    Some(station) => {
                        if !stations.contains(&station) {
                            stations.push(station);
                        }
                    }
                    None => warnings.push(format!(
                        "pixel {}: station identifier '{}' absent; explicit alias used",
                        pixel.index, key
                    )),
                }
            }
            if stations.is_empty() {
                return Err(DisplayError::Layout(format!(
                    "unknown station '{}' and aliases {:?}; use a public code or GTFS station ID",
                    pixel.station, pixel.station_aliases
                )));
            }
            let mut lines: Vec<LineId> = stations
                .iter()
                .flat_map(|&station| network.station(station).lines.iter())
                .copied()
                .filter(|&id| {
                    pixel.line.as_ref().map_or(true, |filter| {
                        let line = network.line(id);
                        filter.eq_ignore_ascii_case(&line.name)
                            || filter.eq_ignore_ascii_case(&line.route_id)
                    })
                })
                .collect();
            lines.sort_unstable();
            lines.dedup();
            if lines.is_empty() {
                return Err(DisplayError::Layout(format!(
                    "pixel {}: station '{}' has no service matching line {:?}",
                    pixel.index, pixel.station, pixel.line
                )));
            }
            if std::iter::once(&pixel.station)
                .chain(&pixel.station_aliases)
                .any(|key| key.eq_ignore_ascii_case(&self.board_station))
            {
                for &station in &stations {
                    if !board.contains(&station) {
                        board.push(station);
                    }
                }
            }
            pixels.push(ResolvedPixel {
                index: pixel.index,
                stations,
                lines,
            });
        }
        if board.is_empty() {
            board.push(
                resolve_station(network, &self.board_station)?.ok_or_else(|| {
                    DisplayError::Layout(format!("unknown board station '{}'", self.board_station))
                })?,
            );
        }
        pixels.sort_by_key(|pixel| pixel.index);
        Ok((board, pixels, warnings))
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn resolve_station(network: &RailNetwork, key: &str) -> Result<Option<StationId>, DisplayError> {
    // Scan to detect collisions instead of trusting an index whose last
    // insertion can silently replace an earlier station with the same code.
    let matches: Vec<_> = network
        .stations()
        .iter()
        .enumerate()
        .filter(|(_, station)| {
            station.gtfs_id == key
                || station
                    .codes
                    .iter()
                    .any(|code| code.eq_ignore_ascii_case(key))
        })
        .map(|(index, _)| StationId(index))
        .collect();
    match matches.as_slice() {
        [station] => Ok(Some(*station)),
        [] => Ok(None),
        _ => Err(DisplayError::Layout(format!("ambiguous station '{key}'"))),
    }
}
