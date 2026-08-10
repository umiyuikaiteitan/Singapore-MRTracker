//! Load a GTFS feed into raw record tables.

use serde::de::DeserializeOwned;

use crate::error::GtfsError;
use crate::model::{
    Agency, Calendar, CalendarDate, Frequency, Route, ShapePoint, Stop, StopTime, Transfer, Trip,
};
use crate::source::{strip_bom, FeedSource};

/// A GTFS feed as raw record tables.
///
/// [`GtfsFeed::load`] fills the tables from a [`FeedSource`]. All
/// fields are public, so tests and tools can also construct a feed
/// directly.
///
/// # Examples
///
/// ```no_run
/// use mrt_gtfs::GtfsFeed;
///
/// let feed = GtfsFeed::from_zip_path("data/singapore-gtfs.zip").unwrap();
/// println!("The feed has {} routes.", feed.routes.len());
/// ```
#[derive(Debug, Clone, Default)]
pub struct GtfsFeed {
    /// The records of `agency.txt`.
    pub agencies: Vec<Agency>,
    /// The records of `stops.txt`.
    pub stops: Vec<Stop>,
    /// The records of `routes.txt`.
    pub routes: Vec<Route>,
    /// The records of `trips.txt`.
    pub trips: Vec<Trip>,
    /// The records of `stop_times.txt`.
    pub stop_times: Vec<StopTime>,
    /// The records of `calendar.txt`.
    pub calendar: Vec<Calendar>,
    /// The records of `calendar_dates.txt`.
    pub calendar_dates: Vec<CalendarDate>,
    /// The records of `frequencies.txt`.
    pub frequencies: Vec<Frequency>,
    /// The records of `transfers.txt`.
    pub transfers: Vec<Transfer>,
    /// The records of `shapes.txt`.
    pub shapes: Vec<ShapePoint>,
}

impl GtfsFeed {
    /// Load a feed from a [`FeedSource`].
    ///
    /// The function requires `stops.txt`, `routes.txt`, `trips.txt`,
    /// and `stop_times.txt`. The other tables are optional. The feed
    /// must contain `calendar.txt`, `calendar_dates.txt`, or both.
    pub fn load<S: FeedSource>(source: &mut S) -> Result<Self, GtfsError> {
        let feed = GtfsFeed {
            agencies: read_table(source, "agency.txt", false)?,
            stops: read_table(source, "stops.txt", true)?,
            routes: read_table(source, "routes.txt", true)?,
            trips: read_table(source, "trips.txt", true)?,
            stop_times: read_table(source, "stop_times.txt", true)?,
            calendar: read_table(source, "calendar.txt", false)?,
            calendar_dates: read_table(source, "calendar_dates.txt", false)?,
            frequencies: read_table(source, "frequencies.txt", false)?,
            transfers: read_table(source, "transfers.txt", false)?,
            shapes: read_table(source, "shapes.txt", false)?,
        };
        if feed.calendar.is_empty() && feed.calendar_dates.is_empty() {
            return Err(GtfsError::NoCalendar);
        }
        Ok(feed)
    }

    /// Load a feed from a directory that contains the feed files.
    pub fn from_dir(path: impl Into<std::path::PathBuf>) -> Result<Self, GtfsError> {
        let mut source = crate::source::DirectorySource::new(path);
        Self::load(&mut source)
    }

    /// Load a feed from a GTFS zip archive.
    #[cfg(feature = "zip-source")]
    pub fn from_zip_path(path: impl AsRef<std::path::Path>) -> Result<Self, GtfsError> {
        let mut source = crate::source::ZipSource::from_path(path)?;
        Self::load(&mut source)
    }
}

/// Read one feed table into records.
///
/// Return an empty table if the file is optional and not in the feed.
fn read_table<S, T>(source: &mut S, name: &str, required: bool) -> Result<Vec<T>, GtfsError>
where
    S: FeedSource,
    T: DeserializeOwned,
{
    let Some(reader) = source.open(name)? else {
        if required {
            return Err(GtfsError::MissingFile(name.to_string()));
        }
        return Ok(Vec::new());
    };
    let reader = strip_bom(reader).map_err(|e| GtfsError::io(name, e))?;
    let mut csv_reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(reader);
    let mut records = Vec::new();
    for row in csv_reader.deserialize() {
        let record: T = row.map_err(|e| GtfsError::parse(name, e))?;
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Read;

    /// A feed source backed by in-memory strings.
    struct MemorySource(HashMap<&'static str, &'static str>);

    impl FeedSource for MemorySource {
        fn open<'a>(&'a mut self, name: &str) -> Result<Option<Box<dyn Read + 'a>>, GtfsError> {
            Ok(self
                .0
                .get(name)
                .map(|data| Box::new(data.as_bytes()) as Box<dyn Read>))
        }
    }

    fn minimal_source() -> MemorySource {
        let mut files = HashMap::new();
        files.insert("stops.txt", "stop_id,stop_name\nS1,Alpha\n");
        files.insert(
            "routes.txt",
            "route_id,route_short_name,route_type\nR1,NS,1\n",
        );
        files.insert("trips.txt", "route_id,service_id,trip_id\nR1,WK,T1\n");
        files.insert(
            "stop_times.txt",
            "trip_id,arrival_time,departure_time,stop_id,stop_sequence\nT1,06:00:00,06:00:30,S1,1\n",
        );
        files.insert(
            "calendar.txt",
            "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\n\
             WK,1,1,1,1,1,0,0,20250101,20271231\n",
        );
        MemorySource(files)
    }

    #[test]
    fn load_minimal_feed() {
        let feed = GtfsFeed::load(&mut minimal_source()).unwrap();
        assert_eq!(feed.stops.len(), 1);
        assert_eq!(feed.routes.len(), 1);
        assert_eq!(feed.trips.len(), 1);
        assert_eq!(feed.stop_times.len(), 1);
        assert_eq!(feed.calendar.len(), 1);
        assert!(feed.frequencies.is_empty());
    }

    #[test]
    fn missing_required_file_is_an_error() {
        let mut source = minimal_source();
        source.0.remove("stop_times.txt");
        let err = GtfsFeed::load(&mut source).unwrap_err();
        assert!(matches!(err, GtfsError::MissingFile(name) if name == "stop_times.txt"));
    }

    #[test]
    fn missing_calendar_data_is_an_error() {
        let mut source = minimal_source();
        source.0.remove("calendar.txt");
        let err = GtfsFeed::load(&mut source).unwrap_err();
        assert!(matches!(err, GtfsError::NoCalendar));
    }

    #[test]
    fn calendar_dates_alone_satisfy_the_calendar_rule() {
        let mut source = minimal_source();
        source.0.remove("calendar.txt");
        source.0.insert(
            "calendar_dates.txt",
            "service_id,date,exception_type\nWK,20250501,1\n",
        );
        let feed = GtfsFeed::load(&mut source).unwrap();
        assert_eq!(feed.calendar_dates.len(), 1);
    }

    #[test]
    fn parse_error_names_the_file() {
        let mut source = minimal_source();
        source.0.insert(
            "stop_times.txt",
            "trip_id,arrival_time,departure_time,stop_id,stop_sequence\nT1,not-a-time,06:00:30,S1,1\n",
        );
        let err = GtfsFeed::load(&mut source).unwrap_err();
        assert!(matches!(err, GtfsError::Parse { file, .. } if file == "stop_times.txt"));
    }

    #[test]
    fn bom_and_optional_empty_fields_parse() {
        let mut source = minimal_source();
        source.0.insert(
            "stops.txt",
            "\u{FEFF}stop_id,stop_code,stop_name,stop_lat,stop_lon,location_type,parent_station\n\
             S1,NS1,Alpha,1.333,103.742,0,\n",
        );
        let feed = GtfsFeed::load(&mut source).unwrap();
        let stop = &feed.stops[0];
        assert_eq!(stop.stop_id, "S1");
        assert_eq!(stop.stop_code.as_deref(), Some("NS1"));
        assert_eq!(stop.stop_lat, Some(1.333));
        assert_eq!(stop.parent_station_id(), None);
    }
}
