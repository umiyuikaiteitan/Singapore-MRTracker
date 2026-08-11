//! Integration tests for the feed loader, the rail filter, the
//! network model, and the schedule queries.
//!
//! The tests use the miniature feed in `tests/fixtures/mini`. The feed
//! models a small Singapore-flavored network: two MRT lines (NSL,
//! EWL), one LRT line (BPL) with frequency-based service, and one bus
//! route that the rail filter must remove.

use std::path::PathBuf;

use mrt_gtfs::{GtfsFeed, GtfsTime, RailFilter, RailNetwork, ServiceDate, StationId};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini")
}

fn load_feed() -> GtfsFeed {
    GtfsFeed::from_dir(fixture_dir()).unwrap()
}

fn network() -> RailNetwork {
    RailNetwork::from_feed(&load_feed()).unwrap()
}

fn date(s: &str) -> ServiceDate {
    s.parse().unwrap()
}

fn time(s: &str) -> GtfsTime {
    s.parse().unwrap()
}

// ----------------------------------------------------------------------
// Feed loading
// ----------------------------------------------------------------------

#[test]
fn the_fixture_feed_loads_completely() {
    let feed = load_feed();
    assert_eq!(feed.agencies.len(), 2);
    assert_eq!(feed.stops.len(), 15);
    assert_eq!(feed.routes.len(), 4);
    assert_eq!(feed.trips.len(), 8);
    assert_eq!(feed.stop_times.len(), 22);
    assert_eq!(feed.calendar.len(), 2);
    assert_eq!(feed.calendar_dates.len(), 2);
    assert_eq!(feed.frequencies.len(), 1);
    assert_eq!(feed.transfers.len(), 3);
    assert_eq!(feed.shapes.len(), 3);
}

// ----------------------------------------------------------------------
// Rail filter
// ----------------------------------------------------------------------

#[test]
fn the_rail_filter_removes_bus_data() {
    let rail = RailFilter::default().apply(&load_feed());

    let route_ids: Vec<&str> = rail.routes.iter().map(|r| r.route_id.as_str()).collect();
    assert_eq!(route_ids, vec!["NS", "EW", "BP"]);

    assert!(rail.trips.iter().all(|t| t.trip_id != "BUS_T1"));
    assert!(rail.stop_times.iter().all(|st| st.trip_id != "BUS_T1"));
    assert!(rail
        .stops
        .iter()
        .all(|s| s.stop_id != "B01" && s.stop_id != "B02"));

    // The parent stations of the kept platforms stay in the feed.
    assert!(rail.stops.iter().any(|s| s.stop_id == "JUR"));

    // Only the agency of the kept routes stays.
    let agency_ids: Vec<_> = rail
        .agencies
        .iter()
        .filter_map(|a| a.agency_id.as_deref())
        .collect();
    assert_eq!(agency_ids, vec!["SMRT"]);

    // The transfers between kept stops stay.
    assert_eq!(rail.transfers.len(), 3);
}

// ----------------------------------------------------------------------
// Network model
// ----------------------------------------------------------------------

#[test]
fn the_network_groups_stops_into_stations() {
    let network = network();

    assert_eq!(network.lines().len(), 3);
    // JUR, CCK, MRB, BNV, RFP, and the standalone South View stop.
    assert_eq!(network.stations().len(), 6);

    let jurong = network.station(network.station_by_code("NS1").unwrap());
    assert_eq!(jurong.name, "Jurong East");
    assert_eq!(jurong.codes, vec!["NS1", "EW24"]);
    assert_eq!(jurong.platform_stop_ids, vec!["JUR_NS", "JUR_EW"]);
    assert!(jurong.lat.is_some());

    // A stop without a parent station becomes its own station.
    let south_view = network.station(network.station_by_code("BP2").unwrap());
    assert_eq!(south_view.name, "South View");
    assert_eq!(south_view.platform_stop_ids, vec!["STH_BP"]);
}

#[test]
fn station_lookups_ignore_case() {
    let network = network();
    assert!(network.station_by_code("ns1").is_some());
    assert!(network.station_by_name("MARINA BAY").is_some());
    assert!(network.station_by_code("XX99").is_none());
    assert_eq!(
        network.station_by_code("EW24"),
        network.station_for_stop("JUR_EW")
    );
}

#[test]
fn aliases_resolve_every_code_in_any_spelling() {
    let network = network();
    let jurong = network.station_by_code("NS1").unwrap();

    // Every code of the interchange, in any spelling.
    for spelling in ["NS1", "ns1", "ns-1", "NS 1", " ns1 ", "EW24", "ew-24"] {
        assert_eq!(
            network.station_by_alias(spelling),
            Some(jurong),
            "alias {spelling:?} does not resolve to Jurong East"
        );
    }

    // Every station code of the network resolves back to its station.
    for (index, station) in network.stations().iter().enumerate() {
        for code in &station.codes {
            assert_eq!(network.station_by_alias(code), Some(StationId(index)));
        }
    }

    assert_eq!(network.station_by_alias("XX99"), None);
    assert_eq!(network.station_by_alias(""), None);
}

#[test]
fn a_station_name_is_not_an_alias() {
    // Names repeat in the official feed, so a name in a URL would
    // name an arbitrary station. `station_by_name` stays for callers
    // that accept that, for example a command line.
    let network = network();
    assert_eq!(network.station_by_alias("Jurong East"), None);
    assert_eq!(network.station_by_alias("jurong-east"), None);
    assert!(network.station_by_name("Jurong East").is_some());
}

#[test]
fn interchanges_are_stations_with_more_than_one_line() {
    let network = network();
    let names: Vec<&str> = network
        .interchanges()
        .map(|id| network.station(id).name.as_str())
        .collect();
    assert_eq!(names, vec!["Jurong East", "Choa Chu Kang"]);
}

#[test]
fn lines_keep_route_metadata() {
    let network = network();
    let nsl = network.line(network.line_by_route_id("NS").unwrap());
    assert_eq!(nsl.name, "NSL");
    assert_eq!(nsl.long_name.as_deref(), Some("North South Line"));
    assert_eq!(nsl.color.as_deref(), Some("D42E12"));
    assert_eq!(nsl.route_type, 1);
}

#[test]
fn patterns_follow_travel_order() {
    let network = network();
    let nsl = network.line_by_route_id("NS").unwrap();
    let patterns: Vec<_> = network.patterns_for_line(nsl).collect();
    // Southbound and northbound. The three southbound trips share one
    // pattern.
    assert_eq!(patterns.len(), 2);

    let southbound = patterns.iter().find(|p| p.direction == Some(0)).unwrap();
    let names: Vec<&str> = southbound
        .stations
        .iter()
        .map(|&id| network.station(id).name.as_str())
        .collect();
    assert_eq!(names, vec!["Jurong East", "Choa Chu Kang", "Marina Bay"]);
}

#[test]
fn transfers_map_to_station_pairs() {
    let network = network();
    // JUR_NS<->JUR_EW collapses to one station and disappears.
    // CCK_NS->CCK_BP collapses too. All fixture transfers are inside
    // one station, so none survive at station level.
    assert!(network.transfers().is_empty());
}

// ----------------------------------------------------------------------
// Service calendar
// ----------------------------------------------------------------------

#[test]
fn weekly_service_rules_apply() {
    let network = network();
    // 2025-05-05 is a Monday.
    assert!(network.service_active("WKDAY", date("20250505")));
    // 2025-05-03 is a Saturday.
    assert!(!network.service_active("WKDAY", date("20250503")));
    assert!(network.service_active("WKEND", date("20250503")));
    // Outside the validity period.
    assert!(!network.service_active("WKDAY", date("20280103")));
    // Unknown services never run.
    assert!(!network.service_active("NOPE", date("20250505")));
}

#[test]
fn calendar_exceptions_override_weekly_rules() {
    let network = network();
    // 2025-05-01 is a Thursday, but the fixture marks it as a public
    // holiday: weekday service is removed, weekend service is added.
    assert!(!network.service_active("WKDAY", date("20250501")));
    assert!(network.service_active("WKEND", date("20250501")));
}

// ----------------------------------------------------------------------
// Departures
// ----------------------------------------------------------------------

fn choa_chu_kang(network: &RailNetwork) -> StationId {
    network.station_by_code("NS4").unwrap()
}

#[test]
fn departures_list_both_directions_in_time_order() {
    let network = network();
    let departures = network.departures(
        choa_chu_kang(&network),
        date("20250505"),
        time("06:00:00"),
        time("08:00:00"),
    );

    let summary: Vec<(String, String)> = departures
        .iter()
        .map(|d| (d.trip_id.clone(), d.time.to_string()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("NS_T1".to_string(), "06:10:30".to_string()),
            ("NS_T3".to_string(), "06:20:30".to_string()),
            ("NS_T2".to_string(), "07:10:30".to_string()),
        ]
    );

    let first = &departures[0];
    assert_eq!(network.station(first.terminus).name, "Marina Bay");
    assert_eq!(first.headsign.as_deref(), Some("Marina Bay"));
    assert_eq!(first.direction, Some(0));
    assert!(first.exact);
}

#[test]
fn weekend_trips_do_not_appear_on_weekdays() {
    let network = network();
    let departures = network.departures(
        choa_chu_kang(&network),
        date("20250505"),
        time("08:00:00"),
        time("09:00:00"),
    );
    assert!(departures.iter().all(|d| d.trip_id != "NS_T4"));

    let departures = network.departures(
        choa_chu_kang(&network),
        date("20250503"),
        time("08:00:00"),
        time("09:00:00"),
    );
    assert_eq!(departures.len(), 1);
    assert_eq!(departures[0].trip_id, "NS_T4");
}

#[test]
fn frequency_trips_expand_into_single_departures() {
    let network = network();
    let departures = network.departures(
        choa_chu_kang(&network),
        date("20250505"),
        time("05:00:00"),
        time("06:00:00"),
    );

    // The block runs 05:30 to 06:00 with a 600-second headway. Trips
    // start at 05:30, 05:40, and 05:50. No trip starts at 06:00.
    let times: Vec<String> = departures.iter().map(|d| d.time.to_string()).collect();
    assert_eq!(times, vec!["05:30:00", "05:40:00", "05:50:00"]);
    assert!(departures.iter().all(|d| !d.exact));
    assert!(departures.iter().all(|d| d.trip_id == "BP_T1"));
}

#[test]
fn termini_have_no_boarding_departures() {
    let network = network();
    let marina_bay = network.station_by_code("NS27").unwrap();
    let departures = network.departures(
        marina_bay,
        date("20250505"),
        time("06:00:00"),
        time("08:00:00"),
    );
    // Southbound trips end at Marina Bay. Only the northbound trip
    // departs there.
    assert_eq!(departures.len(), 1);
    assert_eq!(departures[0].trip_id, "NS_T3");
}

// ----------------------------------------------------------------------
// Destination board
// ----------------------------------------------------------------------

#[test]
fn the_board_includes_trips_from_the_previous_service_day() {
    let network = network();
    // Friday 2025-05-09, just after midnight. Trip NS_T5 started on
    // Thursday at 23:50 and calls at Choa Chu Kang at 24:05:30.
    let entries = network.departure_board(
        choa_chu_kang(&network),
        date("20250509"),
        time("00:00:00"),
        3600,
    );

    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.departure.trip_id, "NS_T5");
    assert_eq!(entry.service_date, date("20250508"));
    assert_eq!(entry.wait_secs, 330);
    assert_eq!(entry.clock_time().to_string(), "00:05:30");
}

#[test]
fn the_board_sorts_by_wait_time() {
    let network = network();
    let entries = network.departure_board(
        choa_chu_kang(&network),
        date("20250505"),
        time("05:45:00"),
        3600,
    );

    let waits: Vec<u32> = entries.iter().map(|e| e.wait_secs).collect();
    let mut sorted = waits.clone();
    sorted.sort_unstable();
    assert_eq!(waits, sorted);

    // 05:50 (BPL) and 06:10:30, 06:20:30 (NSL) are inside the hour.
    assert_eq!(entries.len(), 3);
}

// ----------------------------------------------------------------------
// Zip source
// ----------------------------------------------------------------------

#[cfg(feature = "zip-source")]
mod zip_source {
    use super::*;
    use std::io::Write as _;

    /// Pack the fixture directory into a zip archive, with an optional
    /// directory prefix for every entry.
    fn pack_fixture(prefix: &str) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        for entry in std::fs::read_dir(fixture_dir()).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            writer
                .start_file(format!("{prefix}{name}"), options)
                .unwrap();
            writer.write_all(&std::fs::read(&path).unwrap()).unwrap();
        }
        writer.finish().unwrap();
        buffer.into_inner()
    }

    #[test]
    fn feeds_load_from_zip_archives() {
        let bytes = pack_fixture("");
        let mut source = mrt_gtfs::ZipSource::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let feed = GtfsFeed::load(&mut source).unwrap();
        assert_eq!(feed.stops.len(), 15);
        assert_eq!(feed.routes.len(), 4);
    }

    #[test]
    fn feeds_load_from_zip_archives_with_a_subdirectory() {
        let bytes = pack_fixture("gtfs/");
        let mut source = mrt_gtfs::ZipSource::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let feed = GtfsFeed::load(&mut source).unwrap();
        assert_eq!(feed.stops.len(), 15);
    }

    #[test]
    fn zip_files_load_from_a_path() {
        let bytes = pack_fixture("");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mini.gtfs.zip");
        std::fs::write(&path, bytes).unwrap();
        let feed = GtfsFeed::from_zip_path(&path).unwrap();
        assert_eq!(feed.trips.len(), 8);
    }
}

#[test]
fn split_route_entries_do_not_make_an_interchange() {
    // The official LTA feed splits the Circle Line into several route
    // entries with the same display name. A station that only those
    // entries serve is not an interchange.
    use mrt_gtfs::{Calendar, Route, Stop, StopTime, Trip};

    fn route(id: &str, name: &str) -> Route {
        Route {
            route_id: id.to_string(),
            agency_id: None,
            route_short_name: Some(name.to_string()),
            route_long_name: None,
            route_type: 1,
            route_color: None,
            route_text_color: None,
        }
    }
    fn trip(route: &str, id: &str) -> Trip {
        Trip {
            route_id: route.to_string(),
            service_id: "DAILY".to_string(),
            trip_id: id.to_string(),
            trip_headsign: None,
            direction_id: Some(0),
            shape_id: None,
        }
    }
    fn call(trip: &str, time: &str, stop: &str, seq: u32) -> StopTime {
        StopTime {
            trip_id: trip.to_string(),
            arrival_time: Some(time.parse().unwrap()),
            departure_time: Some(time.parse().unwrap()),
            stop_id: stop.to_string(),
            stop_sequence: seq,
            stop_headsign: None,
            pickup_type: None,
            drop_off_type: None,
        }
    }

    let feed = GtfsFeed {
        stops: vec![
            Stop {
                stop_id: "A".to_string(),
                stop_name: Some("Alpha".to_string()),
                ..Default::default()
            },
            Stop {
                stop_id: "B".to_string(),
                stop_name: Some("Beta".to_string()),
                ..Default::default()
            },
            Stop {
                stop_id: "C".to_string(),
                stop_name: Some("Gamma".to_string()),
                ..Default::default()
            },
        ],
        routes: vec![
            route("CC_a", "CC"),
            route("CC_b", "CC"),
            route("NS_1", "NS"),
        ],
        trips: vec![trip("CC_a", "T1"), trip("CC_b", "T2"), trip("NS_1", "T3")],
        stop_times: vec![
            // Both CC variants and the NS route call at Alpha. Only
            // the CC variants call at Beta.
            call("T1", "08:00:00", "A", 1),
            call("T1", "08:05:00", "B", 2),
            call("T2", "09:00:00", "A", 1),
            call("T2", "09:05:00", "B", 2),
            call("T3", "08:00:00", "A", 1),
            call("T3", "08:03:00", "C", 2),
        ],
        calendar: vec![Calendar {
            service_id: "DAILY".to_string(),
            monday: 1,
            tuesday: 1,
            wednesday: 1,
            thursday: 1,
            friday: 1,
            saturday: 1,
            sunday: 1,
            start_date: "20250101".parse().unwrap(),
            end_date: "20271231".parse().unwrap(),
        }],
        ..Default::default()
    };
    let network = RailNetwork::from_feed(&feed).unwrap();

    let alpha = network.station_by_name("Alpha").unwrap();
    let beta = network.station_by_name("Beta").unwrap();

    // Alpha: CC and NS -> interchange. Beta: CC only -> not one,
    // although two route entries serve it.
    assert!(network.is_interchange(alpha));
    assert!(!network.is_interchange(beta));
    assert!(network.station(beta).is_interchange()); // raw entry count
    let ids: Vec<_> = network.interchanges().collect();
    assert_eq!(ids, vec![alpha]);
}
