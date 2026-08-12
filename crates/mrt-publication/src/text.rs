//! Localized text and the user-interface label catalogue.
//!
//! Two kinds of text appear in a published document:
//!
//! 1. Text from the feed — station names, headsigns, platform codes.
//!    It stays exactly as the feed supplies it.
//! 2. Text from the user interface — "Platform", "every 4 min
//!    approximately", "Departures". This module holds it.
//!
//! The catalogue carries English and Japanese. English is the default.
//! Japanese labels are a presentation choice; nothing in this module
//! infers Japanese railway concepts, such as up and down directions,
//! from GTFS data.

use serde::{Deserialize, Serialize};

/// The language of the user-interface labels.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// English. The default.
    #[default]
    En,
    /// Japanese.
    Ja,
}

impl Language {
    /// Get the IETF language tag, for the HTML `lang` attribute.
    pub const fn tag(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Ja => "ja",
        }
    }
}

impl std::str::FromStr for Language {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" | "english" => Ok(Language::En),
            "ja" | "jp" | "japanese" => Ok(Language::Ja),
            other => Err(format!(
                "\"{other}\" is not a supported language; use \"en\" or \"ja\""
            )),
        }
    }
}

/// A short text in the supported languages.
///
/// The English form is required, because it is the fallback. In a
/// configuration file the value may be a plain string, which fills in
/// the English form:
///
/// ```yaml
/// title: "{station} departure timetable"
/// # or
/// title:
///   en: "{station} departure timetable"
///   ja: "{station} 発車時刻表"
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LocalizedText {
    /// The English form.
    pub en: String,
    /// The Japanese form, when one is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ja: Option<String>,
}

impl LocalizedText {
    /// Make a text with an English form only.
    pub fn en(text: impl Into<String>) -> Self {
        LocalizedText {
            en: text.into(),
            ja: None,
        }
    }

    /// Make a text with both forms.
    pub fn both(en: impl Into<String>, ja: impl Into<String>) -> Self {
        LocalizedText {
            en: en.into(),
            ja: Some(ja.into()),
        }
    }

    /// Get the form for a language, falling back to English.
    pub fn get(&self, language: Language) -> &str {
        match language {
            Language::Ja => self.ja.as_deref().unwrap_or(&self.en),
            Language::En => &self.en,
        }
    }

    /// Replace `{name}` placeholders in both forms.
    pub fn fill(&self, replacements: &[(&str, &str)]) -> LocalizedText {
        let apply = |text: &str| {
            let mut out = text.to_string();
            for (name, value) in replacements {
                out = out.replace(&format!("{{{name}}}"), value);
            }
            out
        };
        LocalizedText {
            en: apply(&self.en),
            ja: self.ja.as_deref().map(apply),
        }
    }
}

impl<'de> Deserialize<'de> for LocalizedText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Plain(String),
            Forms {
                en: String,
                #[serde(default)]
                ja: Option<String>,
            },
        }
        Ok(match Raw::deserialize(deserializer)? {
            Raw::Plain(text) => LocalizedText::en(text),
            Raw::Forms { en, ja } => LocalizedText { en, ja },
        })
    }
}

/// The user-interface labels of one language.
///
/// The struct holds every string that the renderers print outside of
/// the feed data, so a translation is one value away.
#[derive(Clone, Debug)]
pub struct Labels {
    /// The language of these labels.
    pub language: Language,
    /// "Departure timetable".
    pub departures: &'static str,
    /// "Service date".
    pub service_date: &'static str,
    /// "Platform".
    pub platform: &'static str,
    /// "Direction".
    pub direction: &'static str,
    /// "Hour".
    pub hour: &'static str,
    /// "Minutes".
    pub minutes: &'static str,
    /// "Legend".
    pub legend: &'static str,
    /// "Source".
    pub source: &'static str,
    /// "Feed fingerprint".
    pub feed_fingerprint: &'static str,
    /// "Warnings".
    pub warnings: &'static str,
    /// "Train diagram".
    pub diagram: &'static str,
    /// "Line".
    pub line: &'static str,
    /// "Destination".
    pub destination: &'static str,
    /// "Train".
    pub train: &'static str,
    /// "Arrival".
    pub arrival: &'static str,
    /// "Departure".
    pub departure: &'static str,
    /// "Train calls".
    pub calls: &'static str,
    /// "Selected train".
    pub selected_run: &'static str,
    /// "Exact".
    pub exact: &'static str,
    /// "Approximate".
    pub approximate: &'static str,
    /// "Runs".
    pub runs: &'static str,
    /// "Time".
    pub time: &'static str,
    /// "Stations".
    pub stations: &'static str,
    /// "Filters".
    pub filters: &'static str,
    /// "Reset view".
    pub reset: &'static str,
    /// "Print".
    pub print: &'static str,
    /// "Download SVG".
    pub download_svg: &'static str,
    /// "Zoom in".
    pub zoom_in: &'static str,
    /// "Zoom out".
    pub zoom_out: &'static str,
    /// "Show approximate service".
    pub show_approximate: &'static str,
    /// "Monochrome".
    pub monochrome: &'static str,
    /// "This page needs no network access."
    pub offline_note: &'static str,
    /// "No departures".
    pub no_departures: &'static str,
    /// "generated from a cached feed".
    pub stale_feed: &'static str,
    /// The legend entry for an approximate time.
    pub legend_approximate: &'static str,
    /// The legend entry for a computed time.
    pub legend_interpolated: &'static str,
    /// The legend entry for the first departure of the service day.
    pub legend_first: &'static str,
    /// The legend entry for the last departure of the service day.
    pub legend_last: &'static str,
    /// The legend entry for a departure after midnight.
    pub legend_past_midnight: &'static str,
    /// The legend entry for a platform label.
    pub legend_platform: &'static str,
    /// The legend entry for a train that passes without stopping.
    pub legend_pass_through: &'static str,
    /// The legend entry for the dwell segment of a diagram path.
    pub legend_dwell: &'static str,
    /// The legend entry for the date boundary line.
    pub legend_day_boundary: &'static str,
}

/// The English labels.
pub const EN: Labels = Labels {
    language: Language::En,
    departures: "Departure timetable",
    service_date: "Service date",
    platform: "Platform",
    direction: "Direction",
    hour: "Hour",
    minutes: "Minutes",
    legend: "Legend",
    source: "Source",
    feed_fingerprint: "Feed fingerprint",
    warnings: "Warnings",
    diagram: "Train diagram",
    line: "Line",
    destination: "Destination",
    train: "Train",
    arrival: "Arrival",
    departure: "Departure",
    calls: "Train calls",
    selected_run: "Selected train",
    exact: "Exact",
    approximate: "Approximate",
    runs: "Trains",
    time: "Time",
    stations: "Stations",
    filters: "Filters",
    reset: "Reset view",
    print: "Print",
    download_svg: "Download SVG",
    zoom_in: "Zoom in",
    zoom_out: "Zoom out",
    show_approximate: "Show approximate service",
    monochrome: "Monochrome",
    offline_note: "This page needs no network access.",
    no_departures: "No departures on this service day.",
    stale_feed: "generated from a cached feed",
    legend_approximate: "approximate time from a headway, not a scheduled departure",
    legend_interpolated: "time computed between two scheduled times",
    legend_first: "first departure of the service day",
    legend_last: "last departure of the service day",
    legend_past_midnight: "departs after midnight, on the next calendar day",
    legend_platform: "platform",
    legend_pass_through: "the train passes without serving the station",
    legend_dwell: "the train stands at the station",
    legend_day_boundary: "midnight of the service day",
};

/// The Japanese labels.
pub const JA: Labels = Labels {
    language: Language::Ja,
    departures: "発車時刻表",
    service_date: "運転日",
    platform: "のりば",
    direction: "方向",
    hour: "時",
    minutes: "分",
    legend: "凡例",
    source: "出典",
    feed_fingerprint: "データ指紋",
    warnings: "注意",
    diagram: "列車ダイヤグラム",
    line: "路線",
    destination: "行先",
    train: "列車",
    arrival: "着",
    departure: "発",
    calls: "停車時刻",
    selected_run: "選択中の列車",
    exact: "確定",
    approximate: "概算",
    runs: "列車",
    time: "時刻",
    stations: "駅",
    filters: "絞り込み",
    reset: "表示を戻す",
    print: "印刷",
    download_svg: "SVG をダウンロード",
    zoom_in: "拡大",
    zoom_out: "縮小",
    show_approximate: "概算の運転を表示",
    monochrome: "白黒",
    offline_note: "このページは通信を必要としません。",
    no_departures: "この運転日に発車はありません。",
    stale_feed: "保存済みデータから作成",
    legend_approximate: "運転間隔からの概算時刻。時刻表上の発車時刻ではありません",
    legend_interpolated: "前後の時刻から補間した時刻",
    legend_first: "始発",
    legend_last: "終発",
    legend_past_midnight: "翌日 0 時以降の発車",
    legend_platform: "のりば",
    legend_pass_through: "通過",
    legend_dwell: "停車中",
    legend_day_boundary: "運転日の 24 時",
};

impl Labels {
    /// Get the labels of a language.
    pub const fn for_language(language: Language) -> &'static Labels {
        match language {
            Language::En => &EN,
            Language::Ja => &JA,
        }
    }

    /// Build a "for <destination>" heading.
    pub fn towards(&self, destination: &str) -> String {
        match self.language {
            Language::En => format!("For {destination}"),
            Language::Ja => format!("{destination}方面"),
        }
    }

    /// Build a neutral direction heading for a GTFS `direction_id`.
    ///
    /// GTFS gives no meaning to `0` and `1`, so the label states the
    /// number and nothing more.
    pub fn direction_number(&self, direction: Option<u8>) -> String {
        match (self.language, direction) {
            (Language::En, Some(value)) => format!("Direction {value}"),
            (Language::En, None) => "Direction not stated".to_string(),
            (Language::Ja, Some(value)) => format!("方向 {value}"),
            (Language::Ja, None) => "方向の指定なし".to_string(),
        }
    }

    /// Build the text of a headway band, for example
    /// `06:30-09:00  every 4 min approximately`.
    pub fn headway_band(&self, start: &str, end: &str, minutes: u32) -> String {
        match self.language {
            Language::En => format!("{start}\u{2013}{end}  every {minutes} min approximately"),
            Language::Ja => format!("{start}\u{2013}{end}  約{minutes}分間隔"),
        }
    }

    /// Build the platform heading, for example `Platform 1`.
    pub fn platform_label(&self, code: &str) -> String {
        match self.language {
            Language::En => format!("Platform {code}"),
            Language::Ja => format!("{code}番のりば"),
        }
    }

    /// Format a service date for a heading, for example
    /// `2026-08-10 (Mon)`.
    pub fn service_date_text(&self, date: mrt_gtfs::ServiceDate) -> String {
        let weekday = match (self.language, date.weekday()) {
            (Language::En, mrt_gtfs::Weekday::Monday) => "Mon",
            (Language::En, mrt_gtfs::Weekday::Tuesday) => "Tue",
            (Language::En, mrt_gtfs::Weekday::Wednesday) => "Wed",
            (Language::En, mrt_gtfs::Weekday::Thursday) => "Thu",
            (Language::En, mrt_gtfs::Weekday::Friday) => "Fri",
            (Language::En, mrt_gtfs::Weekday::Saturday) => "Sat",
            (Language::En, mrt_gtfs::Weekday::Sunday) => "Sun",
            (Language::Ja, mrt_gtfs::Weekday::Monday) => "月",
            (Language::Ja, mrt_gtfs::Weekday::Tuesday) => "火",
            (Language::Ja, mrt_gtfs::Weekday::Wednesday) => "水",
            (Language::Ja, mrt_gtfs::Weekday::Thursday) => "木",
            (Language::Ja, mrt_gtfs::Weekday::Friday) => "金",
            (Language::Ja, mrt_gtfs::Weekday::Saturday) => "土",
            (Language::Ja, mrt_gtfs::Weekday::Sunday) => "日",
        };
        format!(
            "{:04}-{:02}-{:02} ({weekday})",
            date.year(),
            date.month(),
            date.day()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localized_text_falls_back_to_english() {
        let only_en = LocalizedText::en("Departures");
        assert_eq!(only_en.get(Language::Ja), "Departures");

        let both = LocalizedText::both("Departures", "発車時刻表");
        assert_eq!(both.get(Language::En), "Departures");
        assert_eq!(both.get(Language::Ja), "発車時刻表");
    }

    #[test]
    fn placeholders_fill_in_both_forms() {
        let template = LocalizedText::both("{station} departures", "{station} 発車時刻表");
        let filled = template.fill(&[("station", "Jurong East")]);
        assert_eq!(filled.en, "Jurong East departures");
        assert_eq!(filled.ja.as_deref(), Some("Jurong East 発車時刻表"));
    }

    #[test]
    fn a_direction_label_never_guesses_a_compass_bearing() {
        assert_eq!(EN.direction_number(Some(0)), "Direction 0");
        assert_eq!(EN.direction_number(None), "Direction not stated");
        assert_eq!(JA.direction_number(Some(1)), "方向 1");
    }

    #[test]
    fn a_headway_band_says_that_it_is_approximate() {
        let text = EN.headway_band("06:30", "09:00", 4);
        assert!(text.contains("approximately"), "{text}");
        assert!(JA.headway_band("06:30", "09:00", 4).contains("約4分間隔"));
    }

    #[test]
    fn languages_parse_from_configuration_strings() {
        assert_eq!("ja".parse::<Language>().unwrap(), Language::Ja);
        assert_eq!("English".parse::<Language>().unwrap(), Language::En);
        assert!("de".parse::<Language>().is_err());
    }

    #[test]
    fn service_dates_carry_a_weekday() {
        let date: mrt_gtfs::ServiceDate = "20260810".parse().unwrap();
        assert_eq!(EN.service_date_text(date), "2026-08-10 (Mon)");
        assert_eq!(JA.service_date_text(date), "2026-08-10 (月)");
    }
}
