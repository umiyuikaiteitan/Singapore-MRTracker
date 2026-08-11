//! Shared logic for the static board generator and the live
//! snapshot tool.

use mrt_datamall::{DataMallClient, TrainLine, UreqTransport};
use mrt_gtfs_rt::RailRtFeed;

/// Build the live snapshot: alerts, crowd levels, and trip updates.
///
/// The snapshot is one JSON object:
///
/// ```json
/// {
///   "generated": 1786500000,
///   "live": true,
///   "disrupted": false,
///   "segments": [{"line": "NEL", "stations": ["NE1"]}],
///   "messages": ["..."],
///   "crowd": {"NS1": "l"},
///   "trips": {"<trip_id>": {"d": 120, "c": 0, "s": {"<stop_id>": 60}}}
/// }
/// ```
///
/// In `trips`, `d` is the trip-level delay in seconds, `c` is `1`
/// for a canceled trip, and `s` maps stops to per-stop delays. Only
/// trips with real-time data appear.
pub fn live_snapshot(client: &DataMallClient<UreqTransport>, now_unix: i64) -> serde_json::Value {
    let alerts = client.train_service_alerts().ok();

    let mut crowd = serde_json::Map::new();
    for line in TrainLine::ALL {
        let Ok(records) = client.platform_crowd(line) else {
            continue;
        };
        for record in records {
            let level = match record.crowd_level {
                mrt_datamall::CrowdLevel::Low => "l",
                mrt_datamall::CrowdLevel::Moderate => "m",
                mrt_datamall::CrowdLevel::High => "h",
                mrt_datamall::CrowdLevel::Unknown => continue,
            };
            crowd.insert(record.station, serde_json::json!(level));
        }
    }

    let trips = trip_updates_json(client);

    let (disrupted, segments, messages) = match &alerts {
        Some(alerts) => (
            alerts.status == mrt_datamall::ServiceStatus::Disrupted,
            alerts
                .affected_segments
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "line": s.line,
                        "stations": s.station_codes(),
                    })
                })
                .collect::<Vec<_>>(),
            alerts
                .messages
                .iter()
                .map(|m| m.content.clone())
                .collect::<Vec<_>>(),
        ),
        None => (false, Vec::new(), Vec::new()),
    };

    serde_json::json!({
        "generated": now_unix,
        "live": true,
        "disrupted": disrupted,
        "segments": segments,
        "messages": messages,
        "crowd": crowd,
        "trips": trips,
    })
}

/// Fetch the GTFS-Realtime trip updates and compress them into a
/// per-trip map. Trips without a delay, a cancellation, or per-stop
/// data stay out of the map.
fn trip_updates_json(client: &DataMallClient<UreqTransport>) -> serde_json::Value {
    let feed = client
        .fetch_trip_updates()
        .ok()
        .and_then(|bytes| RailRtFeed::decode(&bytes).ok());
    let mut trips = serde_json::Map::new();
    let Some(feed) = feed else {
        return serde_json::Value::Object(trips);
    };
    for update in &feed.trip_updates {
        let Some(trip_id) = &update.trip_id else {
            continue;
        };
        let mut stops = serde_json::Map::new();
        for stop_update in &update.stop_updates {
            let Some(stop_id) = &stop_update.stop_id else {
                continue;
            };
            let delay = stop_update
                .departure
                .or(stop_update.arrival)
                .and_then(|event| event.delay_secs);
            if stop_update.skipped {
                stops.insert(stop_id.clone(), serde_json::json!("skip"));
            } else if let Some(delay) = delay {
                stops.insert(stop_id.clone(), serde_json::json!(delay));
            }
        }
        if !update.canceled && update.delay_secs.is_none() && stops.is_empty() {
            continue;
        }
        let mut entry = serde_json::Map::new();
        if let Some(delay) = update.delay_secs {
            entry.insert("d".to_string(), serde_json::json!(delay));
        }
        if update.canceled {
            entry.insert("c".to_string(), serde_json::json!(1));
        }
        if !stops.is_empty() {
            entry.insert("s".to_string(), serde_json::Value::Object(stops));
        }
        trips.insert(trip_id.clone(), serde_json::Value::Object(entry));
    }
    serde_json::Value::Object(trips)
}
