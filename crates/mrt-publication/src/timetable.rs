//! The station departure timetable projection.
//!
//! [`build_timetable`] turns the trips of one service date into a
//! Japanese-style 発車時刻表: one panel per line, platform, and
//! direction, each panel a table of hour rows with the departure
//! minutes beside them.
//!
//! # Rules that the projection keeps
//!
//! - Hours follow the *service day*, so a day that starts at `04:00`
//!   ends with the rows `00`, `01`, `02`, `03`, which are the small
//!   hours of the next calendar day.
//! - A departure appears exactly once. A call enters the timetable
//!   only when it permits boarding, carries a usable time, and has a
//!   later call after it.
//! - Destination text comes from `stop_headsign`, then
//!   `trip_headsign`, then the last station of the run, then the
//!   configuration. Nothing is guessed.
//! - Platform text comes from the platform that the run actually
//!   uses, or from an explicit configuration override.
//! - Headway service that GTFS marks `exact_times=0` becomes a band
//!   row such as `06:30–09:00  every 10 min approximately`, never a
//!   list of invented minutes.

use std::collections::BTreeMap;

use mrt_gtfs::{
    Diagnostic, FrequencyPolicy, GtfsTime, LineId, RailNetwork, ScheduledCall, ServiceDate,
    StationId, TimeExactness, TimeQuality, TripInstance, TripInstanceQuery,
};
use serde::Serialize;

use crate::common::{
    DepartureFlag, DocumentSeed, LegendItem, LineView, PublicationMetadata, StationView,
};
use crate::config::{ColumnLayout, PublicationConfig, SecondsDisplay};
use crate::error::PublicationError;
use crate::text::{Labels, LocalizedText};

/// A finished timetable document.
#[derive(Clone, Debug, Serialize)]
pub struct TimetableDocument {
    /// Where the data came from.
    pub metadata: PublicationMetadata,
    /// The station of the timetable.
    pub station: StationView,
    /// The page title.
    pub title: LocalizedText,
    /// The service-day label, for example `2026-08-10 (Mon)`.
    pub service_day_label: String,
    /// The first hour of the service day, as service-day seconds.
    pub day_start: GtfsTime,
    /// The exclusive end of the service day.
    pub day_end: GtfsTime,
    /// One panel per line, platform, and direction.
    pub panels: Vec<TimetablePanel>,
    /// The legend entries that the panels use.
    pub legend: Vec<LegendItem>,
}

impl TimetableDocument {
    /// Count the departures in the whole document.
    pub fn departure_count(&self) -> usize {
        self.panels
            .iter()
            .flat_map(|p| p.hour_groups.iter())
            .map(|g| g.departures.len())
            .sum()
    }
}

/// One direction panel of a timetable.
#[derive(Clone, Debug, Serialize)]
pub struct TimetablePanel {
    /// A stable key for the panel, usable in HTML identifiers.
    pub key: String,
    /// The line of the panel.
    pub line: LineView,
    /// The heading that names the direction.
    pub direction_label: String,
    /// The GTFS direction of the panel.
    pub direction: Option<u8>,
    /// The platform heading, when the feed names a platform.
    pub platform_label: Option<String>,
    /// Every destination that the panel carries, most frequent first.
    pub destination_summary: Vec<String>,
    /// The hour rows, in service-day order.
    pub hour_groups: Vec<HourGroup>,
    /// The indexes of the hour rows at which a new column starts.
    pub column_breaks: Vec<usize>,
    /// The headway bands that affect this panel.
    pub frequency_notes: Vec<FrequencyNote>,
}

impl TimetablePanel {
    /// Count the departures of the panel.
    pub fn departure_count(&self) -> usize {
        self.hour_groups.iter().map(|g| g.departures.len()).sum()
    }

    /// Split the hour rows into columns, using the column breaks.
    pub fn columns(&self) -> Vec<&[HourGroup]> {
        let mut out = Vec::new();
        let mut start = 0usize;
        for &break_at in &self.column_breaks {
            if break_at > start && break_at <= self.hour_groups.len() {
                out.push(&self.hour_groups[start..break_at]);
                start = break_at;
            }
        }
        if start < self.hour_groups.len() {
            out.push(&self.hour_groups[start..]);
        }
        if out.is_empty() {
            out.push(&self.hour_groups[..]);
        }
        out
    }
}

/// One hour row of a panel.
#[derive(Clone, Debug, Serialize)]
pub struct HourGroup {
    /// The hour on the service day. Values of 24 and more are the
    /// small hours of the next calendar day.
    pub service_hour: u32,
    /// The hour on the 24-hour clock, for display.
    pub display_hour: u8,
    /// The departures of the hour, in time order.
    pub departures: Vec<TimetableDeparture>,
}

/// One departure of a timetable.
#[derive(Clone, Debug, Serialize)]
pub struct TimetableDeparture {
    /// The scheduled time on the service day.
    pub scheduled_time: GtfsTime,
    /// The minute, for the large numeral.
    pub display_minute: u8,
    /// The seconds, when the configuration shows them.
    pub display_seconds: Option<u8>,
    /// The destination, possibly abbreviated.
    pub destination: String,
    /// The full destination, for a screen reader and a tooltip.
    pub destination_full: String,
    /// The platform of this departure.
    pub platform: Option<String>,
    /// The public train name, when the feed supplies one.
    pub trip_short_name: Option<String>,
    /// The internal GTFS trip identifier. Never a passenger-facing
    /// train number; renderers keep it out of the printed page.
    pub source_trip_id: String,
    /// The identifier of the run.
    pub instance_id: String,
    /// Whether the time is exact.
    pub exactness: TimeExactness,
    /// The marks on this departure.
    pub flags: Vec<DepartureFlag>,
}

/// A headway band, as a timetable row.
#[derive(Clone, Debug, Serialize)]
pub struct FrequencyNote {
    /// The identifier of the band.
    pub band_id: String,
    /// The first departure of the band.
    pub start: GtfsTime,
    /// The end of the band.
    pub end: GtfsTime,
    /// The headway in whole minutes.
    pub headway_minutes: u32,
    /// The destination of the band.
    pub destination: String,
    /// The ready-made text, for example
    /// `06:30–09:00  every 10 min approximately`.
    pub text: String,
    /// The service hours that the band covers.
    pub service_hours: Vec<u32>,
}

/// The grouping key of a panel.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PanelKey {
    line: usize,
    platform: Option<String>,
    direction_order: u8,
    direction: Option<u8>,
    destination: Option<String>,
}

/// Build a station timetable.
///
/// `line_filter` keeps only the trips of one line. The window comes
/// from [`PublicationConfig::day_start`] and
/// [`PublicationConfig::day_duration_hours`].
pub fn build_timetable(
    network: &RailNetwork,
    station: StationId,
    service_date: ServiceDate,
    line_filter: Option<LineId>,
    config: &PublicationConfig,
    seed: &DocumentSeed,
) -> Result<TimetableDocument, PublicationError> {
    config.check().map_err(PublicationError::Configuration)?;
    let labels = Labels::for_language(config.language);
    let day_start = config.day_start;
    let day_end = config.day_end();

    let mut query = TripInstanceQuery::new(service_date)
        .station(station)
        .window(day_start, day_end)
        .frequency_policy(config.frequency_policy)
        .missing_time_policy(config.missing_time_policy);
    if let Some(line) = line_filter {
        query = query.line(line);
    }
    let result = network.query_trip_instances(&query)?;
    let mut diagnostics: Vec<Diagnostic> = result.diagnostics;

    // Step 1: collect the departures into panel groups.
    let mut groups: BTreeMap<PanelKey, PanelBuilder> = BTreeMap::new();
    for trip in &result.trips {
        for (index, call) in trip.calls.iter().enumerate() {
            if call.station != station {
                continue;
            }
            let Some(departure) = boardable_departure(trip, index, call) else {
                continue;
            };
            if departure < day_start || departure >= day_end {
                continue;
            }
            let destination = resolve_destination(network, trip, index, call, config, labels);
            let platform = resolve_platform(call, config);
            let key = PanelKey {
                line: trip.line.0,
                platform: if config.timetable.group_by_platform {
                    platform.clone()
                } else {
                    None
                },
                direction_order: trip.direction.unwrap_or(u8::MAX),
                direction: trip.direction,
                destination: if config.timetable.split_by_destination {
                    Some(destination.clone())
                } else {
                    None
                },
            };
            let entry = groups.entry(key).or_insert_with(|| PanelBuilder {
                line: trip.line,
                direction: trip.direction,
                platform: platform.clone(),
                departures: Vec::new(),
                bands: Vec::new(),
            });
            let mut flags = Vec::new();
            if trip.exactness == TimeExactness::Approximate {
                flags.push(DepartureFlag::Approximate);
            }
            if call.time_quality == TimeQuality::Interpolated
                || call.time_quality == TimeQuality::Approximate
            {
                flags.push(DepartureFlag::Interpolated);
            }
            if departure.hours() >= 24 {
                flags.push(DepartureFlag::PastMidnight);
            }
            entry.departures.push(TimetableDeparture {
                scheduled_time: departure,
                display_minute: departure.minutes() as u8,
                display_seconds: seconds_for(departure, config.timetable.seconds),
                destination: config.labels.abbreviate(&destination).to_string(),
                destination_full: destination,
                platform,
                trip_short_name: if config.timetable.show_trip_short_name {
                    trip.short_name.clone()
                } else {
                    None
                },
                source_trip_id: trip.source_trip_id.clone(),
                instance_id: trip.instance_id.clone(),
                exactness: trip.exactness,
                flags,
            });
        }
    }

    // Step 2: attach the headway bands to their panels.
    for band in &result.frequency_bands {
        let Some((index, call)) = band
            .template
            .iter()
            .enumerate()
            .find(|(i, c)| c.station == station && *i + 1 < band.template.len())
        else {
            continue;
        };
        if !call.allows_pickup() {
            continue;
        }
        let destination = band
            .headsign
            .clone()
            .or_else(|| call.stop_headsign.clone())
            .unwrap_or_else(|| {
                band.template
                    .last()
                    .map(|c| network.station(c.station).name.clone())
                    .unwrap_or_default()
            });
        let platform = resolve_platform(call, config);
        let offset = call
            .departure_or_arrival()
            .zip(band.template.first().and_then(|c| c.arrival_or_departure()))
            .map(|(here, first)| here.seconds().saturating_sub(first.seconds()))
            .unwrap_or(0);
        let start = band.start.plus_seconds(offset);
        let end = band.end.plus_seconds(offset);
        let key = PanelKey {
            line: band.line.0,
            platform: if config.timetable.group_by_platform {
                platform.clone()
            } else {
                None
            },
            direction_order: band.direction.unwrap_or(u8::MAX),
            direction: band.direction,
            destination: if config.timetable.split_by_destination {
                Some(destination.clone())
            } else {
                None
            },
        };
        let entry = groups.entry(key).or_insert_with(|| PanelBuilder {
            line: band.line,
            direction: band.direction,
            platform: platform.clone(),
            departures: Vec::new(),
            bands: Vec::new(),
        });
        let mut hours: Vec<u32> = (start.hours()..=end.hours()).collect();
        hours.retain(|h| *h >= day_start.hours() && *h < day_end.hours());
        entry.bands.push(FrequencyNote {
            band_id: band.band_id.clone(),
            start,
            end,
            headway_minutes: band.headway_minutes(),
            destination: config.labels.abbreviate(&destination).to_string(),
            text: labels.headway_band(
                &crate::common::service_hhmm(start),
                &crate::common::service_hhmm(end),
                band.headway_minutes(),
            ),
            service_hours: hours,
        });
        let _ = index;
    }

    if groups.is_empty() {
        diagnostics.push(Diagnostic::warning(
            "timetable-empty",
            format!(
                "no boardable departure at {} on {service_date} between {day_start} and {day_end}",
                network.station(station).name
            ),
        ));
    }
    if config.frequency_policy == FrequencyPolicy::ExpandApproximate {
        diagnostics.push(Diagnostic::info(
            "timetable-approximate-expansion",
            "the frequency policy expanded headway service into approximate departures; \
             every such departure carries the approximation mark",
        ));
    }

    // Step 3: turn the groups into panels.
    let hours: Vec<u32> = (day_start.hours()..day_end.hours()).collect();
    let mut panels: Vec<TimetablePanel> = Vec::new();
    for (key, mut builder) in groups {
        builder.departures.sort_by(|a, b| {
            (
                a.scheduled_time,
                a.destination_full.as_str(),
                a.instance_id.as_str(),
            )
                .cmp(&(
                    b.scheduled_time,
                    b.destination_full.as_str(),
                    b.instance_id.as_str(),
                ))
        });
        builder.bands.sort_by(|a, b| a.start.cmp(&b.start));
        if config.timetable.mark_first_and_last {
            if let Some(first) = builder.departures.first_mut() {
                first.flags.push(DepartureFlag::FirstOfDay);
            }
            if let Some(last) = builder.departures.last_mut() {
                last.flags.push(DepartureFlag::LastOfDay);
            }
        }
        for departure in &mut builder.departures {
            departure.flags.sort_unstable();
            departure.flags.dedup();
        }

        let line = LineView::of(network, builder.line);
        let destination_summary = summarize_destinations(&builder.departures, &builder.bands);
        let direction_label = direction_heading(
            &line,
            builder.direction,
            &destination_summary,
            config,
            labels,
        );

        let mut hour_groups: Vec<HourGroup> = hours
            .iter()
            .map(|&hour| HourGroup {
                service_hour: hour,
                display_hour: (hour % 24) as u8,
                departures: builder
                    .departures
                    .iter()
                    .filter(|d| d.scheduled_time.hours() == hour)
                    .cloned()
                    .collect(),
            })
            .collect();
        if !config.timetable.show_empty_hours {
            hour_groups.retain(|g| {
                !g.departures.is_empty()
                    || builder
                        .bands
                        .iter()
                        .any(|b| b.service_hours.contains(&g.service_hour))
            });
        }
        let column_breaks = column_breaks(&hour_groups, config);
        panels.push(TimetablePanel {
            key: panel_key(&line, &key),
            line,
            direction_label,
            direction: builder.direction,
            platform_label: builder
                .platform
                .as_deref()
                .map(|code| labels.platform_label(code)),
            destination_summary,
            hour_groups,
            column_breaks,
            frequency_notes: builder.bands,
        });
    }

    let legend = build_legend(&panels, labels);
    let station_view = StationView::of(network, station);
    let line_name = line_filter
        .map(|id| network.line(id).name.clone())
        .unwrap_or_default();
    let title = config.timetable.title.fill(&[
        ("station", station_view.name.as_str()),
        ("line", line_name.as_str()),
        ("date", &labels.service_date_text(service_date)),
    ]);

    mrt_gtfs::normalize_diagnostics(&mut diagnostics);
    Ok(TimetableDocument {
        metadata: PublicationMetadata::new(seed, service_date, diagnostics),
        station: station_view,
        title,
        service_day_label: labels.service_date_text(service_date),
        day_start,
        day_end,
        panels,
        legend,
    })
}

/// The departures and bands of one panel while it is being built.
struct PanelBuilder {
    line: LineId,
    direction: Option<u8>,
    platform: Option<String>,
    departures: Vec<TimetableDeparture>,
    bands: Vec<FrequencyNote>,
}

/// Get the departure time of a call that a passenger can board.
///
/// A call qualifies when boarding is permitted, a later call follows
/// it, and it carries a departure time or a usable arrival time.
fn boardable_departure(
    trip: &TripInstance,
    index: usize,
    call: &ScheduledCall,
) -> Option<GtfsTime> {
    if index + 1 >= trip.calls.len() {
        return None;
    }
    if !call.allows_pickup() {
        return None;
    }
    call.departure_or_arrival()
}

/// Resolve the destination text of one departure.
///
/// The order follows the specification: the per-call headsign, the
/// trip headsign, the last station of the run, the configured
/// direction override, and finally a neutral direction number.
fn resolve_destination(
    network: &RailNetwork,
    trip: &TripInstance,
    index: usize,
    call: &ScheduledCall,
    config: &PublicationConfig,
    labels: &Labels,
) -> String {
    if let Some(text) = call.stop_headsign.as_deref().filter(|s| !s.is_empty()) {
        return text.to_string();
    }
    if let Some(text) = trip.headsign.as_deref().filter(|s| !s.is_empty()) {
        return text.to_string();
    }
    if let Some(last) = trip.calls.last() {
        if index + 1 < trip.calls.len() {
            return network.station(last.station).name.clone();
        }
    }
    let route_id = &network.line(trip.line).route_id;
    if let Some(text) = config
        .labels
        .direction_override(route_id, trip.direction, config.language)
    {
        return text;
    }
    labels.direction_number(trip.direction)
}

/// Resolve the platform label of one call.
fn resolve_platform(call: &ScheduledCall, config: &PublicationConfig) -> Option<String> {
    if let Some(text) = config.labels.platform_overrides.get(&call.platform_stop_id) {
        return Some(text.clone());
    }
    call.platform_code.clone().filter(|c| !c.is_empty())
}

/// Choose the seconds to display.
fn seconds_for(time: GtfsTime, mode: SecondsDisplay) -> Option<u8> {
    let seconds = time.seconds_part() as u8;
    match mode {
        SecondsDisplay::Hide => None,
        SecondsDisplay::Show => Some(seconds),
        SecondsDisplay::ShowIfNonzero if seconds != 0 => Some(seconds),
        SecondsDisplay::ShowIfNonzero => None,
    }
}

/// List the destinations of a panel, most frequent first.
fn summarize_destinations(
    departures: &[TimetableDeparture],
    bands: &[FrequencyNote],
) -> Vec<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for departure in departures {
        *counts
            .entry(departure.destination_full.as_str())
            .or_insert(0) += 1;
    }
    for band in bands {
        counts.entry(band.destination.as_str()).or_insert(0);
    }
    let mut names: Vec<(&str, usize)> = counts.into_iter().collect();
    names.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    names
        .into_iter()
        .map(|(name, _)| name.to_string())
        .collect()
}

/// Build the heading of a direction panel.
///
/// The heading never guesses a compass bearing or a Japanese up/down
/// term. It uses the configured override, then the destinations that
/// the panel really carries, then the plain direction number.
fn direction_heading(
    line: &LineView,
    direction: Option<u8>,
    destinations: &[String],
    config: &PublicationConfig,
    labels: &Labels,
) -> String {
    if let Some(text) = config
        .labels
        .direction_override(&line.route_id, direction, config.language)
    {
        return text;
    }
    match destinations.len() {
        0 => labels.direction_number(direction),
        1 => labels.towards(&destinations[0]),
        2 | 3 => labels.towards(&destinations.join(" / ")),
        _ => labels.towards(&format!("{} \u{2026}", destinations[..3].join(" / "))),
    }
}

/// Choose the hour rows at which a new column starts.
fn column_breaks(hour_groups: &[HourGroup], config: &PublicationConfig) -> Vec<usize> {
    match config.timetable.layout {
        ColumnLayout::Single => Vec::new(),
        ColumnLayout::SplitAt => config
            .timetable
            .split_at
            .iter()
            .filter_map(|hour| hour_groups.iter().position(|g| g.service_hour == *hour))
            .filter(|index| *index > 0)
            .collect(),
        ColumnLayout::Balanced | ColumnLayout::Responsive => {
            balanced_breaks(hour_groups, config.timetable.columns)
        }
    }
}

/// How many departures fit on one printed line of an hour row.
///
/// The value only feeds the column balance, so an approximation is
/// enough; it stops a busy hour from counting as many empty ones.
const DEPARTURES_PER_LINE: usize = 6;

/// Split the hour rows into columns of roughly equal height.
///
/// The weight of a row is the number of lines that its departures
/// need, which is what actually decides how tall a column becomes.
/// Counting departures instead would push a quiet morning and a busy
/// evening into wildly uneven columns. The order of the rows never
/// changes.
fn balanced_breaks(hour_groups: &[HourGroup], columns: usize) -> Vec<usize> {
    if columns <= 1 || hour_groups.len() < 2 {
        return Vec::new();
    }
    let columns = columns.min(hour_groups.len());
    let weights: Vec<usize> = hour_groups
        .iter()
        .map(|g| 1 + g.departures.len().saturating_sub(1) / DEPARTURES_PER_LINE)
        .collect();
    let total: usize = weights.iter().sum();
    let mut breaks = Vec::new();
    let mut carried = 0usize;
    let mut column = 1usize;
    for (index, weight) in weights.iter().enumerate() {
        if column >= columns {
            break;
        }
        // Break as soon as the running weight passes the ideal share
        // of the columns that are already complete.
        let target = total * column / columns;
        if carried + weight > target && index > 0 && hour_groups.len() - index >= columns - column {
            breaks.push(index);
            column += 1;
        }
        carried += weight;
    }
    breaks
}

/// Build a stable HTML-safe key for a panel.
fn panel_key(line: &LineView, key: &PanelKey) -> String {
    let mut parts = vec![line.key.clone()];
    match key.direction {
        Some(value) => parts.push(format!("d{value}")),
        None => parts.push("dx".to_string()),
    }
    if let Some(platform) = &key.platform {
        parts.push(format!("p{}", crate::common::css_key(platform)));
    }
    if let Some(destination) = &key.destination {
        parts.push(crate::common::css_key(destination));
    }
    parts.join("-")
}

/// Build the legend from the marks that the panels actually use.
fn build_legend(panels: &[TimetablePanel], labels: &Labels) -> Vec<LegendItem> {
    let mut used: Vec<DepartureFlag> = panels
        .iter()
        .flat_map(|p| p.hour_groups.iter())
        .flat_map(|g| g.departures.iter())
        .flat_map(|d| d.flags.iter().copied())
        .collect();
    used.sort_unstable();
    used.dedup();

    let mut legend: Vec<LegendItem> = used
        .into_iter()
        .map(|flag| LegendItem::with_symbol(flag.key(), flag.symbol(), flag.explanation(labels)))
        .collect();
    if panels.iter().any(|p| p.platform_label.is_some()) {
        legend.push(LegendItem::plain("platform", labels.legend_platform));
    }
    if panels.iter().any(|p| !p.frequency_notes.is_empty()) {
        legend.push(LegendItem::plain("headway", labels.legend_approximate));
    }
    legend
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(hour: u32, count: usize) -> HourGroup {
        HourGroup {
            service_hour: hour,
            display_hour: (hour % 24) as u8,
            departures: Vec::new(),
        }
        .with_count(count)
    }

    impl HourGroup {
        fn with_count(mut self, count: usize) -> Self {
            self.departures = (0..count)
                .map(|i| TimetableDeparture {
                    scheduled_time: GtfsTime::from_hms(self.service_hour, i as u32, 0),
                    display_minute: i as u8,
                    display_seconds: None,
                    destination: "X".into(),
                    destination_full: "X".into(),
                    platform: None,
                    trip_short_name: None,
                    source_trip_id: "T".into(),
                    instance_id: "I".into(),
                    exactness: TimeExactness::Exact,
                    flags: Vec::new(),
                })
                .collect();
            self
        }
    }

    #[test]
    fn balanced_columns_keep_the_hour_order() {
        let groups: Vec<HourGroup> = (4..28).map(|h| group(h, 4)).collect();
        let breaks = balanced_breaks(&groups, 2);
        assert_eq!(breaks, vec![12]);
        let breaks = balanced_breaks(&groups, 3);
        assert_eq!(breaks, vec![8, 16]);
    }

    #[test]
    fn balanced_columns_follow_the_height_of_the_rows() {
        // Eight hours with nine departures each need two printed lines
        // apiece; eight quiet hours need one. The break lands where the
        // two columns come out the same height.
        let mut groups: Vec<HourGroup> = (4..12).map(|h| group(h, 9)).collect();
        groups.extend((12..20).map(|h| group(h, 1)));
        assert_eq!(balanced_breaks(&groups, 2), vec![6]);
    }

    #[test]
    fn a_sparse_day_still_splits_down_the_middle() {
        // A feed whose early hours carry a few trains and whose later
        // hours carry none must not leave one column nearly empty.
        let mut groups: Vec<HourGroup> = (4..10).map(|h| group(h, 3)).collect();
        groups.extend((10..28).map(|h| group(h, 0)));
        assert_eq!(balanced_breaks(&groups, 2), vec![12]);
    }

    #[test]
    fn one_column_never_breaks() {
        let groups: Vec<HourGroup> = (4..28).map(|h| group(h, 1)).collect();
        assert!(balanced_breaks(&groups, 1).is_empty());
        assert!(balanced_breaks(&groups[..1], 3).is_empty());
    }

    #[test]
    fn seconds_appear_only_when_they_matter() {
        let round: GtfsTime = "06:10:00".parse().unwrap();
        let odd: GtfsTime = "06:10:30".parse().unwrap();
        assert_eq!(seconds_for(round, SecondsDisplay::ShowIfNonzero), None);
        assert_eq!(seconds_for(odd, SecondsDisplay::ShowIfNonzero), Some(30));
        assert_eq!(seconds_for(odd, SecondsDisplay::Hide), None);
        assert_eq!(seconds_for(round, SecondsDisplay::Show), Some(0));
    }

    #[test]
    fn a_direction_heading_lists_the_real_destinations() {
        let line = LineView {
            route_id: "TE".into(),
            name: "TEL".into(),
            long_name: None,
            color: None,
            text_color: None,
            key: "te".into(),
        };
        let config = PublicationConfig::default();
        let labels = Labels::for_language(config.language);

        let one = direction_heading(&line, Some(0), &["Springleaf".into()], &config, labels);
        assert_eq!(one, "For Springleaf");

        let two = direction_heading(
            &line,
            Some(0),
            &["Springleaf".into(), "Woodlands South".into()],
            &config,
            labels,
        );
        assert_eq!(two, "For Springleaf / Woodlands South");

        let none = direction_heading(&line, Some(1), &[], &config, labels);
        assert_eq!(none, "Direction 1");
    }
}
