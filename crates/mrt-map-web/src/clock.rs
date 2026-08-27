//! Singapore local time.
//!
//! The same three functions the board server keeps in its own `clock`
//! module. They live in this library so that the map server and the
//! static map generator convert POSIX time the same way; the snapshot
//! builder itself reads no clock and takes their results as arguments.

use mrt_gtfs::{GtfsTime, ServiceDate};

/// The offset of Singapore Standard Time from UTC, in seconds.
/// Singapore has no daylight saving time.
const SGT_OFFSET_SECS: i64 = 8 * 3600;

const SECS_PER_DAY: i64 = 86_400;

/// Convert a POSIX timestamp to a Singapore date and clock time.
pub fn sgt_from_unix(unix_secs: i64) -> (ServiceDate, GtfsTime) {
    let local = unix_secs + SGT_OFFSET_SECS;
    let epoch: ServiceDate = "19700101".parse().expect("valid epoch date");
    let date = epoch.plus_days(local.div_euclid(SECS_PER_DAY));
    let clock = GtfsTime::from_seconds(local.rem_euclid(SECS_PER_DAY) as u32);
    (date, clock)
}

/// Get the current POSIX time, in seconds.
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
