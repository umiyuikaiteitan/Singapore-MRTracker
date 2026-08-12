//! # mrt-gtfs-rt
//!
//! Decode GTFS-Realtime feeds for Singapore rail.
//!
//! LTA DataMall publishes two GTFS-Realtime feeds for trains:
//! trip updates (`GTFSRealtimeTrainTripUpdates`) and service alerts
//! (`GTFSRealTimeTrainServiceAlerts`). Both feeds are Protocol Buffer
//! files that follow the standard `gtfs-realtime.proto` schema from
//! Google's GTFS specification.
//!
//! This crate has two layers:
//!
//! 1. [`transit_realtime`] — the full generated Protocol Buffer model.
//!    Use it when you need every field of the specification.
//! 2. [`RailRtFeed`] — a simple, flat view of the same data. Use it
//!    when you build maps, destination boards, or LED panels.
//!
//! The Protocol Buffer code is generated once and committed to the
//! repository, so this crate does not need `protoc` at build time.
//! See `scripts/regenerate-gtfs-rt.sh` in the repository root.
//!
//! # Example
//!
//! ```
//! use mrt_gtfs_rt::RailRtFeed;
//!
//! fn show(feed_bytes: &[u8]) {
//!     let feed = RailRtFeed::decode(feed_bytes).unwrap();
//!     for update in &feed.trip_updates {
//!         println!(
//!             "trip {:?} has {} stop updates",
//!             update.trip_id,
//!             update.stop_updates.len()
//!         );
//!     }
//! }
//! ```

#![warn(missing_docs)]

/// The generated GTFS-Realtime Protocol Buffer model.
///
/// The module content comes from `gtfs-realtime.proto`, compiled with
/// `prost-build`. Do not edit the generated file by hand.
#[allow(missing_docs, clippy::all, rustdoc::all)]
pub mod transit_realtime {
    include!("transit_realtime.rs");
}

use prost::Message as _;
use serde::Serialize;

use transit_realtime as pb;

/// An error that occurs when the library decodes a GTFS-Realtime feed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RtError {
    /// The bytes are not a valid GTFS-Realtime message.
    #[error("cannot decode the GTFS-Realtime message: {0}")]
    Decode(String),
}

/// Decode a GTFS-Realtime message into the full Protocol Buffer model.
pub fn decode_feed(bytes: &[u8]) -> Result<pb::FeedMessage, RtError> {
    pb::FeedMessage::decode(bytes).map_err(|e| RtError::Decode(e.to_string()))
}

/// A simple, flat view of one GTFS-Realtime feed message.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RailRtFeed {
    /// The time when the server created the feed, in POSIX seconds.
    pub feed_timestamp: Option<u64>,
    /// The trip updates in the feed.
    pub trip_updates: Vec<TripUpdate>,
    /// The service alerts in the feed.
    pub alerts: Vec<Alert>,
    /// The vehicle positions in the feed.
    pub vehicle_positions: Vec<VehiclePosition>,
}

impl RailRtFeed {
    /// Decode a GTFS-Realtime message and flatten it.
    pub fn decode(bytes: &[u8]) -> Result<Self, RtError> {
        Ok(Self::from_message(&decode_feed(bytes)?))
    }

    /// Flatten a decoded Protocol Buffer message.
    pub fn from_message(message: &pb::FeedMessage) -> Self {
        let mut feed = RailRtFeed {
            feed_timestamp: message.header.timestamp,
            ..Default::default()
        };
        for entity in &message.entity {
            if entity.is_deleted() {
                continue;
            }
            if let Some(tu) = &entity.trip_update {
                feed.trip_updates.push(TripUpdate::from_pb(tu));
            }
            if let Some(alert) = &entity.alert {
                feed.alerts.push(Alert::from_pb(alert));
            }
            if let Some(vp) = &entity.vehicle {
                feed.vehicle_positions.push(VehiclePosition::from_pb(vp));
            }
        }
        feed
    }
}

/// A real-time update for one trip.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TripUpdate {
    /// The GTFS trip identifier. Resolve it against the GTFS Schedule
    /// feed.
    pub trip_id: Option<String>,
    /// The GTFS route identifier.
    pub route_id: Option<String>,
    /// The GTFS direction of the trip.
    pub direction_id: Option<u32>,
    /// The start date of the trip, in `YYYYMMDD` format.
    pub start_date: Option<String>,
    /// The scheduled start time of the trip, in `HH:MM:SS` format.
    pub start_time: Option<String>,
    /// The delay of the trip, in seconds. A positive value means late.
    pub delay_secs: Option<i32>,
    /// The time when the operator measured the update, in POSIX
    /// seconds.
    pub timestamp: Option<u64>,
    /// `true` if the operator canceled the trip.
    pub canceled: bool,
    /// The per-stop updates of the trip, in stop order.
    pub stop_updates: Vec<StopTimeUpdate>,
}

impl TripUpdate {
    fn from_pb(tu: &pb::TripUpdate) -> Self {
        let trip = &tu.trip;
        TripUpdate {
            trip_id: trip.trip_id.clone(),
            route_id: trip.route_id.clone(),
            direction_id: trip.direction_id,
            start_date: trip.start_date.clone(),
            start_time: trip.start_time.clone(),
            delay_secs: tu.delay,
            timestamp: tu.timestamp,
            canceled: trip.schedule_relationship()
                == pb::trip_descriptor::ScheduleRelationship::Canceled,
            stop_updates: tu
                .stop_time_update
                .iter()
                .map(StopTimeUpdate::from_pb)
                .collect(),
        }
    }
}

/// A real-time update for one stop of one trip.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StopTimeUpdate {
    /// The GTFS stop identifier.
    pub stop_id: Option<String>,
    /// The order of the stop in the trip.
    pub stop_sequence: Option<u32>,
    /// The predicted arrival.
    pub arrival: Option<StopTimeEvent>,
    /// The predicted departure.
    pub departure: Option<StopTimeEvent>,
    /// `true` if the trip does not call at this stop.
    pub skipped: bool,
}

impl StopTimeUpdate {
    fn from_pb(stu: &pb::trip_update::StopTimeUpdate) -> Self {
        StopTimeUpdate {
            stop_id: stu.stop_id.clone(),
            stop_sequence: stu.stop_sequence,
            arrival: stu.arrival.as_ref().map(StopTimeEvent::from_pb),
            departure: stu.departure.as_ref().map(StopTimeEvent::from_pb),
            skipped: stu.schedule_relationship()
                == pb::trip_update::stop_time_update::ScheduleRelationship::Skipped,
        }
    }
}

/// A predicted arrival or departure.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct StopTimeEvent {
    /// The predicted time, in POSIX seconds.
    pub time: Option<i64>,
    /// The deviation from the schedule, in seconds. A positive value
    /// means late.
    pub delay_secs: Option<i32>,
}

impl StopTimeEvent {
    fn from_pb(event: &pb::trip_update::StopTimeEvent) -> Self {
        StopTimeEvent {
            time: event.time,
            delay_secs: event.delay,
        }
    }
}

/// The cause of a service alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(missing_docs)]
pub enum AlertCause {
    Unknown,
    Other,
    TechnicalProblem,
    Strike,
    Demonstration,
    Accident,
    Holiday,
    Weather,
    Maintenance,
    Construction,
    PoliceActivity,
    MedicalEmergency,
}

/// The effect of a service alert on the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(missing_docs)]
pub enum AlertEffect {
    NoService,
    ReducedService,
    SignificantDelays,
    Detour,
    AdditionalService,
    ModifiedService,
    Other,
    Unknown,
    StopMoved,
    NoEffect,
    AccessibilityIssue,
}

/// A time range in which an alert is active.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct ActivePeriod {
    /// The start of the range, in POSIX seconds. `None` means "always
    /// in the past".
    pub start: Option<u64>,
    /// The end of the range, in POSIX seconds. `None` means "until
    /// further notice".
    pub end: Option<u64>,
}

/// A network part that an alert applies to.
#[derive(Debug, Clone, Default, Serialize)]
pub struct InformedEntity {
    /// The GTFS agency identifier.
    pub agency_id: Option<String>,
    /// The GTFS route identifier.
    pub route_id: Option<String>,
    /// The GTFS stop identifier.
    pub stop_id: Option<String>,
    /// The GTFS trip identifier.
    pub trip_id: Option<String>,
}

/// A service alert.
#[derive(Debug, Clone, Serialize)]
pub struct Alert {
    /// The cause of the alert.
    pub cause: AlertCause,
    /// The effect of the alert.
    pub effect: AlertEffect,
    /// The short summary text of the alert.
    pub header: Option<String>,
    /// The full description text of the alert.
    pub description: Option<String>,
    /// A URL with more information.
    pub url: Option<String>,
    /// The time ranges in which the alert is active.
    pub active_periods: Vec<ActivePeriod>,
    /// The network parts that the alert applies to.
    pub informed: Vec<InformedEntity>,
}

impl AlertEffect {
    /// Report whether the effect removes service entirely.
    ///
    /// A board treats an affected departure like a cancellation.
    pub fn stops_service(self) -> bool {
        matches!(self, AlertEffect::NoService)
    }

    /// Report whether the effect disturbs the timetable without
    /// removing service.
    ///
    /// A board marks an affected departure as disturbed, but keeps
    /// its scheduled time: the alert carries no delay figure.
    ///
    /// [`AlertEffect::ModifiedService`] stays out. The LTA feed uses
    /// it for planned adjustments that run for months, for example
    /// the Sengkang West LRT loop closure, and those already sit in
    /// the published timetable. Marking every departure of such a
    /// line disturbed would leave the line permanently red and hide
    /// the disruptions that matter. The alert still reaches the
    /// board as a notice.
    pub fn disturbs_service(self) -> bool {
        matches!(
            self,
            AlertEffect::ReducedService | AlertEffect::SignificantDelays | AlertEffect::Detour
        )
    }
}

impl Alert {
    /// Report whether the alert is active at the given POSIX time.
    ///
    /// An alert without active periods is always active, as the
    /// GTFS-Realtime specification defines. An open start means
    /// "since forever", an open end means "until further notice".
    pub fn is_active(&self, unix_secs: u64) -> bool {
        if self.active_periods.is_empty() {
            return true;
        }
        self.active_periods.iter().any(|period| {
            period.start.map_or(true, |start| unix_secs >= start)
                && period.end.map_or(true, |end| unix_secs <= end)
        })
    }

    /// Get the best display text: the header, or else the
    /// description.
    ///
    /// A blank string is no text. The LTA feed publishes alerts
    /// whose header is one space, and a board would scroll an empty
    /// notice for them.
    pub fn text(&self) -> Option<&str> {
        [self.header.as_deref(), self.description.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|text| !text.is_empty())
    }

    fn from_pb(alert: &pb::Alert) -> Self {
        Alert {
            cause: match alert.cause() {
                pb::alert::Cause::UnknownCause => AlertCause::Unknown,
                pb::alert::Cause::OtherCause => AlertCause::Other,
                pb::alert::Cause::TechnicalProblem => AlertCause::TechnicalProblem,
                pb::alert::Cause::Strike => AlertCause::Strike,
                pb::alert::Cause::Demonstration => AlertCause::Demonstration,
                pb::alert::Cause::Accident => AlertCause::Accident,
                pb::alert::Cause::Holiday => AlertCause::Holiday,
                pb::alert::Cause::Weather => AlertCause::Weather,
                pb::alert::Cause::Maintenance => AlertCause::Maintenance,
                pb::alert::Cause::Construction => AlertCause::Construction,
                pb::alert::Cause::PoliceActivity => AlertCause::PoliceActivity,
                pb::alert::Cause::MedicalEmergency => AlertCause::MedicalEmergency,
            },
            effect: match alert.effect() {
                pb::alert::Effect::NoService => AlertEffect::NoService,
                pb::alert::Effect::ReducedService => AlertEffect::ReducedService,
                pb::alert::Effect::SignificantDelays => AlertEffect::SignificantDelays,
                pb::alert::Effect::Detour => AlertEffect::Detour,
                pb::alert::Effect::AdditionalService => AlertEffect::AdditionalService,
                pb::alert::Effect::ModifiedService => AlertEffect::ModifiedService,
                pb::alert::Effect::OtherEffect => AlertEffect::Other,
                pb::alert::Effect::UnknownEffect => AlertEffect::Unknown,
                pb::alert::Effect::StopMoved => AlertEffect::StopMoved,
                pb::alert::Effect::NoEffect => AlertEffect::NoEffect,
                pb::alert::Effect::AccessibilityIssue => AlertEffect::AccessibilityIssue,
            },
            header: alert.header_text.as_ref().and_then(best_translation),
            description: alert.description_text.as_ref().and_then(best_translation),
            url: alert.url.as_ref().and_then(best_translation),
            active_periods: alert
                .active_period
                .iter()
                .map(|p| ActivePeriod {
                    start: p.start,
                    end: p.end,
                })
                .collect(),
            informed: alert
                .informed_entity
                .iter()
                .map(|e| InformedEntity {
                    agency_id: e.agency_id.clone(),
                    route_id: e.route_id.clone(),
                    stop_id: e.stop_id.clone(),
                    trip_id: e.trip.as_ref().and_then(|t| t.trip_id.clone()),
                })
                .collect(),
        }
    }
}

/// The stop status of a vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StopStatus {
    /// The vehicle is about to arrive at the stop.
    IncomingAt,
    /// The vehicle stands at the stop.
    StoppedAt,
    /// The vehicle departed the previous stop and moves to the stop.
    InTransitTo,
}

/// The real-time position of one vehicle.
#[derive(Debug, Clone, Default, Serialize)]
pub struct VehiclePosition {
    /// The GTFS trip identifier of the vehicle.
    pub trip_id: Option<String>,
    /// The GTFS route identifier of the vehicle.
    pub route_id: Option<String>,
    /// The latitude, in WGS 84 degrees.
    pub latitude: Option<f32>,
    /// The longitude, in WGS 84 degrees.
    pub longitude: Option<f32>,
    /// The bearing, in degrees clockwise from north.
    pub bearing: Option<f32>,
    /// The stop that the status refers to.
    pub stop_id: Option<String>,
    /// The order of that stop in the trip.
    pub current_stop_sequence: Option<u32>,
    /// The stop status of the vehicle.
    pub status: Option<StopStatus>,
    /// The time of the measurement, in POSIX seconds.
    pub timestamp: Option<u64>,
}

impl VehiclePosition {
    fn from_pb(vp: &pb::VehiclePosition) -> Self {
        VehiclePosition {
            trip_id: vp.trip.as_ref().and_then(|t| t.trip_id.clone()),
            route_id: vp.trip.as_ref().and_then(|t| t.route_id.clone()),
            latitude: vp.position.as_ref().map(|p| p.latitude),
            longitude: vp.position.as_ref().map(|p| p.longitude),
            bearing: vp.position.as_ref().and_then(|p| p.bearing),
            stop_id: vp.stop_id.clone(),
            current_stop_sequence: vp.current_stop_sequence,
            status: vp.current_status.map(|_| match vp.current_status() {
                pb::vehicle_position::VehicleStopStatus::IncomingAt => StopStatus::IncomingAt,
                pb::vehicle_position::VehicleStopStatus::StoppedAt => StopStatus::StoppedAt,
                pb::vehicle_position::VehicleStopStatus::InTransitTo => StopStatus::InTransitTo,
            }),
            timestamp: vp.timestamp,
        }
    }
}

/// Pick the best text from a translated string.
///
/// The function prefers the English translation. If the string has no
/// English translation, the function takes the first one.
fn best_translation(text: &pb::TranslatedString) -> Option<String> {
    text.translation
        .iter()
        .find(|t| matches!(t.language.as_deref(), Some("en")))
        .or_else(|| text.translation.first())
        .map(|t| t.text.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a small feed message like the DataMall train feeds.
    fn sample_message() -> pb::FeedMessage {
        pb::FeedMessage {
            header: pb::FeedHeader {
                gtfs_realtime_version: "2.0".to_string(),
                incrementality: None,
                timestamp: Some(1_754_000_000),
                feed_version: None,
            },
            entity: vec![
                pb::FeedEntity {
                    id: "tu-1".to_string(),
                    trip_update: Some(pb::TripUpdate {
                        trip: pb::TripDescriptor {
                            trip_id: Some("NS-T1".to_string()),
                            route_id: Some("NSL".to_string()),
                            direction_id: Some(0),
                            start_date: Some("20260810".to_string()),
                            ..Default::default()
                        },
                        stop_time_update: vec![pb::trip_update::StopTimeUpdate {
                            stop_id: Some("NS4".to_string()),
                            stop_sequence: Some(2),
                            arrival: Some(pb::trip_update::StopTimeEvent {
                                delay: Some(120),
                                time: Some(1_754_000_100),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }],
                        timestamp: Some(1_754_000_050),
                        delay: Some(120),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                pb::FeedEntity {
                    id: "alert-1".to_string(),
                    alert: Some(pb::Alert {
                        cause: Some(pb::alert::Cause::TechnicalProblem as i32),
                        effect: Some(pb::alert::Effect::SignificantDelays as i32),
                        header_text: Some(pb::TranslatedString {
                            translation: vec![pb::translated_string::Translation {
                                text: "NSL delays of 10 minutes".to_string(),
                                language: Some("en".to_string()),
                            }],
                        }),
                        informed_entity: vec![pb::EntitySelector {
                            route_id: Some("NSL".to_string()),
                            ..Default::default()
                        }],
                        active_period: vec![pb::TimeRange {
                            start: Some(1_753_999_000),
                            end: None,
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn decode_and_flatten_a_feed() {
        let bytes = sample_message().encode_to_vec();
        let feed = RailRtFeed::decode(&bytes).unwrap();

        assert_eq!(feed.feed_timestamp, Some(1_754_000_000));
        assert_eq!(feed.trip_updates.len(), 1);
        assert_eq!(feed.alerts.len(), 1);
        assert!(feed.vehicle_positions.is_empty());

        let tu = &feed.trip_updates[0];
        assert_eq!(tu.trip_id.as_deref(), Some("NS-T1"));
        assert_eq!(tu.route_id.as_deref(), Some("NSL"));
        assert_eq!(tu.delay_secs, Some(120));
        assert!(!tu.canceled);
        let stu = &tu.stop_updates[0];
        assert_eq!(stu.stop_id.as_deref(), Some("NS4"));
        assert_eq!(stu.arrival.unwrap().delay_secs, Some(120));
        assert!(!stu.skipped);

        let alert = &feed.alerts[0];
        assert_eq!(alert.cause, AlertCause::TechnicalProblem);
        assert_eq!(alert.effect, AlertEffect::SignificantDelays);
        assert_eq!(alert.header.as_deref(), Some("NSL delays of 10 minutes"));
        assert_eq!(alert.informed[0].route_id.as_deref(), Some("NSL"));
        assert_eq!(alert.active_periods[0].start, Some(1_753_999_000));
        assert_eq!(alert.active_periods[0].end, None);
    }

    #[test]
    fn canceled_and_skipped_flags() {
        let mut message = sample_message();
        message.entity[0]
            .trip_update
            .as_mut()
            .unwrap()
            .trip
            .set_schedule_relationship(pb::trip_descriptor::ScheduleRelationship::Canceled);
        message.entity[0]
            .trip_update
            .as_mut()
            .unwrap()
            .stop_time_update[0]
            .set_schedule_relationship(
                pb::trip_update::stop_time_update::ScheduleRelationship::Skipped,
            );
        let feed = RailRtFeed::from_message(&message);
        assert!(feed.trip_updates[0].canceled);
        assert!(feed.trip_updates[0].stop_updates[0].skipped);
    }

    #[test]
    fn deleted_entities_are_ignored() {
        let mut message = sample_message();
        message.entity[0].is_deleted = Some(true);
        let feed = RailRtFeed::from_message(&message);
        assert!(feed.trip_updates.is_empty());
        assert_eq!(feed.alerts.len(), 1);
    }

    #[test]
    fn invalid_bytes_are_an_error() {
        let result = RailRtFeed::decode(&[0xFF, 0xFF, 0xFF]);
        assert!(matches!(result, Err(RtError::Decode(_))));
    }

    #[test]
    fn translations_prefer_english() {
        let text = pb::TranslatedString {
            translation: vec![
                pb::translated_string::Translation {
                    text: "中文".to_string(),
                    language: Some("zh".to_string()),
                },
                pb::translated_string::Translation {
                    text: "English".to_string(),
                    language: Some("en".to_string()),
                },
            ],
        };
        assert_eq!(best_translation(&text).as_deref(), Some("English"));
    }

    fn alert_with_periods(periods: Vec<ActivePeriod>) -> Alert {
        Alert {
            cause: AlertCause::Unknown,
            effect: AlertEffect::NoService,
            header: None,
            description: None,
            url: None,
            active_periods: periods,
            informed: Vec::new(),
        }
    }

    #[test]
    fn an_alert_without_periods_is_always_active() {
        assert!(alert_with_periods(Vec::new()).is_active(0));
        assert!(alert_with_periods(Vec::new()).is_active(u64::MAX));
    }

    #[test]
    fn active_periods_bound_the_alert() {
        let alert = alert_with_periods(vec![ActivePeriod {
            start: Some(100),
            end: Some(200),
        }]);
        assert!(!alert.is_active(99));
        assert!(alert.is_active(100));
        assert!(alert.is_active(200));
        assert!(!alert.is_active(201));
    }

    #[test]
    fn open_ended_periods_stay_active() {
        let since = alert_with_periods(vec![ActivePeriod {
            start: Some(100),
            end: None,
        }]);
        assert!(since.is_active(u64::MAX));
        assert!(!since.is_active(99));

        let until = alert_with_periods(vec![ActivePeriod {
            start: None,
            end: Some(200),
        }]);
        assert!(until.is_active(0));
        assert!(!until.is_active(201));
    }

    #[test]
    fn any_of_several_periods_activates_the_alert() {
        let alert = alert_with_periods(vec![
            ActivePeriod {
                start: Some(100),
                end: Some(200),
            },
            ActivePeriod {
                start: Some(400),
                end: Some(500),
            },
        ]);
        assert!(alert.is_active(150));
        assert!(!alert.is_active(300));
        assert!(alert.is_active(450));
    }

    #[test]
    fn effects_classify_for_the_board() {
        assert!(AlertEffect::NoService.stops_service());
        assert!(!AlertEffect::NoService.disturbs_service());
        for effect in [
            AlertEffect::ReducedService,
            AlertEffect::SignificantDelays,
            AlertEffect::Detour,
        ] {
            assert!(effect.disturbs_service(), "{effect:?} must disturb");
            assert!(!effect.stops_service());
        }
        for effect in [
            AlertEffect::ModifiedService,
            AlertEffect::AdditionalService,
            AlertEffect::Other,
            AlertEffect::Unknown,
            AlertEffect::StopMoved,
            AlertEffect::NoEffect,
            AlertEffect::AccessibilityIssue,
        ] {
            assert!(!effect.stops_service());
            assert!(!effect.disturbs_service());
        }
    }

    #[test]
    fn a_modified_schedule_is_a_notice_only() {
        // The LTA feed marks months-long planned adjustments this
        // way. They must not paint a whole line as disturbed.
        assert!(!AlertEffect::ModifiedService.disturbs_service());
        assert!(!AlertEffect::ModifiedService.stops_service());
    }

    #[test]
    fn blank_alert_text_counts_as_no_text() {
        let mut alert = alert_with_periods(Vec::new());
        alert.header = Some(" ".to_string());
        alert.description = None;
        assert_eq!(alert.text(), None);

        // A blank header falls through to the description.
        alert.description = Some("Modified Schedule".to_string());
        assert_eq!(alert.text(), Some("Modified Schedule"));

        // Surrounding space never reaches the board.
        alert.header = Some("  Signal fault  ".to_string());
        assert_eq!(alert.text(), Some("Signal fault"));

        alert.header = None;
        alert.description = Some("\n\t ".to_string());
        assert_eq!(alert.text(), None);
    }
}
