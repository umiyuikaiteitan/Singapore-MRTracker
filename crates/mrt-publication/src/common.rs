//! View types that both documents share.

use mrt_gtfs::{
    Diagnostic, GtfsTime, Line, LineId, RailNetwork, ServiceDate, Severity, Station, StationId,
};
use serde::Serialize;

use crate::config::PublicationConfig;
use crate::text::{Labels, LocalizedText};

/// The version of the JSON view-model schema.
///
/// A change that removes or renames a field raises the major part. A
/// change that only adds a field raises the minor part.
pub const SCHEMA_VERSION: &str = "1.0";

/// The facts about the source of a document.
///
/// The caller fills this in. It carries no clock reading, so two runs
/// over the same feed and configuration produce identical documents.
#[derive(Clone, Debug, Default)]
pub struct DocumentSeed {
    /// The version of the program that generated the document.
    pub generator_version: String,
    /// The SHA-256 of the feed archive or directory.
    pub feed_sha256: String,
    /// The timestamp that the feed publisher stated, if known.
    pub feed_timestamp: Option<String>,
    /// The time zone of the schedule.
    pub timezone: String,
    /// Whether the feed came from the local cache after a failed
    /// download.
    pub generated_from_cache: bool,
    /// The SHA-256 of the configuration.
    pub configuration_sha256: String,
}

/// The provenance block of a document.
#[derive(Clone, Debug, Serialize)]
pub struct PublicationMetadata {
    /// The version of the JSON view-model schema.
    pub schema_version: String,
    /// The version of the program that generated the document.
    pub generator_version: String,
    /// The SHA-256 of the feed.
    pub feed_sha256: String,
    /// The timestamp that the feed publisher stated.
    pub feed_timestamp: Option<String>,
    /// The service date of the document.
    pub service_date: ServiceDate,
    /// The time zone of the schedule.
    pub timezone: String,
    /// Whether the feed came from the cache.
    pub generated_from_cache: bool,
    /// The SHA-256 of the configuration.
    pub configuration_sha256: String,
    /// The human-readable warnings that the projection produced.
    pub warnings: Vec<String>,
    /// The full diagnostics, for tools.
    pub diagnostics: Vec<Diagnostic>,
}

impl PublicationMetadata {
    /// Build the metadata from a seed, a date, and the diagnostics.
    pub fn new(
        seed: &DocumentSeed,
        service_date: ServiceDate,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        let warnings = diagnostics
            .iter()
            .filter(|d| d.severity >= Severity::Warning)
            .map(|d| d.to_string())
            .collect();
        PublicationMetadata {
            schema_version: SCHEMA_VERSION.to_string(),
            generator_version: seed.generator_version.clone(),
            feed_sha256: seed.feed_sha256.clone(),
            feed_timestamp: seed.feed_timestamp.clone(),
            service_date,
            timezone: seed.timezone.clone(),
            generated_from_cache: seed.generated_from_cache,
            configuration_sha256: seed.configuration_sha256.clone(),
            warnings,
            diagnostics,
        }
    }

    /// Get the first eight characters of the feed fingerprint, for a
    /// compact footer.
    pub fn short_feed_sha(&self) -> &str {
        let end = self.feed_sha256.len().min(12);
        &self.feed_sha256[..end]
    }
}

/// A station, as a document shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StationView {
    /// The GTFS identifier of the station.
    pub gtfs_id: String,
    /// The public name.
    pub name: String,
    /// The public codes, for example `NS1` and `EW24`.
    pub codes: Vec<String>,
}

impl StationView {
    /// Build the view of one station.
    pub fn of(network: &RailNetwork, id: StationId) -> Self {
        Self::from_station(network.station(id))
    }

    /// Build the view from a station record.
    pub fn from_station(station: &Station) -> Self {
        StationView {
            gtfs_id: station.gtfs_id.clone(),
            name: station.name.clone(),
            codes: station.codes.clone(),
        }
    }

    /// Get the primary code, when the station has one.
    pub fn primary_code(&self) -> Option<&str> {
        self.codes.first().map(String::as_str)
    }
}

/// A line, as a document shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LineView {
    /// The GTFS route identifier.
    pub route_id: String,
    /// The display name, for example `NSL`.
    pub name: String,
    /// The long name, for example `North South Line`.
    pub long_name: Option<String>,
    /// The line color as a CSS color, for example `#D42E12`.
    pub color: Option<String>,
    /// The text color on the line color.
    pub text_color: Option<String>,
    /// A CSS-safe key derived from the route identifier.
    pub key: String,
}

impl LineView {
    /// Build the view of one line.
    pub fn of(network: &RailNetwork, id: LineId) -> Self {
        Self::from_line(network.line(id))
    }

    /// Build the view from a line record.
    pub fn from_line(line: &Line) -> Self {
        LineView {
            route_id: line.route_id.clone(),
            name: line.name.clone(),
            long_name: line.long_name.clone(),
            color: line.color.as_deref().map(css_color),
            text_color: line.text_color.as_deref().map(css_color),
            key: css_key(&line.route_id),
        }
    }
}

/// Turn a six-digit GTFS color into a CSS color.
///
/// The check is strict: anything that is not exactly six hexadecimal
/// digits, with an optional leading `#`, yields an empty string. A
/// stylesheet therefore cannot receive a value that a hostile feed
/// chose, whatever the feed carries in `route_color`.
pub fn css_color(value: &str) -> String {
    let digits = value.strip_prefix('#').unwrap_or(value);
    if digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        format!("#{digits}")
    } else {
        String::new()
    }
}

/// Turn any identifier into a key that is safe in a CSS class, an
/// HTML identifier, and a JavaScript object key.
pub fn css_key(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "x".to_string()
    } else {
        trimmed
    }
}

/// One entry of the legend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LegendItem {
    /// A stable key, for example `approximate`.
    pub key: String,
    /// The symbol that appears in the output, if the entry has one.
    pub symbol: Option<String>,
    /// The explanation.
    pub label: String,
}

impl LegendItem {
    /// Make a legend entry with a symbol.
    pub fn with_symbol(key: &str, symbol: &str, label: &str) -> Self {
        LegendItem {
            key: key.to_string(),
            symbol: Some(symbol.to_string()),
            label: label.to_string(),
        }
    }

    /// Make a legend entry without a symbol.
    pub fn plain(key: &str, label: &str) -> Self {
        LegendItem {
            key: key.to_string(),
            symbol: None,
            label: label.to_string(),
        }
    }
}

/// A mark on one departure or one train path.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DepartureFlag {
    /// The time comes from a headway, not from a schedule.
    Approximate,
    /// The library computed the time between two scheduled times.
    Interpolated,
    /// The first departure of the panel on this service day.
    FirstOfDay,
    /// The last departure of the panel on this service day.
    LastOfDay,
    /// The departure happens after midnight, on the next calendar day.
    PastMidnight,
}

impl DepartureFlag {
    /// Get the stable key of the flag.
    pub const fn key(self) -> &'static str {
        match self {
            DepartureFlag::Approximate => "approximate",
            DepartureFlag::Interpolated => "interpolated",
            DepartureFlag::FirstOfDay => "first",
            DepartureFlag::LastOfDay => "last",
            DepartureFlag::PastMidnight => "past-midnight",
        }
    }

    /// Get the symbol that the renderers print.
    ///
    /// The symbols are plain characters, so they render with any font
    /// and stay legible in a monochrome print.
    pub const fn symbol(self) -> &'static str {
        match self {
            DepartureFlag::Approximate => "~",
            DepartureFlag::Interpolated => "*",
            DepartureFlag::FirstOfDay => "\u{25B7}",
            DepartureFlag::LastOfDay => "\u{25C1}",
            DepartureFlag::PastMidnight => "\u{2020}",
        }
    }

    /// Get the explanation of the flag.
    pub fn explanation(self, labels: &Labels) -> &'static str {
        match self {
            DepartureFlag::Approximate => labels.legend_approximate,
            DepartureFlag::Interpolated => labels.legend_interpolated,
            DepartureFlag::FirstOfDay => labels.legend_first,
            DepartureFlag::LastOfDay => labels.legend_last,
            DepartureFlag::PastMidnight => labels.legend_past_midnight,
        }
    }
}

/// Format a time on the 24-hour clock, for example `25:35:00` as
/// `01:35`.
pub fn clock_hhmm(time: GtfsTime) -> String {
    let clock = GtfsTime::from_seconds(time.clock_seconds());
    format!("{:02}:{:02}", clock.hours(), clock.minutes())
}

/// Format a time on the service day, for example `25:35`.
pub fn service_hhmm(time: GtfsTime) -> String {
    format!("{:02}:{:02}", time.hours(), time.minutes())
}

/// Round a coordinate to two decimal places.
///
/// Geometry is written into JSON and SVG. Rounding keeps snapshots
/// byte-identical across platforms with different floating-point
/// formatting.
pub fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Resolve the title of a document from the configuration template.
pub fn fill_title(
    template: &LocalizedText,
    replacements: &[(&str, &str)],
    config: &PublicationConfig,
) -> LocalizedText {
    let _ = config;
    template.fill(replacements)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_keys_are_safe_and_stable() {
        assert_eq!(css_key("NS"), "ns");
        assert_eq!(css_key("CC_a"), "cc-a");
        assert_eq!(css_key("EW 24 / X"), "ew-24-x");
        assert_eq!(css_key("--"), "x");
        assert_eq!(css_key(""), "x");
    }

    #[test]
    fn css_colors_only_carry_hexadecimal_digits() {
        assert_eq!(css_color("D42E12"), "#D42E12");
        assert_eq!(css_color("#D42E12"), "#D42E12");
        // A hostile value cannot escape into the stylesheet.
        assert_eq!(css_color("red;}body{display:none"), "");
        assert_eq!(css_color(""), "");
    }

    #[test]
    fn clock_formatting_wraps_past_midnight() {
        let late: GtfsTime = "25:35:00".parse().unwrap();
        assert_eq!(clock_hhmm(late), "01:35");
        assert_eq!(service_hhmm(late), "25:35");
    }

    #[test]
    fn rounding_keeps_two_decimal_places() {
        assert_eq!(round2(1.005), 1.0);
        assert_eq!(round2(1.006), 1.01);
        assert_eq!(round2(-2.345), -2.35);
    }

    #[test]
    fn flags_carry_distinct_symbols() {
        let all = [
            DepartureFlag::Approximate,
            DepartureFlag::Interpolated,
            DepartureFlag::FirstOfDay,
            DepartureFlag::LastOfDay,
            DepartureFlag::PastMidnight,
        ];
        let mut symbols: Vec<&str> = all.iter().map(|f| f.symbol()).collect();
        symbols.sort_unstable();
        symbols.dedup();
        assert_eq!(symbols.len(), all.len());
    }
}
