//! The publication configuration.
//!
//! One configuration drives both outputs. It carries the presentation
//! choices that GTFS cannot answer: how long the service day is, how
//! many columns a printed timetable uses, which direction headings to
//! override, and which corridor a diagram draws.
//!
//! The structures derive `serde::Deserialize`, so any format that maps
//! onto serde can supply them. The command line reads YAML.
//!
//! Everything here is a *presentation* choice. No option here invents
//! schedule data. An option that changes what the data means — the
//! frequency policy and the missing-time policy — names the policy
//! explicitly and the renderers mark the result.

use std::collections::BTreeMap;

use mrt_gtfs::{FrequencyPolicy, GtfsTime, MissingTimePolicy};
use serde::{Deserialize, Serialize};

use crate::text::{Language, LocalizedText};

/// The configuration schema version that this crate understands.
pub const CONFIG_VERSION: u32 = 1;

/// The whole publication configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PublicationConfig {
    /// The schema version of the configuration file.
    pub version: u32,
    /// A free-text profile name, for example `singapore-lta`.
    pub profile: Option<String>,
    /// The time zone that the output states, for example
    /// `Asia/Singapore`. When empty, the feed decides.
    pub timezone: Option<String>,
    /// The first hour of the timetable service day.
    #[serde(with = "gtfs_time_string")]
    pub day_start: GtfsTime,
    /// How many hours the service day covers.
    pub day_duration_hours: u32,
    /// How to treat headway-based service.
    pub frequency_policy: FrequencyPolicy,
    /// How to fill in missing stop times.
    pub missing_time_policy: MissingTimePolicy,
    /// The language of the user-interface labels.
    pub language: Language,
    /// The timetable options.
    pub timetable: TimetableConfig,
    /// The diagram options.
    pub diagram: DiagramConfig,
    /// The label overrides.
    pub labels: LabelConfig,
    /// The visual theme.
    pub theme: ThemeConfig,
    /// The corridor definitions for diagrams.
    pub corridors: Vec<CorridorConfig>,
}

impl Default for PublicationConfig {
    fn default() -> Self {
        PublicationConfig {
            version: CONFIG_VERSION,
            profile: None,
            timezone: None,
            day_start: GtfsTime::from_hms(4, 0, 0),
            day_duration_hours: 24,
            frequency_policy: FrequencyPolicy::Bands,
            missing_time_policy: MissingTimePolicy::InterpolateBounded,
            language: Language::En,
            timetable: TimetableConfig::default(),
            diagram: DiagramConfig::default(),
            labels: LabelConfig::default(),
            theme: ThemeConfig::default(),
            corridors: Vec::new(),
        }
    }
}

impl PublicationConfig {
    /// Get the exclusive end of the timetable service day.
    pub fn day_end(&self) -> GtfsTime {
        self.day_start.plus_seconds(self.day_duration_hours * 3600)
    }

    /// Check the configuration for values that cannot work.
    pub fn check(&self) -> Result<(), String> {
        if self.version != CONFIG_VERSION {
            return Err(format!(
                "the configuration states version {}, but this build understands version {CONFIG_VERSION}",
                self.version
            ));
        }
        if self.day_duration_hours == 0 || self.day_duration_hours > 48 {
            return Err(format!(
                "day_duration_hours is {}; use a value from 1 to 48",
                self.day_duration_hours
            ));
        }
        if self.timetable.columns == 0 || self.timetable.columns > 6 {
            return Err(format!(
                "timetable.columns is {}; use a value from 1 to 6",
                self.timetable.columns
            ));
        }
        for (name, value) in [
            ("major_grid_minutes", self.diagram.major_grid_minutes),
            ("medium_grid_minutes", self.diagram.medium_grid_minutes),
            ("minor_grid_minutes", self.diagram.minor_grid_minutes),
        ] {
            if value == 0 {
                return Err(format!("diagram.{name} must be greater than zero"));
            }
        }
        if self.diagram.pixels_per_hour <= 0.0 || self.diagram.row_height <= 0.0 {
            return Err(
                "diagram.pixels_per_hour and diagram.row_height must be greater than zero"
                    .to_string(),
            );
        }
        let mut seen = std::collections::BTreeSet::new();
        for corridor in &self.corridors {
            if corridor.id.trim().is_empty() {
                return Err("every corridor needs a non-empty id".to_string());
            }
            if !seen.insert(corridor.id.as_str()) {
                return Err(format!("the corridor id \"{}\" appears twice", corridor.id));
            }
            if corridor.axis.len() < 2 {
                return Err(format!(
                    "the corridor \"{}\" needs at least two stations on its axis",
                    corridor.id
                ));
            }
            for branch in &corridor.branches {
                if !corridor.axis.iter().any(|s| s == &branch.junction) {
                    return Err(format!(
                        "the branch junction \"{}\" of corridor \"{}\" is not on its axis",
                        branch.junction, corridor.id
                    ));
                }
            }
        }
        Ok(())
    }

    /// Find a corridor definition by identifier.
    pub fn corridor(&self, id: &str) -> Option<&CorridorConfig> {
        self.corridors.iter().find(|c| c.id == id)
    }
}

/// How a timetable panel splits into columns.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColumnLayout {
    /// One continuous hour table.
    Single,
    /// Split at the service hours in [`TimetableConfig::split_at`].
    SplitAt,
    /// Distribute the hour rows evenly, in order.
    Balanced,
    /// Balanced on a wide screen and in print, stacked on a narrow
    /// screen. This is the default.
    #[default]
    Responsive,
}

/// When a departure shows its seconds.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecondsDisplay {
    /// Never show seconds.
    Hide,
    /// Always show seconds.
    Show,
    /// Show seconds only when they are not zero. This is the default.
    #[default]
    ShowIfNonzero,
}

/// The timetable options.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TimetableConfig {
    /// How the panel splits into columns.
    pub layout: ColumnLayout,
    /// How many columns a balanced or responsive layout uses.
    pub columns: usize,
    /// The service hours at which [`ColumnLayout::SplitAt`] breaks.
    pub split_at: Vec<u32>,
    /// Show an hour row that carries no departure.
    pub show_empty_hours: bool,
    /// When a departure shows its seconds.
    pub seconds: SecondsDisplay,
    /// Give each platform its own panel, when the feed names
    /// platforms.
    pub group_by_platform: bool,
    /// Give each destination its own panel.
    pub split_by_destination: bool,
    /// Mark the first and the last departure of each panel.
    pub mark_first_and_last: bool,
    /// Show the public trip name next to a departure, when the feed
    /// supplies one.
    pub show_trip_short_name: bool,
    /// The page title. `{station}`, `{line}`, and `{date}` fill in.
    pub title: LocalizedText,
}

impl Default for TimetableConfig {
    fn default() -> Self {
        TimetableConfig {
            layout: ColumnLayout::Responsive,
            columns: 2,
            split_at: Vec::new(),
            show_empty_hours: true,
            seconds: SecondsDisplay::ShowIfNonzero,
            group_by_platform: true,
            split_by_destination: false,
            mark_first_and_last: true,
            show_trip_short_name: true,
            title: LocalizedText::both("{station} departure timetable", "{station} 発車時刻表"),
        }
    }
}

/// How a diagram spaces the stations on the vertical axis.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StationSpacing {
    /// Every station gets the same height. This is the default,
    /// because it keeps dense sections legible.
    #[default]
    Equal,
    /// Space the stations by the distance between them. Falls back to
    /// equal spacing when the feed carries no usable distance.
    Distance,
    /// Use the offsets from the corridor configuration.
    Manual,
}

/// When a diagram writes a label on a train path.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TripLabelMode {
    /// Never write a label.
    Never,
    /// Write a label where one fits without overlapping. This is the
    /// default.
    #[default]
    Auto,
    /// Write every label, even where they overlap.
    Always,
}

/// The diagram options.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiagramConfig {
    /// How the stations are spaced.
    pub station_spacing: StationSpacing,
    /// The interval of the strong grid lines, in minutes.
    pub major_grid_minutes: u32,
    /// The interval of the medium grid lines, in minutes.
    pub medium_grid_minutes: u32,
    /// The interval of the faint grid lines, in minutes.
    pub minor_grid_minutes: u32,
    /// Draw the horizontal segment where a train stands at a station.
    pub show_dwell: bool,
    /// When to write a label on a train path.
    pub show_trip_labels: TripLabelMode,
    /// Put the internal GTFS `trip_id` in the hover details.
    ///
    /// The identifier never appears as a passenger-facing train
    /// number, whatever this option says.
    pub show_internal_trip_ids: bool,
    /// The width of one hour, in user units.
    pub pixels_per_hour: f64,
    /// The height of one station row, in user units.
    pub row_height: f64,
    /// The page title. `{line}`, `{corridor}`, and `{date}` fill in.
    pub title: LocalizedText,
}

impl Default for DiagramConfig {
    fn default() -> Self {
        DiagramConfig {
            station_spacing: StationSpacing::Equal,
            major_grid_minutes: 60,
            medium_grid_minutes: 30,
            minor_grid_minutes: 10,
            show_dwell: true,
            show_trip_labels: TripLabelMode::Auto,
            show_internal_trip_ids: false,
            pixels_per_hour: 240.0,
            row_height: 34.0,
            title: LocalizedText::both("{corridor} train diagram", "{corridor} 列車ダイヤグラム"),
        }
    }
}

/// The label overrides.
///
/// Every override is explicit. Nothing here is derived from the feed,
/// and nothing in the renderers invents a label that is not either in
/// the feed or in this section.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LabelConfig {
    /// Direction headings, keyed by `<route_id>:<direction_id>`, for
    /// example `NS:0`. Use `NS:none` for a route without a direction.
    pub direction_overrides: BTreeMap<String, LocalizedText>,
    /// Short forms of destination names, keyed by the full name.
    pub destination_abbreviations: BTreeMap<String, String>,
    /// Platform labels, keyed by the GTFS `stop_id` of the platform.
    pub platform_overrides: BTreeMap<String, String>,
    /// Corridor headings, keyed by the corridor identifier.
    pub corridor_overrides: BTreeMap<String, LocalizedText>,
}

impl LabelConfig {
    /// Build the key of a direction override.
    pub fn direction_key(route_id: &str, direction: Option<u8>) -> String {
        match direction {
            Some(value) => format!("{route_id}:{value}"),
            None => format!("{route_id}:none"),
        }
    }

    /// Get the configured direction heading, if one exists.
    pub fn direction_override(
        &self,
        route_id: &str,
        direction: Option<u8>,
        language: Language,
    ) -> Option<String> {
        self.direction_overrides
            .get(&Self::direction_key(route_id, direction))
            .map(|text| text.get(language).to_string())
    }

    /// Get the short form of a destination, or the name itself.
    pub fn abbreviate<'a>(&'a self, destination: &'a str) -> &'a str {
        self.destination_abbreviations
            .get(destination)
            .map(String::as_str)
            .unwrap_or(destination)
    }
}

/// The visual theme.
///
/// The renderers turn these values into CSS custom properties. The
/// font stack names system and openly licensed families only; the
/// generator embeds no font file.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    /// The font families, in order of preference.
    pub font_stack: Vec<String>,
    /// The background of the hour cells, as a CSS color.
    pub hour_cell: String,
    /// The text color on the hour cells.
    pub hour_cell_text: String,
    /// The background of an alternating row.
    pub row_alternate: String,
    /// The page background.
    pub background: String,
    /// The page text color.
    pub text: String,
    /// The fallback accent when a line carries no color.
    pub accent: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        ThemeConfig {
            font_stack: [
                "Noto Sans",
                "Noto Sans JP",
                "Hiragino Kaku Gothic ProN",
                "Arial",
                "sans-serif",
            ]
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
            hour_cell: "#1b2a5e".to_string(),
            hour_cell_text: "#ffffff".to_string(),
            row_alternate: "#eef1f8".to_string(),
            background: "#ffffff".to_string(),
            text: "#14171f".to_string(),
            accent: "#1b2a5e".to_string(),
        }
    }
}

/// A hand-written corridor for a diagram.
///
/// A corridor names the stations of the vertical axis. Use it when the
/// automatic derivation cannot put the patterns of a line on one axis,
/// for example on a line with a branch.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CorridorConfig {
    /// The identifier that the command line selects.
    pub id: String,
    /// The line whose trips the corridor draws. A GTFS `route_id` or a
    /// route short name.
    pub line: Option<String>,
    /// The heading of the corridor.
    pub label: Option<LocalizedText>,
    /// The stations of the main axis, as station codes or GTFS
    /// identifiers, in travel order.
    pub axis: Vec<String>,
    /// The branches that leave the main axis.
    pub branches: Vec<BranchConfig>,
    /// Manual vertical offsets, one per axis entry, for
    /// [`StationSpacing::Manual`].
    pub offsets: Vec<f64>,
}

/// A branch of a corridor.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BranchConfig {
    /// The station of the main axis where the branch leaves it.
    pub junction: String,
    /// The stations of the branch, in travel order away from the
    /// junction.
    pub axis: Vec<String>,
    /// The heading of the branch panel.
    pub label: Option<LocalizedText>,
}

/// Serialize and deserialize a [`GtfsTime`] as an `HH:MM:SS` string.
mod gtfs_time_string {
    use mrt_gtfs::GtfsTime;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &GtfsTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<GtfsTime, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_service_day_runs_from_04_00_to_28_00() {
        let config = PublicationConfig::default();
        assert_eq!(config.day_start.to_string(), "04:00:00");
        assert_eq!(config.day_end().to_string(), "28:00:00");
        assert!(config.check().is_ok());
    }

    #[test]
    fn a_wrong_schema_version_is_rejected() {
        let config = PublicationConfig {
            version: 99,
            ..Default::default()
        };
        assert!(config.check().unwrap_err().contains("version"));
    }

    #[test]
    fn impossible_layouts_are_rejected() {
        let mut config = PublicationConfig::default();
        config.timetable.columns = 0;
        // (the nested field cannot move into the initializer)
        assert!(config.check().unwrap_err().contains("columns"));

        let mut config = PublicationConfig::default();
        config.diagram.minor_grid_minutes = 0;
        assert!(config.check().unwrap_err().contains("minor_grid_minutes"));
    }

    #[test]
    fn a_branch_junction_must_lie_on_the_axis() {
        let mut config = PublicationConfig::default();
        config.corridors.push(CorridorConfig {
            id: "main".into(),
            axis: vec!["A".into(), "B".into()],
            branches: vec![BranchConfig {
                junction: "Z".into(),
                axis: vec!["C".into()],
                label: None,
            }],
            ..Default::default()
        });
        assert!(config.check().unwrap_err().contains("junction"));
    }

    #[test]
    fn duplicate_corridor_ids_are_rejected() {
        let mut config = PublicationConfig::default();
        for _ in 0..2 {
            config.corridors.push(CorridorConfig {
                id: "main".into(),
                axis: vec!["A".into(), "B".into()],
                ..Default::default()
            });
        }
        assert!(config.check().unwrap_err().contains("twice"));
    }

    #[test]
    fn direction_overrides_use_a_stable_key() {
        let mut labels = LabelConfig::default();
        labels.direction_overrides.insert(
            "NS:0".to_string(),
            LocalizedText::both("Southbound", "下り"),
        );
        assert_eq!(
            labels
                .direction_override("NS", Some(0), Language::Ja)
                .as_deref(),
            Some("下り")
        );
        assert_eq!(labels.direction_override("NS", Some(1), Language::En), None);
        assert_eq!(LabelConfig::direction_key("NS", None), "NS:none");
    }

    #[test]
    fn abbreviations_fall_back_to_the_full_name() {
        let mut labels = LabelConfig::default();
        labels
            .destination_abbreviations
            .insert("Marina Bay".into(), "Mar Bay".into());
        assert_eq!(labels.abbreviate("Marina Bay"), "Mar Bay");
        assert_eq!(labels.abbreviate("Jurong East"), "Jurong East");
    }
}
