use std::collections::HashSet;

use mrt_gtfs::{
    FrequencyPolicy, GtfsTime, LineId, RailNetwork, ScheduledCall, StationId, TimeQuality,
    TripInstance, TripInstanceQuery,
};
use mrt_gtfs_rt::{Alert, RailRtFeed, StopTimeEvent, TripUpdate};

use crate::{
    black_pixels, checked_expiry, compact_text, Board, BoardRow, DisplayError, Frame, FrameOptions,
    Layout, Pixel, SourceState, SCHEMA_VERSION,
};

// Bound the searched schedule window. A matched delay outside this range
// suppresses that candidate instead of showing its obsolete scheduled time.
const MAX_SHIFT_SECS: i64 = 3600;

struct Candidate {
    station: StationId,
    line: LineId,
    wait_secs: u32,
    row: BoardRow,
    instance_id: String,
    expires: Option<u64>,
}

struct Projection {
    candidates: Vec<Candidate>,
    canceled: bool,
    stale: bool,
    unsupported_delay: bool,
    cancellation_expiry: Option<u64>,
    notices: Vec<String>,
}

/// Build a deterministic complete frame. Layout mappings are validated
/// against this exact network before any projection is produced.
///
/// Fresh matched delays adjust countdowns. Canceled trips, skipped calls,
/// no-pickup calls and terminal arrivals are excluded. Frequency runs need
/// an explicit matching start date and start time to receive realtime data.
/// Feed and update timestamps must never be in the future.
pub fn build_frame(
    network: &RailNetwork,
    layout: &Layout,
    realtime: Option<&RailRtFeed>,
    options: &FrameOptions,
) -> Result<Frame, DisplayError> {
    options.validate()?;
    let (board_stations, pixels, layout_warnings) = layout.resolve(network)?;
    let mut visible: HashSet<StationId> = pixels
        .iter()
        .flat_map(|p| p.stations.iter())
        .copied()
        .collect();
    visible.extend(board_stations.iter().copied());
    let fresh = realtime.filter(|feed| timestamp_fresh(feed.feed_timestamp, options));
    let mut projection = Projection {
        candidates: Vec::new(),
        canceled: false,
        stale: realtime.is_some() && fresh.is_none(),
        unsupported_delay: false,
        cancellation_expiry: None,
        notices: Vec::new(),
    };

    // Include late previous-service-day calls and next-calendar-day calls
    // when the horizon crosses midnight. Scheduled seconds remain on their
    // own service date throughout matching.
    for day_offset in [-1i64, 0, 1] {
        let now_on_day = i64::from(options.clock.seconds()) - day_offset * 86_400;
        let from = (now_on_day - MAX_SHIFT_SECS).max(0);
        let until = now_on_day + i64::from(options.lookahead_secs) + MAX_SHIFT_SECS;
        if until <= from {
            continue;
        }
        let query = TripInstanceQuery::new(options.service_date.plus_days(day_offset))
            .window(
                GtfsTime::from_seconds(from as u32),
                GtfsTime::from_seconds(until as u32),
            )
            .frequency_policy(FrequencyPolicy::ExpandApproximate);
        for trip in network.query_trip_instances(&query)?.trips {
            for (call_index, call) in trip.calls.iter().enumerate() {
                if !visible.contains(&call.station)
                    || !call.allows_pickup()
                    || call_index + 1 == trip.calls.len()
                {
                    continue;
                }
                let Some(departure) = call.departure_or_arrival() else {
                    continue;
                };
                let scheduled_wait = i64::from(departure.seconds()) - now_on_day;
                projection.add_call(network, &trip, call, scheduled_wait, fresh, options);
            }
        }
    }

    projection.candidates.sort_by(|a, b| {
        (
            a.wait_secs,
            a.row.line.as_str(),
            a.row.destination.as_str(),
            a.instance_id.as_str(),
        )
            .cmp(&(
                b.wait_secs,
                b.row.line.as_str(),
                b.row.destination.as_str(),
                b.instance_id.as_str(),
            ))
    });
    let board_candidates: Vec<_> = projection
        .candidates
        .iter()
        .filter(|entry| board_stations.contains(&entry.station))
        .take(options.max_rows)
        .collect();
    let mut selected = board_candidates.clone();
    let mut frame_pixels = black_pixels(layout.led_count);
    for pixel in &pixels {
        if let Some(candidate) = projection.candidates.iter().find(|entry| {
            pixel.stations.contains(&entry.station) && pixel.lines.contains(&entry.line)
        }) {
            frame_pixels[usize::from(pixel.index)] = color_pixel(network, candidate, pixel.index);
            selected.push(candidate);
        }
    }
    let has_realtime = selected.iter().any(|entry| entry.row.realtime) || projection.canceled;
    let source_state = if has_realtime {
        SourceState::Realtime
    } else if projection.stale {
        SourceState::Stale
    } else {
        SourceState::Schedule
    };
    if source_state == SourceState::Stale {
        frame_pixels = black_pixels(layout.led_count);
    }
    let mut valid_until = checked_expiry(options.now_unix, options.ttl_secs)?;
    for expires in selected
        .iter()
        .filter_map(|entry| entry.expires)
        .chain(projection.cancellation_expiry)
    {
        valid_until = valid_until.min(expires);
    }
    // Visible alert text also expires with the source, even if it carries
    // no numerical delay and therefore does not change row provenance.
    if !projection.notices.is_empty() {
        if let Some(timestamp) = fresh.and_then(|feed| feed.feed_timestamp) {
            valid_until =
                valid_until.min(timestamp.saturating_add(u64::from(options.max_rt_age_secs)));
        }
    }
    let mut notices = vec![match source_state {
        SourceState::Realtime => "Realtime where marked; other rows use timetable.".to_string(),
        SourceState::Stale => "Realtime data stale; timetable fallback. LEDs paused.".to_string(),
        _ => "Timetable departures; live predictions unavailable.".to_string(),
    }];
    if !layout_warnings.is_empty() {
        notices.push(format!(
            "{} layout identifier(s) absent; explicit aliases used.",
            layout_warnings.len()
        ));
    }
    if projection.canceled {
        notices.push("Canceled/skipped departures omitted.".into());
    }
    if projection.unsupported_delay {
        notices.push("Delays beyond one hour omitted.".into());
    }
    if board_candidates.is_empty() {
        notices.push("No upcoming departures in this window.".into());
    }
    if board_candidates.iter().any(|entry| entry.row.approximate) {
        notices.push("Approximate times marked ~.".into());
    }
    notices.extend(projection.notices);
    notices.push("Station departure activity.".into());
    Ok(Frame {
        schema_version: SCHEMA_VERSION,
        layout_id: layout.layout_id.clone(),
        generated_at: options.now_unix,
        valid_until,
        source_state,
        pixels: frame_pixels,
        board: Board {
            station_name: compact_text(&network.station(board_stations[0]).name, 80),
            rows: board_candidates
                .iter()
                .map(|entry| entry.row.clone())
                .collect(),
            notice: compact_text(&notices.join(" "), 240),
        },
    })
}

impl Projection {
    fn add_call(
        &mut self,
        network: &RailNetwork,
        trip: &TripInstance,
        call: &ScheduledCall,
        scheduled_wait: i64,
        feed: Option<&RailRtFeed>,
        options: &FrameOptions,
    ) {
        let line = network.line(trip.line);
        let mut delay = None;
        let mut canceled = false;
        let mut expires = None;
        if let Some(feed) = feed {
            // Reject ambiguous duplicates instead of picking feed order.
            let matching: Vec<_> = feed
                .trip_updates
                .iter()
                .filter(|update| matches_trip(update, trip, &line.route_id, options))
                .collect();
            if matching.len() == 1 {
                let update = matching[0];
                let measurement = update.timestamp.or(feed.feed_timestamp);
                if timestamp_fresh(measurement, options) {
                    canceled = update.canceled;
                    let stops: Vec<_> = update
                        .stop_updates
                        .iter()
                        .filter(|stop| {
                            stop.stop_id.as_deref() == Some(call.platform_stop_id.as_str())
                        })
                        .collect();
                    let unique_platform = trip
                        .calls
                        .iter()
                        .filter(|candidate| candidate.platform_stop_id == call.platform_stop_id)
                        .count()
                        == 1;
                    if stops.len() == 1 && unique_platform {
                        canceled |= stops[0].skipped;
                        delay = stops[0]
                            .departure
                            .and_then(|event| event_delay(event, scheduled_wait, options))
                            .or_else(|| {
                                stops[0].arrival.and_then(|event| {
                                    let arrival_wait = scheduled_wait
                                        + call.arrival.map_or(0, |arrival| {
                                            i64::from(arrival.seconds())
                                                - i64::from(
                                                    call.departure_or_arrival().unwrap().seconds(),
                                                )
                                        });
                                    event_delay(event, arrival_wait, options)
                                })
                            });
                    } else if update.stop_updates.is_empty() {
                        // The flattened RT model cannot represent NO_DATA.
                        // Never propagate trip-level delay across stop updates
                        // without an explicit numerical prediction at this call.
                        delay = update.delay_secs.map(i128::from);
                    }
                    if delay.is_some() || canceled {
                        expires = Some(
                            measurement
                                .unwrap()
                                .min(feed.feed_timestamp.unwrap())
                                .saturating_add(u64::from(options.max_rt_age_secs)),
                        );
                    }
                } else {
                    self.stale = true;
                }
            }
            for alert in &feed.alerts {
                if alert_active(alert, options.now_unix)
                    && alert_matches(alert, trip, call, &line.route_id)
                {
                    if let Some(text) = alert.text() {
                        if !self.notices.iter().any(|existing| existing == text) {
                            self.notices.push(compact_text(text, 160));
                        }
                    }
                    if alert.effect.stops_service() {
                        canceled = true;
                        let expiry = feed
                            .feed_timestamp
                            .unwrap()
                            .saturating_add(u64::from(options.max_rt_age_secs));
                        expires = Some(expires.map_or(expiry, |earlier| earlier.min(expiry)));
                    }
                }
            }
        }
        if canceled {
            if (0..=i64::from(options.lookahead_secs)).contains(&scheduled_wait) {
                self.canceled = true;
                if let Some(expiry) = expires {
                    self.cancellation_expiry = Some(
                        self.cancellation_expiry
                            .map_or(expiry, |current| current.min(expiry)),
                    );
                }
            }
            return;
        }
        if delay.is_some_and(|seconds| seconds.abs() > i128::from(MAX_SHIFT_SECS)) {
            self.unsupported_delay = true;
            return;
        }
        let wait = i128::from(scheduled_wait) + delay.unwrap_or(0);
        if !(0..=i128::from(options.lookahead_secs)).contains(&wait) {
            return;
        }
        let destination = call
            .stop_headsign
            .as_deref()
            .or(trip.headsign.as_deref())
            .or_else(|| {
                trip.terminus()
                    .map(|station| network.station(station).name.as_str())
            })
            .unwrap_or("Unknown destination");
        self.candidates.push(Candidate {
            station: call.station,
            line: trip.line,
            wait_secs: wait as u32,
            row: BoardRow {
                line: compact_text(&line.name, 12),
                destination: compact_text(destination, 96),
                minutes: (wait as u32).div_ceil(60),
                approximate: !trip.exactness.is_exact() || call.time_quality != TimeQuality::Exact,
                realtime: delay.is_some(),
            },
            instance_id: trip.instance_id.clone(),
            expires,
        });
    }
}

fn timestamp_fresh(timestamp: Option<u64>, options: &FrameOptions) -> bool {
    timestamp.is_some_and(|time| {
        time <= options.now_unix && options.now_unix - time < u64::from(options.max_rt_age_secs)
    })
}

fn matches_trip(
    update: &TripUpdate,
    trip: &TripInstance,
    route_id: &str,
    options: &FrameOptions,
) -> bool {
    if update.trip_id.as_deref() != Some(trip.source_trip_id.as_str())
        || update
            .route_id
            .as_ref()
            .is_some_and(|route| route != route_id)
        || update
            .direction_id
            .is_some_and(|direction| Some(direction) != trip.direction.map(u32::from))
    {
        return false;
    }
    if update
        .start_date
        .as_ref()
        .is_some_and(|date| date != &trip.service_date.to_string())
    {
        return false;
    }
    // A template's trip_id is shared by many physical runs. Only an exact
    // instance identity can attach its cancellation/delay to one of them.
    let frequency_prefix = format!("{}:{}@", trip.service_date, trip.source_trip_id);
    if let Some(instance_start) = trip.instance_id.strip_prefix(&frequency_prefix) {
        return update.start_date.is_some()
            && update
                .start_time
                .as_deref()
                .and_then(|time| time.parse::<GtfsTime>().ok())
                == instance_start.parse::<GtfsTime>().ok();
    }
    if update.start_date.is_none() && trip.service_date != options.service_date {
        return false;
    }
    update.start_time.as_deref().map_or(true, |time| {
        time.parse::<GtfsTime>().ok()
            == trip
                .calls
                .first()
                .and_then(ScheduledCall::departure_or_arrival)
    })
}

fn event_delay(event: StopTimeEvent, scheduled_wait: i64, options: &FrameOptions) -> Option<i128> {
    // GTFS explicitly gives absolute time precedence when both fields exist.
    // Wide signed arithmetic prevents corrupt epochs wrapping into a near
    // departure. add_call bounds the resulting deviation before conversion.
    event
        .time
        .map(|time| i128::from(time) - i128::from(options.now_unix) - i128::from(scheduled_wait))
        .or_else(|| event.delay_secs.map(i128::from))
}

fn alert_active(alert: &Alert, now: u64) -> bool {
    // GTFS end timestamps are exclusive.
    alert.active_periods.is_empty()
        || alert.active_periods.iter().any(|period| {
            period.start.map_or(true, |start| now >= start)
                && period.end.map_or(true, |end| now < end)
        })
}

fn alert_matches(alert: &Alert, trip: &TripInstance, call: &ScheduledCall, route_id: &str) -> bool {
    alert.informed.iter().any(|entity| {
        // Fields within a selector are ANDed; distinct selectors are ORed.
        // Agency-only selectors cannot be resolved by this network model.
        (entity.trip_id.is_some() || entity.route_id.is_some() || entity.stop_id.is_some())
            && entity
                .trip_id
                .as_deref()
                .map_or(true, |id| id == trip.source_trip_id)
            && entity.route_id.as_deref().map_or(true, |id| id == route_id)
            && entity
                .stop_id
                .as_deref()
                .map_or(true, |id| id == call.platform_stop_id)
    })
}

fn color_pixel(network: &RailNetwork, candidate: &Candidate, index: u16) -> Pixel {
    let hex = network
        .line(candidate.line)
        .color
        .as_deref()
        .unwrap_or("FFFFFF");
    let rgb = if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        u32::from_str_radix(hex, 16).unwrap_or(0xFFFFFF)
    } else {
        0xFFFFFF
    };
    // Conservative software brightness; the controller still applies its
    // own independent current limit. Scheduled activity remains visibly dim.
    let scale = if candidate.row.realtime { 96u32 } else { 24u32 };
    Pixel {
        index,
        r: (((rgb >> 16) & 255) * scale / 255) as u8,
        g: (((rgb >> 8) & 255) * scale / 255) as u8,
        b: ((rgb & 255) * scale / 255) as u8,
    }
}
