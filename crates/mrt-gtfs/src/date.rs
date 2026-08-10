//! Calendar dates for GTFS service periods.
//!
//! This module has no external dependencies. It uses the civil-calendar
//! algorithms from Howard Hinnant's `chrono`-compatible date paper, so a
//! port to another language stays simple.

use std::fmt;
use std::str::FromStr;

use crate::error::GtfsError;

/// A day of the week.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    /// Get the index of the day, where Monday is 0 and Sunday is 6.
    pub const fn index(self) -> usize {
        match self {
            Weekday::Monday => 0,
            Weekday::Tuesday => 1,
            Weekday::Wednesday => 2,
            Weekday::Thursday => 3,
            Weekday::Friday => 4,
            Weekday::Saturday => 5,
            Weekday::Sunday => 6,
        }
    }

    const ALL: [Weekday; 7] = [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ];
}

/// A calendar date in the GTFS `YYYYMMDD` format.
///
/// # Examples
///
/// ```
/// use mrt_gtfs::{ServiceDate, Weekday};
///
/// let date: ServiceDate = "20250501".parse().unwrap();
/// assert_eq!(date.weekday(), Weekday::Thursday);
/// assert_eq!(date.to_string(), "20250501");
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceDate {
    year: i32,
    month: u8,
    day: u8,
}

impl ServiceDate {
    /// Make a date from a year, a month (1 to 12), and a day (1 to 31).
    ///
    /// The function returns an error if the values do not make a valid
    /// calendar date.
    pub fn new(year: i32, month: u8, day: u8) -> Result<Self, GtfsError> {
        if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return Err(GtfsError::InvalidValue(format!(
                "{year:04}-{month:02}-{day:02} is not a valid date"
            )));
        }
        Ok(ServiceDate { year, month, day })
    }

    /// Get the year.
    pub const fn year(self) -> i32 {
        self.year
    }

    /// Get the month, from 1 to 12.
    pub const fn month(self) -> u8 {
        self.month
    }

    /// Get the day of the month, from 1 to 31.
    pub const fn day(self) -> u8 {
        self.day
    }

    /// Get the day of the week.
    pub fn weekday(self) -> Weekday {
        // 1970-01-01 is a Thursday, which has index 3.
        let days = days_from_civil(self.year, self.month, self.day);
        let index = (days + 3).rem_euclid(7) as usize;
        Weekday::ALL[index]
    }

    /// Get the date that is `days` days after this date.
    ///
    /// A negative `days` value gives a date in the past.
    pub fn plus_days(self, days: i64) -> Self {
        let total = days_from_civil(self.year, self.month, self.day) + days;
        let (year, month, day) = civil_from_days(total);
        ServiceDate { year, month, day }
    }

    /// Get the date one day before this date.
    pub fn previous_day(self) -> Self {
        self.plus_days(-1)
    }
}

impl fmt::Display for ServiceDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}{:02}{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for ServiceDate {
    type Err = GtfsError;

    /// Parse a date string in the GTFS `YYYYMMDD` format.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || GtfsError::InvalidValue(format!("\"{s}\" is not a valid GTFS date"));
        let s = s.trim();
        if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        let year: i32 = s[0..4].parse().map_err(|_| bad())?;
        let month: u8 = s[4..6].parse().map_err(|_| bad())?;
        let day: u8 = s[6..8].parse().map_err(|_| bad())?;
        ServiceDate::new(year, month, day)
    }
}

impl<'de> serde::Deserialize<'de> for ServiceDate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for ServiceDate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Report whether the year is a leap year.
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Get the number of days in the month.
fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Count the days from 1970-01-01 to the given civil date.
///
/// This is the `days_from_civil` algorithm by Howard Hinnant.
fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let m = i64::from(month);
    let d = i64::from(day);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Get the civil date for a count of days from 1970-01-01.
///
/// This is the `civil_from_days` algorithm by Howard Hinnant.
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    ((y + i64::from(m <= 2)) as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        let date: ServiceDate = "20260810".parse().unwrap();
        assert_eq!(date.year(), 2026);
        assert_eq!(date.month(), 8);
        assert_eq!(date.day(), 10);
        assert_eq!(date.to_string(), "20260810");
    }

    #[test]
    fn reject_invalid_dates() {
        assert!("2025051".parse::<ServiceDate>().is_err());
        assert!("20250532".parse::<ServiceDate>().is_err());
        assert!("20251301".parse::<ServiceDate>().is_err());
        assert!("20250229".parse::<ServiceDate>().is_err());
        assert!("abcdefgh".parse::<ServiceDate>().is_err());
    }

    #[test]
    fn weekday_anchors() {
        // Well-known anchor dates.
        assert_eq!(date(1970, 1, 1).weekday(), Weekday::Thursday);
        assert_eq!(date(2000, 1, 1).weekday(), Weekday::Saturday);
        assert_eq!(date(2024, 2, 29).weekday(), Weekday::Thursday);
        assert_eq!(date(2026, 8, 10).weekday(), Weekday::Monday);
    }

    #[test]
    fn leap_year_rules() {
        assert!(is_leap_year(2024));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2025));
        assert!("20240229".parse::<ServiceDate>().is_ok());
    }

    #[test]
    fn plus_days_crosses_boundaries() {
        assert_eq!(date(2025, 1, 1).previous_day(), date(2024, 12, 31));
        assert_eq!(date(2024, 2, 28).plus_days(1), date(2024, 2, 29));
        assert_eq!(date(2024, 2, 29).plus_days(1), date(2024, 3, 1));
        assert_eq!(date(2025, 12, 31).plus_days(1), date(2026, 1, 1));
        assert_eq!(date(2025, 3, 1).plus_days(-1), date(2025, 2, 28));
    }

    #[test]
    fn ordering_is_chronological() {
        assert!(date(2025, 1, 31) < date(2025, 2, 1));
        assert!(date(2025, 12, 31) < date(2026, 1, 1));
    }

    fn date(year: i32, month: u8, day: u8) -> ServiceDate {
        ServiceDate::new(year, month, day).unwrap()
    }
}
