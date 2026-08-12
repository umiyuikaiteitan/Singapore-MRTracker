//! Shared logic for the static board generator and the live
//! snapshot tool.

use mrt_datamall::{DataMallClient, TrainLine, UreqTransport};
use mrt_gtfs_rt::{Alert, RailRtFeed};

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
///   "trips": {"<trip_id>": {"d": 120, "c": 0, "s": {"<stop_id>": 60}}},
///   "alerts": [{"m": "...", "e": "sd", "p": [[1786500000, null]],
///               "r": ["NS"], "s": ["JUR_NS"], "t": ["trip-1"]}]
/// }
/// ```
///
/// In `trips`, `d` is the trip-level delay in seconds, `c` is `1`
/// for a canceled trip, and `s` maps stops to per-stop delays. Only
/// trips with real-time data appear.
///
/// `alerts` carries the GTFS-Realtime service alerts. `m` is the
/// display text, `e` the effect (`no` no service, `rs` reduced
/// service, `sd` significant delays, `dt` detour, `ms` modified
/// service, `ot` anything else), `p` the active periods as
/// `[start, end]` pairs with `null` for an open bound, and `r`, `s`,
/// and `t` the informed route, stop, and trip identifiers. The page
/// applies the periods against the visitor's clock, so an alert
/// takes effect and expires between snapshot refreshes. A no-service
/// alert that names a trip is also folded into `trips` as a
/// cancellation, which older cached pages understand.
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

    let mut trips = trip_updates_json(client);
    let rt_alerts = client
        .fetch_service_alerts()
        .ok()
        .and_then(|bytes| RailRtFeed::decode(&bytes).ok())
        .map(|feed| feed.alerts)
        .unwrap_or_default();
    if let serde_json::Value::Object(map) = &mut trips {
        fold_alert_cancellations(map, &rt_alerts, now_unix);
    }

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
        "alerts": rt_alerts_json(&rt_alerts, now_unix),
    })
}

/// Compress the service alerts into the snapshot form.
///
/// Alerts whose periods have all ended stay out. Alerts without an
/// informed entity carry a network-wide text; alerts without any
/// text and without an entity carry nothing, so they stay out too.
pub fn rt_alerts_json(alerts: &[Alert], now_unix: i64) -> serde_json::Value {
    let mut out = Vec::new();
    for alert in alerts {
        if alert.text().is_none() && alert.informed.is_empty() {
            continue;
        }
        // Keep future alerts: the page applies the periods itself.
        let expired = !alert.active_periods.is_empty()
            && alert
                .active_periods
                .iter()
                .all(|p| p.end.is_some_and(|end| (end as i64) < now_unix));
        if expired {
            continue;
        }
        let mut entry = serde_json::Map::new();
        if let Some(text) = alert.text() {
            entry.insert("m".to_string(), serde_json::json!(text));
        }
        entry.insert("e".to_string(), serde_json::json!(effect_code(alert)));
        if !alert.active_periods.is_empty() {
            let periods: Vec<serde_json::Value> = alert
                .active_periods
                .iter()
                .map(|p| serde_json::json!([p.start, p.end]))
                .collect();
            entry.insert("p".to_string(), serde_json::json!(periods));
        }
        let routes: Vec<&String> = alert
            .informed
            .iter()
            .filter_map(|e| e.route_id.as_ref())
            .collect();
        let stops: Vec<&String> = alert
            .informed
            .iter()
            .filter_map(|e| e.stop_id.as_ref())
            .collect();
        let trip_ids: Vec<&String> = alert
            .informed
            .iter()
            .filter_map(|e| e.trip_id.as_ref())
            .collect();
        if !routes.is_empty() {
            entry.insert("r".to_string(), serde_json::json!(routes));
        }
        if !stops.is_empty() {
            entry.insert("s".to_string(), serde_json::json!(stops));
        }
        if !trip_ids.is_empty() {
            entry.insert("t".to_string(), serde_json::json!(trip_ids));
        }
        out.push(serde_json::Value::Object(entry));
    }
    serde_json::json!(out)
}

/// The compact effect code of an alert.
fn effect_code(alert: &Alert) -> &'static str {
    use mrt_gtfs_rt::AlertEffect;
    match alert.effect {
        AlertEffect::NoService => "no",
        AlertEffect::ReducedService => "rs",
        AlertEffect::SignificantDelays => "sd",
        AlertEffect::Detour => "dt",
        AlertEffect::ModifiedService => "ms",
        _ => "ot",
    }
}

/// Fold active no-service alerts that name a trip into the trips
/// map as cancellations, so pages that predate the alerts array
/// still cancel the affected departures.
pub fn fold_alert_cancellations(
    trips: &mut serde_json::Map<String, serde_json::Value>,
    alerts: &[Alert],
    now_unix: i64,
) {
    for alert in alerts {
        if !alert.effect.stops_service() || !alert.is_active(now_unix.max(0) as u64) {
            continue;
        }
        for trip_id in alert.informed.iter().filter_map(|e| e.trip_id.as_ref()) {
            let entry = trips
                .entry(trip_id.clone())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if let serde_json::Value::Object(map) = entry {
                map.insert("c".to_string(), serde_json::json!(1));
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use mrt_gtfs_rt::{ActivePeriod, AlertCause, AlertEffect, InformedEntity};

    fn alert(effect: AlertEffect) -> Alert {
        Alert {
            cause: AlertCause::Unknown,
            effect,
            header: Some("Header".to_string()),
            description: Some("Description".to_string()),
            url: None,
            active_periods: Vec::new(),
            informed: Vec::new(),
        }
    }

    fn trip_entity(trip_id: &str) -> InformedEntity {
        InformedEntity {
            agency_id: None,
            route_id: None,
            stop_id: None,
            trip_id: Some(trip_id.to_string()),
        }
    }

    #[test]
    fn alerts_compress_into_the_snapshot_form() {
        let mut a = alert(AlertEffect::SignificantDelays);
        a.active_periods = vec![ActivePeriod {
            start: Some(100),
            end: None,
        }];
        a.informed = vec![
            InformedEntity {
                agency_id: None,
                route_id: Some("NS".to_string()),
                stop_id: Some("JUR_NS".to_string()),
                trip_id: None,
            },
            trip_entity("T1"),
        ];
        let json = rt_alerts_json(&[a], 1_000);
        let entry = &json.as_array().unwrap()[0];
        assert_eq!(entry["m"], "Header");
        assert_eq!(entry["e"], "sd");
        assert_eq!(entry["p"], serde_json::json!([[100, null]]));
        assert_eq!(entry["r"], serde_json::json!(["NS"]));
        assert_eq!(entry["s"], serde_json::json!(["JUR_NS"]));
        assert_eq!(entry["t"], serde_json::json!(["T1"]));
    }

    #[test]
    fn expired_alerts_stay_out_and_future_alerts_stay_in() {
        let mut expired = alert(AlertEffect::NoService);
        expired.active_periods = vec![ActivePeriod {
            start: Some(100),
            end: Some(200),
        }];
        let mut future = alert(AlertEffect::NoService);
        future.active_periods = vec![ActivePeriod {
            start: Some(5_000),
            end: Some(6_000),
        }];
        let json = rt_alerts_json(&[expired, future], 1_000);
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["p"], serde_json::json!([[5_000, 6_000]]));
    }

    #[test]
    fn an_alert_without_text_and_entities_stays_out() {
        let mut empty = alert(AlertEffect::NoService);
        empty.header = None;
        empty.description = None;
        let json = rt_alerts_json(&[empty], 1_000);
        assert!(json.as_array().unwrap().is_empty());
    }

    #[test]
    fn the_description_fills_in_for_a_missing_header() {
        let mut a = alert(AlertEffect::Detour);
        a.header = None;
        a.informed = vec![trip_entity("T1")];
        let json = rt_alerts_json(&[a], 1_000);
        assert_eq!(json[0]["m"], "Description");
        assert_eq!(json[0]["e"], "dt");
    }

    #[test]
    fn no_service_trip_alerts_fold_into_the_trips_map() {
        let mut trips = serde_json::Map::new();
        trips.insert("T1".to_string(), serde_json::json!({"d": 60}));
        let mut a = alert(AlertEffect::NoService);
        a.informed = vec![trip_entity("T1"), trip_entity("T2")];
        fold_alert_cancellations(&mut trips, &[a], 1_000);
        // The existing entry keeps its delay and gains the flag; the
        // unknown trip gets a fresh entry.
        assert_eq!(trips["T1"], serde_json::json!({"d": 60, "c": 1}));
        assert_eq!(trips["T2"], serde_json::json!({"c": 1}));
    }

    #[test]
    fn inactive_or_mild_alerts_fold_nothing() {
        let mut trips = serde_json::Map::new();
        let mut inactive = alert(AlertEffect::NoService);
        inactive.active_periods = vec![ActivePeriod {
            start: Some(5_000),
            end: Some(6_000),
        }];
        inactive.informed = vec![trip_entity("T1")];
        let mut mild = alert(AlertEffect::SignificantDelays);
        mild.informed = vec![trip_entity("T2")];
        fold_alert_cancellations(&mut trips, &[inactive, mild], 1_000);
        assert!(trips.is_empty());
    }
}
