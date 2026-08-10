//! Schedule queries: departures and destination boards.

use serde::Serialize;

use crate::date::ServiceDate;
use crate::network::{LineId, RailNetwork, StationId, TripSchedule};
use crate::time::{GtfsTime, SECS_PER_DAY};

/// One scheduled departure from a station.
#[derive(Debug, Clone, Serialize)]
pub struct Departure {
    /// The line of the departure.
    pub line: LineId,
    /// The GTFS trip identifier.
    pub trip_id: String,
    /// The station of the departure.
    pub station: StationId,
    /// The departure time on the clock of the service day. The value
    /// can be after `24:00:00` for a trip that started on the day
    /// before.
    pub time: GtfsTime,
    /// The destination text of the trip, if the feed supplies one.
    pub headsign: Option<String>,
    /// The last station of the trip.
    pub terminus: StationId,
    /// The GTFS direction of the trip.
    pub direction: Option<u8>,
    /// `true` if the time is exact. `false` if the time comes from a
    /// headway-based frequency entry and is approximate.
    pub exact: bool,
}

/// One row of a destination board.
///
/// A board row wraps a [`Departure`] together with its service date and
/// the wait time from the query time.
#[derive(Debug, Clone, Serialize)]
pub struct BoardEntry {
    /// The departure.
    pub departure: Departure,
    /// The service date of the trip. For a trip that runs past
    /// midnight, this is the date on which the trip started.
    pub service_date: ServiceDate,
    /// The wait time from the query time to the departure, in seconds.
    pub wait_secs: u32,
}

impl BoardEntry {
    /// Get the departure time on a 24-hour clock.
    pub fn clock_time(&self) -> GtfsTime {
        GtfsTime::from_seconds(self.departure.time.clock_seconds())
    }
}

impl RailNetwork {
    /// Get the departures from a station on one service day.
    ///
    /// The function returns every departure with `from <= time <= until`,
    /// in time order. It expands frequency-based trips into single
    /// departures. It does not return the final arrival of a trip at
    /// its terminus, because passengers cannot board there.
    ///
    /// A time after `24:00:00` selects trips that continue past
    /// midnight into the next calendar day.
    pub fn departures(
        &self,
        station: StationId,
        date: ServiceDate,
        from: GtfsTime,
        until: GtfsTime,
    ) -> Vec<Departure> {
        let mut result = Vec::new();
        for trip in &self.trips {
            if !self.services.active(trip.service, date) {
                continue;
            }
            self.collect_trip_departures(trip, station, from, until, &mut result);
        }
        result.sort_by(|a, b| {
            (a.time, a.line, a.trip_id.as_str()).cmp(&(b.time, b.line, b.trip_id.as_str()))
        });
        result
    }

    /// Collect the departures of one trip at one station.
    fn collect_trip_departures(
        &self,
        trip: &TripSchedule,
        station: StationId,
        from: GtfsTime,
        until: GtfsTime,
        out: &mut Vec<Departure>,
    ) {
        let pattern = self.pattern(trip.pattern);
        let terminus = match pattern.stations.last() {
            Some(&terminus) => terminus,
            None => return,
        };
        for (idx, &at) in pattern.stations.iter().enumerate() {
            // Passengers cannot board at the terminus.
            if at != station || idx + 1 == pattern.stations.len() {
                continue;
            }
            if trip.frequencies.is_empty() {
                let Some(time) = trip.calls[idx].departure_or_arrival() else {
                    continue;
                };
                if from <= time && time <= until {
                    out.push(self.make_departure(trip, station, terminus, time, true));
                }
            } else {
                self.collect_frequency_departures(trip, station, terminus, idx, from, until, out);
            }
        }
    }

    /// Expand the frequency blocks of a trip into single departures.
    #[allow(clippy::too_many_arguments)]
    fn collect_frequency_departures(
        &self,
        trip: &TripSchedule,
        station: StationId,
        terminus: StationId,
        stop_idx: usize,
        from: GtfsTime,
        until: GtfsTime,
        out: &mut Vec<Departure>,
    ) {
        // The template times give the offset from the trip start to
        // the call at this stop.
        let Some(first) = trip.calls.first().and_then(|c| c.departure_or_arrival()) else {
            return;
        };
        let Some(at_stop) = trip.calls[stop_idx].departure_or_arrival() else {
            return;
        };
        let Some(offset) = at_stop.seconds().checked_sub(first.seconds()) else {
            return;
        };
        for block in &trip.frequencies {
            if block.headway_secs == 0 {
                continue;
            }
            // Trips start every `headway_secs` from `start_time`.
            // No trip starts at or after `end_time`.
            let mut start = block.start_time.seconds();
            while start < block.end_time.seconds() {
                let time = GtfsTime::from_seconds(start + offset);
                if from <= time && time <= until {
                    out.push(self.make_departure(trip, station, terminus, time, block.is_exact()));
                }
                start += block.headway_secs;
            }
        }
    }

    fn make_departure(
        &self,
        trip: &TripSchedule,
        station: StationId,
        terminus: StationId,
        time: GtfsTime,
        exact: bool,
    ) -> Departure {
        Departure {
            line: trip.line,
            trip_id: trip.trip_id.clone(),
            station,
            time,
            headsign: trip.headsign.clone(),
            terminus,
            direction: self.pattern(trip.pattern).direction,
            exact,
        }
    }

    /// Get a destination board for a station.
    ///
    /// The board lists the departures in the window from `clock` to
    /// `clock + lookahead_secs`, in wait-time order. The `clock` value
    /// is a time on the 24-hour clock of the given date.
    ///
    /// The function also examines the service day before `date`,
    /// because a trip that started before midnight can depart after
    /// midnight.
    pub fn departure_board(
        &self,
        station: StationId,
        date: ServiceDate,
        clock: GtfsTime,
        lookahead_secs: u32,
    ) -> Vec<BoardEntry> {
        let mut entries: Vec<BoardEntry> = Vec::new();

        // Departures on the service day of `date`.
        let until = clock.plus_seconds(lookahead_secs);
        for departure in self.departures(station, date, clock, until) {
            entries.push(BoardEntry {
                wait_secs: departure.time.seconds() - clock.seconds(),
                service_date: date,
                departure,
            });
        }

        // Departures of trips that started on the day before and run
        // past midnight. On that service day, the query window starts
        // 24 hours later.
        let previous = date.previous_day();
        let from = clock.plus_seconds(SECS_PER_DAY);
        let until = from.plus_seconds(lookahead_secs);
        for departure in self.departures(station, previous, from, until) {
            entries.push(BoardEntry {
                wait_secs: departure.time.seconds() - from.seconds(),
                service_date: previous,
                departure,
            });
        }

        entries.sort_by_key(|e| e.wait_secs);
        entries
    }
}
