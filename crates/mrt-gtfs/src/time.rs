//! Time of day on a GTFS service day.

use std::fmt;
use std::str::FromStr;

use crate::error::GtfsError;

/// The number of seconds in one hour.
const SECS_PER_HOUR: u32 = 3600;
/// The number of seconds in one minute.
const SECS_PER_MIN: u32 = 60;
/// The number of seconds in one day.
pub(crate) const SECS_PER_DAY: u32 = 24 * SECS_PER_HOUR;

/// A time of day on a GTFS service day.
///
/// The value counts seconds after midnight of the service day.
/// GTFS permits times after `24:00:00` for trips that continue past
/// midnight. Such a trip belongs to the service day on which it starts.
///
/// # Examples
///
/// ```
/// use mrt_gtfs::GtfsTime;
///
/// let t: GtfsTime = "25:15:00".parse().unwrap();
/// assert_eq!(t.hours(), 25);
/// assert_eq!(t.clock_seconds(), 1 * 3600 + 15 * 60);
/// assert_eq!(t.to_string(), "25:15:00");
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GtfsTime(u32);

impl GtfsTime {
    /// Make a time from a count of seconds after midnight.
    pub const fn from_seconds(seconds: u32) -> Self {
        GtfsTime(seconds)
    }

    /// Make a time from hours, minutes, and seconds.
    ///
    /// The `hours` value can be 24 or more.
    pub const fn from_hms(hours: u32, minutes: u32, seconds: u32) -> Self {
        GtfsTime(hours * SECS_PER_HOUR + minutes * SECS_PER_MIN + seconds)
    }

    /// Get the count of seconds after midnight of the service day.
    pub const fn seconds(self) -> u32 {
        self.0
    }

    /// Get the hour part. The value can be 24 or more.
    pub const fn hours(self) -> u32 {
        self.0 / SECS_PER_HOUR
    }

    /// Get the minute part, from 0 to 59.
    pub const fn minutes(self) -> u32 {
        (self.0 % SECS_PER_HOUR) / SECS_PER_MIN
    }

    /// Get the second part, from 0 to 59.
    pub const fn seconds_part(self) -> u32 {
        self.0 % SECS_PER_MIN
    }

    /// Get the equivalent count of seconds on a 24-hour clock.
    ///
    /// For `25:15:00` the result is the count of seconds at `01:15:00`.
    pub const fn clock_seconds(self) -> u32 {
        self.0 % SECS_PER_DAY
    }

    /// Add a count of seconds and return the new time.
    pub const fn plus_seconds(self, seconds: u32) -> Self {
        GtfsTime(self.0 + seconds)
    }
}

impl fmt::Display for GtfsTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02}:{:02}:{:02}",
            self.hours(),
            self.minutes(),
            self.seconds_part()
        )
    }
}

impl FromStr for GtfsTime {
    type Err = GtfsError;

    /// Parse a GTFS time string.
    ///
    /// The accepted formats are `HH:MM:SS` and `H:MM:SS`.
    /// The hour value can be 24 or more.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || GtfsError::InvalidValue(format!("\"{s}\" is not a valid GTFS time"));
        let mut parts = s.split(':');
        let hours: u32 = parts
            .next()
            .and_then(|p| p.trim().parse().ok())
            .ok_or_else(bad)?;
        let minutes: u32 = parts
            .next()
            .and_then(|p| p.trim().parse().ok())
            .ok_or_else(bad)?;
        let seconds: u32 = parts
            .next()
            .and_then(|p| p.trim().parse().ok())
            .ok_or_else(bad)?;
        if parts.next().is_some() || minutes > 59 || seconds > 59 || hours > 99 {
            return Err(bad());
        }
        Ok(GtfsTime::from_hms(hours, minutes, seconds))
    }
}

impl<'de> serde::Deserialize<'de> for GtfsTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for GtfsTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_standard_time() {
        let t: GtfsTime = "06:30:15".parse().unwrap();
        assert_eq!(t, GtfsTime::from_hms(6, 30, 15));
    }

    #[test]
    fn parse_single_digit_hour() {
        let t: GtfsTime = "6:30:00".parse().unwrap();
        assert_eq!(t, GtfsTime::from_hms(6, 30, 0));
    }

    #[test]
    fn parse_time_after_midnight() {
        let t: GtfsTime = "25:01:02".parse().unwrap();
        assert_eq!(t.hours(), 25);
        assert_eq!(t.clock_seconds(), GtfsTime::from_hms(1, 1, 2).seconds());
    }

    #[test]
    fn reject_invalid_times() {
        assert!("06:60:00".parse::<GtfsTime>().is_err());
        assert!("06:00:60".parse::<GtfsTime>().is_err());
        assert!("06:00".parse::<GtfsTime>().is_err());
        assert!("".parse::<GtfsTime>().is_err());
        assert!("abc".parse::<GtfsTime>().is_err());
        assert!("06:00:00:00".parse::<GtfsTime>().is_err());
    }

    #[test]
    fn display_round_trip() {
        for s in ["00:00:00", "09:05:03", "23:59:59", "26:10:00"] {
            let t: GtfsTime = s.parse().unwrap();
            assert_eq!(t.to_string(), s);
        }
    }

    #[test]
    fn ordering_follows_seconds() {
        let early = GtfsTime::from_hms(23, 50, 0);
        let late = GtfsTime::from_hms(24, 5, 0);
        assert!(early < late);
    }
}
