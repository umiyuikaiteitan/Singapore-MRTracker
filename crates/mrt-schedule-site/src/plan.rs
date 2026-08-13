//! What the site contains.
//!
//! A plan is the list of pages to generate: which service dates,
//! which stations, which lines, and which diagram windows. It is
//! computed before anything is rendered, so the hub, the navigation
//! blocks, and the files agree on one set of names.
//!
//! Every path in a plan is relative. A GitHub Pages project site
//! lives under `/<repository>/`, and a site that hard-codes an
//! absolute path breaks there.

use mrt_gtfs::{GtfsTime, LineId, RailNetwork, ServiceDate, StationId, Weekday};
use serde::Serialize;

/// One service date that the site covers.
#[derive(Clone, Debug, Serialize)]
pub struct DateEntry {
    /// The GTFS service date.
    pub date: ServiceDate,
    /// `20260810`, for a file name.
    pub key: String,
    /// `2026-08-10`, for a heading.
    pub iso: String,
    /// `Mon 10 Aug`, for a link title.
    pub short: String,
    /// `10 Aug`, for the second line of a date tab, where the first
    /// line already carries the weekday.
    pub day_month: String,
    /// `Today`, `Tomorrow`, or the weekday name.
    pub relation: String,
}

/// One station that the site covers.
#[derive(Clone, Debug, Serialize)]
pub struct StationEntry {
    /// The station in the network model.
    #[serde(skip)]
    pub id: StationId,
    /// The file-name key, from the first code, for example `ns1`.
    pub key: String,
    /// The public name.
    pub name: String,
    /// Every public code of the station.
    pub codes: Vec<String>,
    /// The lines that serve it, as route identifiers.
    pub lines: Vec<String>,
    /// A lower-case search key: the name and every code, without
    /// punctuation.
    pub search: String,
}

/// One line that the site covers.
#[derive(Clone, Debug, Serialize)]
pub struct LineEntry {
    /// The line in the network model.
    #[serde(skip)]
    pub id: LineId,
    /// The file-name key, from the route identifier.
    pub key: String,
    /// The GTFS route identifier.
    pub route_id: String,
    /// The display name, for example `NSL`.
    pub name: String,
    /// The long name, when the feed carries one.
    pub long_name: Option<String>,
    /// The line colour as a CSS colour, when the feed carries one.
    pub color: Option<String>,
    /// How many stations the line serves.
    pub station_count: usize,
}

/// One diagram time window.
#[derive(Clone, Debug, Serialize)]
pub struct WindowEntry {
    /// The file-name key, for example `morning`.
    pub key: String,
    /// The heading, for example `Morning 05:00-10:00`.
    pub label: String,
    /// The start of the window.
    #[serde(serialize_with = "as_string")]
    pub from: GtfsTime,
    /// The exclusive end of the window.
    #[serde(serialize_with = "as_string")]
    pub until: GtfsTime,
}

fn as_string<S: serde::Serializer>(value: &GtfsTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&value.to_string())
}

/// The default diagram windows.
///
/// A whole service day on one time axis is unreadable at any page
/// size, so the site cuts the day into the four periods that a
/// planner actually compares.
pub fn default_windows() -> Vec<WindowEntry> {
    vec![
        window("morning", "Morning", 5, 10),
        window("midday", "Midday", 10, 16),
        window("evening", "Evening", 16, 21),
        window("night", "Night", 21, 28),
    ]
}

fn window(key: &str, name: &str, from_hour: u32, until_hour: u32) -> WindowEntry {
    WindowEntry {
        key: key.to_string(),
        label: format!("{name} {from_hour:02}:00\u{2013}{:02}:00", until_hour % 24),
        from: GtfsTime::from_hms(from_hour, 0, 0),
        until: GtfsTime::from_hms(until_hour, 0, 0),
    }
}

/// Everything the site will contain.
#[derive(Clone, Debug, Serialize)]
pub struct SitePlan {
    /// The service dates, in order, starting with today.
    pub dates: Vec<DateEntry>,
    /// The stations, sorted by name.
    pub stations: Vec<StationEntry>,
    /// The lines, in feed order.
    pub lines: Vec<LineEntry>,
    /// The diagram windows.
    pub windows: Vec<WindowEntry>,
}

impl SitePlan {
    /// Build the plan for a network.
    ///
    /// `today` is the first service date; `days` is how many
    /// consecutive dates to cover. A station without a public code is
    /// left out, because it has no stable name for a URL and no code
    /// for a reader to search by.
    pub fn build(
        network: &RailNetwork,
        today: ServiceDate,
        days: u32,
        windows: Vec<WindowEntry>,
    ) -> Self {
        let dates = (0..days.max(1) as i64)
            .map(|offset| date_entry(today.plus_days(offset), offset))
            .collect();

        let mut stations: Vec<StationEntry> = network
            .stations()
            .iter()
            .enumerate()
            .filter(|(_, station)| !station.codes.is_empty())
            .map(|(index, station)| {
                let id = StationId(index);
                let mut lines: Vec<String> = station
                    .lines
                    .iter()
                    .map(|&line| network.line(line).route_id.clone())
                    .collect();
                lines.dedup();
                StationEntry {
                    id,
                    key: mrt_publication::css_key(&station.codes[0]),
                    name: station.name.clone(),
                    codes: station.codes.clone(),
                    lines,
                    search: search_key(&station.name, &station.codes),
                }
            })
            .collect();
        stations.sort_by(|a, b| {
            (a.name.as_str(), a.key.as_str()).cmp(&(b.name.as_str(), b.key.as_str()))
        });
        // Two stations can share a name, and a key must still name one
        // file. The code is the tie-breaker, and codes are unique.
        stations.dedup_by(|a, b| a.key == b.key);

        let lines = network
            .lines()
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let id = LineId(index);
                LineEntry {
                    id,
                    key: mrt_publication::css_key(&line.route_id),
                    route_id: line.route_id.clone(),
                    name: line.name.clone(),
                    long_name: line.long_name.clone(),
                    color: Some(mrt_publication::css_color(
                        line.color.as_deref().unwrap_or_default(),
                    ))
                    .filter(|c| !c.is_empty()),
                    station_count: network
                        .stations()
                        .iter()
                        .filter(|station| station.lines.contains(&id))
                        .count(),
                }
            })
            .collect();

        SitePlan {
            dates,
            stations,
            lines,
            windows,
        }
    }

    /// Get the relative path of a station timetable, from the hub.
    pub fn timetable_path(&self, station: &StationEntry, date: &DateEntry) -> String {
        format!("t/{}-{}.html", station.key, date.key)
    }

    /// Get the relative path of a line diagram, from the hub.
    pub fn diagram_path(&self, line: &LineEntry, date: &DateEntry, window: &WindowEntry) -> String {
        format!("d/{}-{}-{}.html", line.key, date.key, window.key)
    }

    /// Get the relative path of a standalone drawing, from the hub.
    pub fn drawing_path(&self, line: &LineEntry, date: &DateEntry, window: &WindowEntry) -> String {
        format!("d/{}-{}-{}.svg", line.key, date.key, window.key)
    }

    /// Count the pages that the plan will produce.
    pub fn page_count(&self) -> usize {
        self.stations.len() * self.dates.len()
            + self.lines.len() * self.dates.len() * self.windows.len()
    }

    /// Get the first date, which the hub opens on.
    pub fn first_date(&self) -> &DateEntry {
        &self.dates[0]
    }
}

fn date_entry(date: ServiceDate, offset: i64) -> DateEntry {
    let weekday = match date.weekday() {
        Weekday::Monday => "Mon",
        Weekday::Tuesday => "Tue",
        Weekday::Wednesday => "Wed",
        Weekday::Thursday => "Thu",
        Weekday::Friday => "Fri",
        Weekday::Saturday => "Sat",
        Weekday::Sunday => "Sun",
    };
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = MONTHS[(date.month() as usize).clamp(1, 12) - 1];
    DateEntry {
        key: date.to_string(),
        iso: format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day()),
        short: format!("{weekday} {} {month}", date.day()),
        day_month: format!("{} {month}", date.day()),
        relation: match offset {
            0 => "Today".to_string(),
            1 => "Tomorrow".to_string(),
            _ => weekday.to_string(),
        },
        date,
    }
}

/// Build the lower-case, punctuation-free search key of a station.
fn search_key(name: &str, codes: &[String]) -> String {
    let mut parts = vec![squash(name)];
    parts.extend(codes.iter().map(|code| squash(code)));
    parts.join(" ")
}

fn squash(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_run_forward_from_today_and_name_themselves() {
        let today: ServiceDate = "20260810".parse().unwrap(); // a Monday
        let entries: Vec<DateEntry> = (0..4).map(|i| date_entry(today.plus_days(i), i)).collect();
        assert_eq!(entries[0].relation, "Today");
        assert_eq!(entries[1].relation, "Tomorrow");
        assert_eq!(entries[2].relation, "Wed");
        assert_eq!(entries[0].short, "Mon 10 Aug");
        assert_eq!(entries[0].day_month, "10 Aug");
        assert_eq!(entries[0].key, "20260810");
        assert_eq!(entries[0].iso, "2026-08-10");
        assert_eq!(entries[3].key, "20260813");
    }

    #[test]
    fn a_search_key_holds_the_name_and_every_code() {
        let key = search_key("Jurong East", &["NS1".into(), "EW24".into()]);
        assert_eq!(key, "jurong east ns1 ew24");
        // Punctuation and case never reach the key.
        assert_eq!(squash("Bras Basah-Bugis"), "bras basahbugis");
        assert_eq!(squash("TE1"), "te1");
    }

    #[test]
    fn the_default_windows_cover_the_whole_service_day() {
        let windows = default_windows();
        assert_eq!(windows.len(), 4);
        assert_eq!(windows[0].from.to_string(), "05:00:00");
        assert_eq!(windows[3].until.to_string(), "28:00:00");
        // They join end to end, so no service falls between two of
        // them.
        for pair in windows.windows(2) {
            assert_eq!(pair[0].until, pair[1].from);
        }
        assert_eq!(windows[3].label, "Night 21:00\u{2013}04:00");
    }

    #[test]
    fn paths_stay_relative() {
        let plan = SitePlan {
            dates: vec![date_entry("20260810".parse().unwrap(), 0)],
            stations: vec![StationEntry {
                id: StationId(0),
                key: "ns1".into(),
                name: "Jurong East".into(),
                codes: vec!["NS1".into()],
                lines: vec!["NS".into()],
                search: "jurong east ns1".into(),
            }],
            lines: vec![LineEntry {
                id: LineId(0),
                key: "ns".into(),
                route_id: "NS".into(),
                name: "NSL".into(),
                long_name: None,
                color: None,
                station_count: 1,
            }],
            windows: default_windows(),
        };
        let path = plan.timetable_path(&plan.stations[0], &plan.dates[0]);
        assert_eq!(path, "t/ns1-20260810.html");
        assert!(!path.starts_with('/'));
        assert_eq!(
            plan.diagram_path(&plan.lines[0], &plan.dates[0], &plan.windows[0]),
            "d/ns-20260810-morning.html"
        );
        assert_eq!(plan.page_count(), 1 + 4);
    }
}
