//! Export a bounded schedule for continuous, browser-side map playback.
use mrt_gtfs::{GtfsFeed, GtfsTime, RailNetwork, TripInstanceQuery};
use mrt_live::clock;
use serde_json::{json, Value};

fn schedule(network: &RailNetwork, now: i64) -> Value {
    let (today, time) = clock::sgt_from_unix(now);
    let midnight = now - i64::from(time.seconds());
    let valid_from = now - 3600;
    let valid_until = now + 36 * 3600;
    let mut runs = Vec::new();
    let mut bands = Vec::new();
    let mut missing = 0;
    for offset in -1..=2 {
        let date = today.plus_days(offset);
        let base = midnight + offset * 86400;
        let query = TripInstanceQuery::new(date)
            .window(GtfsTime::from_seconds(0), GtfsTime::from_seconds(48 * 3600));
        let result = network
            .query_trip_instances(&query)
            .expect("cannot expand service calendar");
        for trip in result.trips {
            let (Some(first), Some(last)) = (trip.first_time(), trip.last_time()) else {
                missing += 1;
                continue;
            };
            if base + i64::from(last.seconds()) < valid_from
                || base + i64::from(first.seconds()) > valid_until
            {
                continue;
            }
            if trip.has_missing_times() {
                missing += 1;
                continue;
            }
            let calls: Vec<Value> = trip
                .calls
                .iter()
                .map(|call| {
                    json!([
                        call.platform_stop_id,
                        call.arrival_or_departure().unwrap().seconds(),
                        call.departure_or_arrival().unwrap().seconds(),
                    ])
                })
                .collect();
            runs.push(json!({"id":trip.instance_id,"trip":trip.source_trip_id,
                "route":network.line(trip.line).route_id,"day":date.to_string(),
                "base":base,"headsign":trip.headsign,"frequency":trip.instance_id != format!("{}:{}",date,trip.source_trip_id),"computed":trip.has_interpolated_times(),"calls":calls}));
        }
        for band in result.frequency_bands {
            let start = base + i64::from(band.start.seconds());
            let end = base + i64::from(band.span_end().seconds());
            if end >= valid_from && start <= valid_until {
                bands.push(json!({"route":network.line(band.line).route_id,
                    "start":start,"end":end,"headway":band.headway_secs}));
            }
        }
    }
    assert!(
        missing == 0,
        "{missing} runs lack bounded stop times; refusing an incomplete map"
    );
    json!({"version":1,"generated":now,"validFrom":valid_from,"validUntil":valid_until,"runs":runs,"bands":bands})
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 3,
        "usage: mrt-map-schedule <output.json> <feed.zip>"
    );
    let feed = GtfsFeed::from_zip_path(&args[2]).expect("cannot read GTFS");
    let network = RailNetwork::from_feed(&feed).expect("cannot build rail network");
    let result = schedule(&network, clock::unix_now());
    eprintln!(
        "Map schedule: {} runs, {} headway bands",
        result["runs"].as_array().unwrap().len(),
        result["bands"].as_array().unwrap().len()
    );
    std::fs::write(&args[1], result.to_string()).expect("cannot write schedule");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedule_uses_service_dates_and_preserves_platform_calls() {
        let feed = GtfsFeed::from_dir("../mrt-gtfs/tests/fixtures/mini").unwrap();
        let network = RailNetwork::from_feed(&feed).unwrap();
        let data = schedule(&network, 1_786_392_108);
        assert_eq!(
            data["validUntil"].as_i64().unwrap() - data["generated"].as_i64().unwrap(),
            36 * 3600
        );
        assert!(!data["runs"].as_array().unwrap().is_empty());
        for run in data["runs"].as_array().unwrap() {
            assert!(run["day"].is_string());
            assert!(run["calls"]
                .as_array()
                .unwrap()
                .iter()
                .all(|call| call[0].is_string() && call[1].is_number() && call[2].is_number()));
        }
    }
}
