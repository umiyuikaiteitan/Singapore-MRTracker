//! Typed models for the DataMall rail responses.
//!
//! The field names follow the LTA DataMall API User Guide. Timestamps
//! stay as strings in ISO 8601 format with the `+08:00` offset, for
//! example `2021-11-02T13:20:00+08:00`. This keeps the crate free of a
//! date-time dependency and easy to port.

use serde::{Deserialize, Serialize};

/// A Singapore train line, as the `TrainLine` parameter of the
/// platform crowd density APIs accepts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[allow(clippy::upper_case_acronyms)]
pub enum TrainLine {
    /// Circle Line.
    CCL,
    /// Circle Line Extension.
    CEL,
    /// Changi Extension.
    CGL,
    /// Downtown Line.
    DTL,
    /// East West Line.
    EWL,
    /// North East Line.
    NEL,
    /// North South Line.
    NSL,
    /// Bukit Panjang LRT.
    BPL,
    /// Sengkang LRT.
    SLRT,
    /// Punggol LRT.
    PLRT,
    /// Thomson-East Coast Line.
    TEL,
}

impl TrainLine {
    /// All train lines.
    pub const ALL: [TrainLine; 11] = [
        TrainLine::CCL,
        TrainLine::CEL,
        TrainLine::CGL,
        TrainLine::DTL,
        TrainLine::EWL,
        TrainLine::NEL,
        TrainLine::NSL,
        TrainLine::BPL,
        TrainLine::SLRT,
        TrainLine::PLRT,
        TrainLine::TEL,
    ];

    /// Get the API code of the line, for example `NSL`.
    pub const fn code(self) -> &'static str {
        match self {
            TrainLine::CCL => "CCL",
            TrainLine::CEL => "CEL",
            TrainLine::CGL => "CGL",
            TrainLine::DTL => "DTL",
            TrainLine::EWL => "EWL",
            TrainLine::NEL => "NEL",
            TrainLine::NSL => "NSL",
            TrainLine::BPL => "BPL",
            TrainLine::SLRT => "SLRT",
            TrainLine::PLRT => "PLRT",
            TrainLine::TEL => "TEL",
        }
    }

    /// Get the public name of the line.
    pub const fn full_name(self) -> &'static str {
        match self {
            TrainLine::CCL => "Circle Line",
            TrainLine::CEL => "Circle Line Extension",
            TrainLine::CGL => "Changi Extension",
            TrainLine::DTL => "Downtown Line",
            TrainLine::EWL => "East West Line",
            TrainLine::NEL => "North East Line",
            TrainLine::NSL => "North South Line",
            TrainLine::BPL => "Bukit Panjang LRT",
            TrainLine::SLRT => "Sengkang LRT",
            TrainLine::PLRT => "Punggol LRT",
            TrainLine::TEL => "Thomson-East Coast Line",
        }
    }
}

impl std::str::FromStr for TrainLine {
    type Err = String;

    /// Parse a line code. The parser ignores case.
    ///
    /// The parser also accepts the loop codes that the train service
    /// alerts use: `SEL` and `SWL` map to `SLRT`; `PEL` and `PWL` map
    /// to `PLRT`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_uppercase().as_str() {
            "CCL" => Ok(TrainLine::CCL),
            "CEL" => Ok(TrainLine::CEL),
            "CGL" | "CG" => Ok(TrainLine::CGL),
            "DTL" => Ok(TrainLine::DTL),
            "EWL" => Ok(TrainLine::EWL),
            "NEL" => Ok(TrainLine::NEL),
            "NSL" => Ok(TrainLine::NSL),
            "BPL" => Ok(TrainLine::BPL),
            "SLRT" | "SEL" | "SWL" => Ok(TrainLine::SLRT),
            "PLRT" | "PEL" | "PWL" => Ok(TrainLine::PLRT),
            "TEL" => Ok(TrainLine::TEL),
            other => Err(format!("\"{other}\" is not a known train line code")),
        }
    }
}

/// The OData envelope that wraps every DataMall response.
#[derive(Debug, Deserialize)]
pub(crate) struct Envelope<T> {
    #[serde(default)]
    pub value: T,
}

/// A link to a downloadable dataset file.
///
/// The GTFS endpoints and the passenger volume endpoint return this
/// shape. The link is pre-signed and expires after a short time
/// (15 minutes at the time of writing). Download the file directly
/// after you receive the link.
#[derive(Debug, Clone, Serialize)]
pub struct DatasetLink {
    /// The time when the server created the dataset, if the endpoint
    /// reports it.
    pub timestamp: Option<String>,
    /// The pre-signed download URL.
    pub url: String,
}

/// The raw link record inside a dataset-link response.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct RawLink {
    /// The GTFS endpoints use `link`. The passenger volume endpoint
    /// uses `Link`.
    #[serde(default, alias = "Link")]
    pub link: Option<String>,
    #[serde(default, alias = "Timestamp")]
    pub timestamp: Option<String>,
}

/// The overall rail service status in a train service alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ServiceStatus {
    /// All train services run as normal.
    Normal,
    /// One or more train services are disrupted.
    Disrupted,
    /// The API reported a value that this library does not know.
    Unknown(u8),
}

impl<'de> Deserialize<'de> for ServiceStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Ok(match value {
            1 => ServiceStatus::Normal,
            2 => ServiceStatus::Disrupted,
            other => ServiceStatus::Unknown(other),
        })
    }
}

/// A disrupted segment in a train service alert.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct AffectedSegment {
    /// The code of the affected line, for example `NEL`.
    pub line: String,
    /// The direction of the affected service, for example
    /// `HarbourFront` or `Both`.
    #[serde(default)]
    pub direction: String,
    /// The affected station codes as one delimited string, as the API
    /// supplies it. Use [`AffectedSegment::station_codes`] for a list.
    #[serde(default)]
    pub stations: String,
    /// The stations with free public bus service, as one delimited
    /// string.
    #[serde(default)]
    pub free_public_bus: String,
    /// The stations with free MRT shuttle service, as one delimited
    /// string.
    #[serde(default)]
    pub free_mrt_shuttle: String,
    /// The direction of the free MRT shuttle service.
    #[serde(default, alias = "MRTShuttleDirection")]
    pub mrt_shuttle_direction: String,
}

impl AffectedSegment {
    /// Get the affected line, if the code is known.
    pub fn train_line(&self) -> Option<TrainLine> {
        self.line.parse().ok()
    }

    /// Get the affected station codes as a list.
    pub fn station_codes(&self) -> Vec<String> {
        split_codes(&self.stations)
    }

    /// Get the station codes with free public bus service as a list.
    pub fn free_public_bus_codes(&self) -> Vec<String> {
        split_codes(&self.free_public_bus)
    }
}

/// Split a delimited station code string into single codes.
///
/// The API delimits with a comma or a hyphen, dependent on the
/// dataset version. This function accepts both.
fn split_codes(joined: &str) -> Vec<String> {
    joined
        .split([',', '-'])
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("free"))
        .map(str::to_string)
        .collect()
}

/// A public message in a train service alert.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct AlertMessage {
    /// The message text.
    pub content: String,
    /// The creation time of the message, for example
    /// `2018-01-21 17:17:11`.
    #[serde(default)]
    pub created_date: String,
}

/// The response of the legacy `TrainServiceAlerts` endpoint.
///
/// The GTFS-Realtime service alerts feed carries the same information
/// in the standard GTFS-Realtime format. This legacy endpoint is
/// simpler: it needs no Protocol Buffer decoder.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TrainServiceAlerts {
    /// The overall service status.
    #[serde(default = "default_status")]
    pub status: ServiceStatus,
    /// The disrupted segments. Empty when service is normal.
    #[serde(default)]
    pub affected_segments: Vec<AffectedSegment>,
    /// The public messages. Empty when service is normal.
    #[serde(default, rename = "Message")]
    pub messages: Vec<AlertMessage>,
}

fn default_status() -> ServiceStatus {
    ServiceStatus::Unknown(0)
}

impl Default for ServiceStatus {
    fn default() -> Self {
        ServiceStatus::Unknown(0)
    }
}

/// A crowd level on a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CrowdLevel {
    /// Low crowd.
    Low,
    /// Moderate crowd.
    Moderate,
    /// High crowd.
    High,
    /// The API reported no usable value.
    Unknown,
}

impl<'de> Deserialize<'de> for CrowdLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.trim().to_ascii_lowercase().as_str() {
            "l" => CrowdLevel::Low,
            "m" => CrowdLevel::Moderate,
            "h" => CrowdLevel::High,
            _ => CrowdLevel::Unknown,
        })
    }
}

/// One record of the `PCDRealTime` endpoint: the current crowd level
/// on the platforms of one station.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlatformCrowd {
    /// The station code, for example `BP11`.
    pub station: String,
    /// The start of the measurement interval, in ISO 8601 format.
    #[serde(default)]
    pub start_time: String,
    /// The end of the measurement interval, in ISO 8601 format.
    #[serde(default)]
    pub end_time: String,
    /// The measured crowd level.
    pub crowd_level: CrowdLevel,
}

/// One record of the `PCDForecast` endpoint: a full day of forecast
/// intervals for the stations of one line.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct CrowdForecastDay {
    /// The forecast date, in ISO 8601 format.
    #[serde(default, alias = "Date")]
    pub date: String,
    /// The forecasts per station.
    #[serde(default)]
    pub stations: Vec<StationCrowdForecast>,
}

/// The forecast intervals for one station.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct StationCrowdForecast {
    /// The station code, for example `CC1`.
    pub station: String,
    /// The 30-minute forecast intervals.
    #[serde(default)]
    pub interval: Vec<CrowdInterval>,
}

/// One 30-minute crowd forecast interval.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct CrowdInterval {
    /// The start of the interval, in ISO 8601 format.
    #[serde(default)]
    pub start: String,
    /// The forecast crowd level.
    pub crowd_level: CrowdLevel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn train_line_codes_round_trip() {
        for line in TrainLine::ALL {
            let parsed: TrainLine = line.code().parse().unwrap();
            assert_eq!(parsed, line);
        }
    }

    #[test]
    fn train_line_parser_accepts_alert_loop_codes() {
        assert_eq!("SEL".parse::<TrainLine>().unwrap(), TrainLine::SLRT);
        assert_eq!("swl".parse::<TrainLine>().unwrap(), TrainLine::SLRT);
        assert_eq!("PEL".parse::<TrainLine>().unwrap(), TrainLine::PLRT);
        assert_eq!("PWL".parse::<TrainLine>().unwrap(), TrainLine::PLRT);
        assert!("XYZ".parse::<TrainLine>().is_err());
    }

    #[test]
    fn station_code_strings_split_on_comma_and_hyphen() {
        assert_eq!(split_codes("NE1,NE3,NE4"), vec!["NE1", "NE3", "NE4"]);
        assert_eq!(split_codes("NE1-NE3-NE4"), vec!["NE1", "NE3", "NE4"]);
        assert_eq!(split_codes(""), Vec::<String>::new());
    }
}
