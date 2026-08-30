//! Singapore local time.
//!
//! The view models of this crate read no clock: a caller passes the
//! service date, the clock reading, and the realtime `now_unix` in.
//! These three functions are what a caller needs to produce them, and
//! they live here so that every caller converts POSIX time the same
//! way. Singapore keeps one offset all year and observes no daylight
//! saving, so the conversion is arithmetic and needs no time zone
//! database.
//!
//! The map server (`mrt-map-web`) and the static map generator
//! (`mrt-map-static`) call these. The same arithmetic is still written
//! out in three other places, and they are deliberately left alone:
//! `mrt-board-web`'s own `clock` module, which the board's untouched
//! deployment depends on, and the date helpers in the binaries
//! `mrt-board-static` (`sgt_date`) and `mrt-schedule-site`
//! (`build::today_at_offset`).

use mrt_gtfs::{GtfsTime, ServiceDate};

/// The offset of Singapore Standard Time from UTC, in seconds.
/// Singapore has no daylight saving time.
const SGT_OFFSET_SECS: i64 = 8 * 3600;

/// The number of seconds in one calendar day.
const SECS_PER_DAY: i64 = 86_400;

/// Convert a POSIX timestamp to a Singapore date and clock time.
///
/// The clock is a civil time on that date, from `00:00:00` to
/// `23:59:59`. It is not a GTFS service-day time: a run that started
/// before midnight belongs to the day before, and the map builder
/// reaches that day itself.
///
/// # Example
///
/// ```
/// let (date, clock) = mrt_live::clock::sgt_from_unix(0);
/// assert_eq!(date.to_string(), "19700101");
/// assert_eq!(clock.to_string(), "08:00:00");
/// ```
pub fn sgt_from_unix(unix_secs: i64) -> (ServiceDate, GtfsTime) {
    let local = unix_secs + SGT_OFFSET_SECS;
    let epoch: ServiceDate = "19700101".parse().expect("valid epoch date");
    let date = epoch.plus_days(local.div_euclid(SECS_PER_DAY));
    let clock = GtfsTime::from_seconds(local.rem_euclid(SECS_PER_DAY) as u32);
    (date, clock)
}

/// Get the current POSIX time, in seconds.
///
/// This is the one clock reading in the map stack, and it belongs to
/// the caller: no view model of this crate calls it.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs() as i64
}

/// Get the current Singapore date and clock time.
pub fn sgt_now() -> (ServiceDate, GtfsTime) {
    sgt_from_unix(unix_now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_is_eight_in_the_morning() {
        let (date, clock) = sgt_from_unix(0);
        assert_eq!(date.to_string(), "19700101");
        assert_eq!(clock.to_string(), "08:00:00");
    }

    #[test]
    fn a_known_timestamp_converts() {
        // 2026-08-11 04:01:48 SGT is 2026-08-10 20:01:48 UTC.
        let (date, clock) = sgt_from_unix(1_786_392_108);
        assert_eq!(date.to_string(), "20260811");
        assert_eq!(clock.to_string(), "04:01:48");
    }

    #[test]
    fn the_day_flips_at_sgt_midnight() {
        // 15:59:59 UTC is 23:59:59 SGT on the same day.
        let (date, clock) = sgt_from_unix(1_786_377_599);
        assert_eq!(date.to_string(), "20260810");
        assert_eq!(clock.to_string(), "23:59:59");
        // One second later the SGT date advances.
        let (date, clock) = sgt_from_unix(1_786_377_600);
        assert_eq!(date.to_string(), "20260811");
        assert_eq!(clock.to_string(), "00:00:00");
    }
}
