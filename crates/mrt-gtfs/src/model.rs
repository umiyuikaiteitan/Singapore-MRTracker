//! Raw GTFS records.
//!
//! Each structure in this module maps one row of a GTFS feed file.
//! The structures keep the data as the feed supplies it. The
//! [`crate::network::RailNetwork`] type builds a linked model from
//! these records.

use serde::Deserialize;

use crate::date::ServiceDate;
use crate::time::GtfsTime;

/// One row of `agency.txt`. A transit operator.
#[derive(Debug, Clone, Deserialize)]
pub struct Agency {
    /// The identifier of the operator. Optional in single-operator feeds.
    #[serde(default)]
    pub agency_id: Option<String>,
    /// The full name of the operator.
    pub agency_name: String,
    /// The URL of the operator.
    #[serde(default)]
    pub agency_url: Option<String>,
    /// The time zone of the operator, for example `Asia/Singapore`.
    #[serde(default)]
    pub agency_timezone: Option<String>,
}

/// One row of `stops.txt`. A stop, a platform, or a station.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Stop {
    /// The identifier of the stop.
    pub stop_id: String,
    /// The public code of the stop, for example `NS1`.
    #[serde(default)]
    pub stop_code: Option<String>,
    /// The public name of the stop.
    #[serde(default)]
    pub stop_name: Option<String>,
    /// The latitude of the stop, in WGS 84 degrees.
    #[serde(default)]
    pub stop_lat: Option<f64>,
    /// The longitude of the stop, in WGS 84 degrees.
    #[serde(default)]
    pub stop_lon: Option<f64>,
    /// The type of the location. `0` or empty is a stop or a platform.
    /// `1` is a station. `2` is an entrance or an exit.
    #[serde(default)]
    pub location_type: Option<u8>,
    /// The identifier of the parent station, if the stop is in a station.
    #[serde(default)]
    pub parent_station: Option<String>,
    /// The platform identifier, for example `A` or `1`.
    #[serde(default)]
    pub platform_code: Option<String>,
}

impl Stop {
    /// Report whether this record describes a station.
    pub fn is_station(&self) -> bool {
        self.location_type == Some(1)
    }

    /// Report whether this record describes a boarding location.
    ///
    /// A boarding location is a stop or a platform where passengers
    /// board a vehicle.
    pub fn is_boarding_location(&self) -> bool {
        matches!(self.location_type, None | Some(0))
    }

    /// Get the parent station identifier, if the field has content.
    pub fn parent_station_id(&self) -> Option<&str> {
        self.parent_station.as_deref().filter(|s| !s.is_empty())
    }
}

/// One row of `routes.txt`. A transit line.
#[derive(Debug, Clone, Deserialize)]
pub struct Route {
    /// The identifier of the route.
    pub route_id: String,
    /// The identifier of the operator of the route.
    #[serde(default)]
    pub agency_id: Option<String>,
    /// The short public name, for example `NS` or `NSL`.
    #[serde(default)]
    pub route_short_name: Option<String>,
    /// The long public name, for example `North South Line`.
    #[serde(default)]
    pub route_long_name: Option<String>,
    /// The type of the route. See [`crate::filter::RailFilter`] for the
    /// rail values.
    pub route_type: u16,
    /// The line color as a six-digit hexadecimal value, for example
    /// `D42E12`.
    #[serde(default)]
    pub route_color: Option<String>,
    /// The text color as a six-digit hexadecimal value.
    #[serde(default)]
    pub route_text_color: Option<String>,
}

/// One row of `trips.txt`. One run of a vehicle along a route.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Trip {
    /// The identifier of the route of the trip.
    pub route_id: String,
    /// The identifier of the service period of the trip.
    pub service_id: String,
    /// The identifier of the trip.
    pub trip_id: String,
    /// The destination text that the vehicle shows, for example
    /// `Marina Bay`.
    #[serde(default)]
    pub trip_headsign: Option<String>,
    /// The public name of the trip, for example a train number.
    ///
    /// This is the only trip identifier that a timetable may show to
    /// passengers. `trip_id` is an internal key and must stay out of
    /// passenger-facing output.
    #[serde(default)]
    pub trip_short_name: Option<String>,
    /// The direction of travel. `0` and `1` are opposite directions.
    #[serde(default)]
    pub direction_id: Option<u8>,
    /// The identifier of the block that this trip belongs to.
    ///
    /// Consecutive trips of one vehicle share a block. A later
    /// release can link them into one continuous diagram run.
    #[serde(default)]
    pub block_id: Option<String>,
    /// The identifier of the shape of the trip in `shapes.txt`.
    #[serde(default)]
    pub shape_id: Option<String>,
}

/// One row of `stop_times.txt`. One scheduled call of a trip at a stop.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StopTime {
    /// The identifier of the trip.
    pub trip_id: String,
    /// The arrival time at the stop. Empty for some intermediate stops.
    #[serde(default)]
    pub arrival_time: Option<GtfsTime>,
    /// The departure time from the stop. Empty for some intermediate
    /// stops.
    #[serde(default)]
    pub departure_time: Option<GtfsTime>,
    /// The identifier of the stop.
    pub stop_id: String,
    /// The order of the stop in the trip. Values increase along the trip.
    pub stop_sequence: u32,
    /// The destination text for the remainder of the trip, if it is
    /// different from the trip headsign.
    #[serde(default)]
    pub stop_headsign: Option<String>,
    /// The pickup rule. `1` means no pickup at this stop.
    #[serde(default)]
    pub pickup_type: Option<u8>,
    /// The drop-off rule. `1` means no drop-off at this stop.
    #[serde(default)]
    pub drop_off_type: Option<u8>,
    /// The time precision of this call. `1` or empty marks an exact
    /// time. `0` marks an approximate time that the publisher
    /// interpolated.
    #[serde(default)]
    pub timepoint: Option<u8>,
    /// The distance travelled along the shape of the trip, in the
    /// unit of `shapes.txt`.
    ///
    /// A diagram uses the value to place a call on a distance axis and
    /// to weight the interpolation of a missing time.
    #[serde(default)]
    pub shape_dist_traveled: Option<f64>,
}

impl StopTime {
    /// Report whether passengers may board at this call.
    ///
    /// GTFS `pickup_type` `1` means that no pickup is available. Every
    /// other value, including an empty one, permits boarding.
    pub fn allows_pickup(&self) -> bool {
        self.pickup_type != Some(1)
    }

    /// Report whether the feed marks the times of this call as exact.
    ///
    /// GTFS `timepoint` `0` marks times that the publisher
    /// interpolated. An empty value means an exact time.
    pub fn is_exact_timepoint(&self) -> bool {
        self.timepoint != Some(0)
    }
}

/// One row of `calendar.txt`. A weekly service pattern.
#[derive(Debug, Clone, Deserialize)]
pub struct Calendar {
    /// The identifier of the service period.
    pub service_id: String,
    /// `1` if the service operates on Monday.
    pub monday: u8,
    /// `1` if the service operates on Tuesday.
    pub tuesday: u8,
    /// `1` if the service operates on Wednesday.
    pub wednesday: u8,
    /// `1` if the service operates on Thursday.
    pub thursday: u8,
    /// `1` if the service operates on Friday.
    pub friday: u8,
    /// `1` if the service operates on Saturday.
    pub saturday: u8,
    /// `1` if the service operates on Sunday.
    pub sunday: u8,
    /// The first day of the service period.
    pub start_date: ServiceDate,
    /// The last day of the service period.
    pub end_date: ServiceDate,
}

impl Calendar {
    /// Get the weekday flags as an array. Index 0 is Monday.
    pub fn weekday_flags(&self) -> [bool; 7] {
        [
            self.monday != 0,
            self.tuesday != 0,
            self.wednesday != 0,
            self.thursday != 0,
            self.friday != 0,
            self.saturday != 0,
            self.sunday != 0,
        ]
    }
}

/// The exception type in `calendar_dates.txt`.
pub const EXCEPTION_SERVICE_ADDED: u8 = 1;
/// The exception type in `calendar_dates.txt`.
pub const EXCEPTION_SERVICE_REMOVED: u8 = 2;

/// One row of `calendar_dates.txt`. A service exception for one date.
#[derive(Debug, Clone, Deserialize)]
pub struct CalendarDate {
    /// The identifier of the service period.
    pub service_id: String,
    /// The date of the exception.
    pub date: ServiceDate,
    /// The type of the exception. `1` adds service. `2` removes service.
    pub exception_type: u8,
}

/// One row of `frequencies.txt`. A headway-based service block.
#[derive(Debug, Clone, Deserialize)]
pub struct Frequency {
    /// The identifier of the template trip.
    pub trip_id: String,
    /// The start of the block. The first trip starts at this time.
    pub start_time: GtfsTime,
    /// The end of the block. No trip starts at or after this time.
    pub end_time: GtfsTime,
    /// The time between trip starts, in seconds.
    pub headway_secs: u32,
    /// `1` if the block repeats the exact template schedule.
    /// `0` or empty if the times are approximate.
    #[serde(default)]
    pub exact_times: Option<u8>,
}

impl Frequency {
    /// Report whether the departure times from this block are exact.
    pub fn is_exact(&self) -> bool {
        self.exact_times == Some(1)
    }
}

/// One row of `transfers.txt`. A transfer rule between two stops.
#[derive(Debug, Clone, Deserialize)]
pub struct Transfer {
    /// The identifier of the origin stop.
    pub from_stop_id: String,
    /// The identifier of the destination stop.
    pub to_stop_id: String,
    /// The type of the transfer. `0` is a recommended transfer point.
    /// `2` requires a minimum time. `3` means no transfer is possible.
    #[serde(default)]
    pub transfer_type: Option<u8>,
    /// The minimum transfer time, in seconds.
    #[serde(default)]
    pub min_transfer_time: Option<u32>,
}

/// One row of `shapes.txt`. One point of a trip path polyline.
#[derive(Debug, Clone, Deserialize)]
pub struct ShapePoint {
    /// The identifier of the shape.
    pub shape_id: String,
    /// The latitude of the point, in WGS 84 degrees.
    pub shape_pt_lat: f64,
    /// The longitude of the point, in WGS 84 degrees.
    pub shape_pt_lon: f64,
    /// The order of the point in the shape. Values increase along the
    /// path.
    pub shape_pt_sequence: u32,
    /// The distance travelled along the shape up to this point.
    #[serde(default)]
    pub shape_dist_traveled: Option<f64>,
}
