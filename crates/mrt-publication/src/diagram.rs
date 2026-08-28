//! The time–distance string-diagram projection.
//!
//! [`build_diagram`] turns the runs of one corridor into a planning
//! diagram: time runs along the horizontal axis, stations down the
//! vertical axis, and each train is a polyline.
//!
//! # Geometry
//!
//! For every pair of consecutive calls the projection emits
//!
//! - a horizontal dwell segment from the arrival to the departure at
//!   one station, and
//! - a sloped travel segment from the departure at one station to the
//!   arrival at the next.
//!
//! Opposite directions therefore slope opposite ways without any
//! special case. A path that leaves the requested window is cut at
//! the window edge, so no geometry escapes the plot.
//!
//! # What the projection refuses to invent
//!
//! - A run whose calls do not fit the corridor is dropped with a
//!   diagnostic, never bent onto the axis.
//! - Headway service that GTFS marks `exact_times=0` becomes a band
//!   with a dashed envelope, not a set of solid train paths.

use mrt_gtfs::{
    Diagnostic, GtfsTime, RailNetwork, ServiceDate, TimeExactness, TimeQuality, TripInstance,
    TripInstanceQuery,
};
use serde::Serialize;

use crate::common::{
    round2, DepartureFlag, DocumentSeed, LegendItem, LineView, PublicationMetadata,
};
use crate::config::{PublicationConfig, StationSpacing, TripLabelMode};
use crate::corridor::{
    effective_spacing, resolve_corridor, AxisDirection, Corridor, DiagramTarget,
};
use crate::error::PublicationError;
use crate::text::{Labels, LocalizedText};

/// A finished diagram document.
#[derive(Clone, Debug, Serialize)]
pub struct DiagramDocument {
    /// Where the data came from.
    pub metadata: PublicationMetadata,
    /// The page title.
    pub title: LocalizedText,
    /// The service-day label.
    pub service_day_label: String,
    /// The plot geometry.
    pub layout: DiagramLayout,
    /// The horizontal axis.
    pub time_axis: TimeAxis,
    /// The vertical axis.
    pub corridor: Corridor,
    /// The station spacing that the document actually uses.
    pub station_spacing: StationSpacing,
    /// The train paths.
    pub runs: Vec<DiagramRun>,
    /// The headway blocks.
    pub frequency_bands: Vec<DiagramFrequencyBand>,
    /// The legend entries.
    pub legend: Vec<LegendItem>,
    /// Every line that the diagram draws, for the filter controls.
    pub lines: Vec<LineView>,
    /// Every destination that the diagram draws, for the filter
    /// controls.
    pub destinations: Vec<String>,
}

/// The plot geometry, in user units.
///
/// The renderers use these numbers directly as the SVG `viewBox`, so
/// the JSON and the drawing agree exactly.
#[derive(Clone, Debug, Serialize)]
pub struct DiagramLayout {
    /// The total width.
    pub width: f64,
    /// The total height.
    pub height: f64,
    /// The space for the station labels.
    pub margin_left: f64,
    /// The space to the right of the plot.
    pub margin_right: f64,
    /// The space for the upper time axis.
    pub margin_top: f64,
    /// The space for the lower time axis.
    pub margin_bottom: f64,
    /// The width of the plot area.
    pub plot_width: f64,
    /// The height of the plot area.
    pub plot_height: f64,
}

/// How strong a grid line is.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TickLevel {
    /// A faint line.
    Minor,
    /// A medium line.
    Medium,
    /// A strong line, usually on the hour.
    Major,
    /// The boundary between two calendar days, at `24:00`.
    DayBoundary,
}

/// One grid line of the time axis.
#[derive(Clone, Debug, Serialize)]
pub struct TimeTick {
    /// The time on the service day.
    pub time: GtfsTime,
    /// The horizontal coordinate.
    pub x: f64,
    /// How strong the line is.
    pub level: TickLevel,
    /// The label, on major lines and day boundaries only.
    pub label: Option<String>,
}

/// The horizontal axis.
#[derive(Clone, Debug, Serialize)]
pub struct TimeAxis {
    /// The first time of the window.
    pub start: GtfsTime,
    /// The exclusive end of the window.
    pub end: GtfsTime,
    /// The grid lines.
    pub ticks: Vec<TimeTick>,
    /// The interval of the strong lines, in minutes.
    pub major_minutes: u32,
    /// The interval of the medium lines, in minutes.
    pub medium_minutes: u32,
    /// The interval of the faint lines, in minutes.
    pub minor_minutes: u32,
}

/// One point of a train path.
#[derive(Clone, Debug, Serialize)]
pub struct DiagramPoint {
    /// The time on the service day.
    pub time: GtfsTime,
    /// The corridor node, or `None` for a point that clipping created.
    pub node: Option<usize>,
    /// The horizontal coordinate.
    pub x: f64,
    /// The vertical coordinate.
    pub y: f64,
}

/// One call of a train path.
#[derive(Clone, Debug, Serialize)]
pub struct DiagramCallView {
    /// The corridor node of the call.
    pub node: usize,
    /// The key of that node.
    pub node_key: String,
    /// The station name.
    pub station: String,
    /// The arrival time.
    pub arrival: Option<GtfsTime>,
    /// The departure time.
    pub departure: Option<GtfsTime>,
    /// The platform of the call.
    pub platform: Option<String>,
    /// The horizontal coordinate of the arrival.
    pub x_arrival: Option<f64>,
    /// The horizontal coordinate of the departure.
    pub x_departure: Option<f64>,
    /// The vertical coordinate.
    pub y: f64,
    /// Whether the train serves the station. A call that permits
    /// neither boarding nor alighting is a pass-through.
    pub stops: bool,
    /// Whether the call lies inside the requested window.
    pub in_window: bool,
    /// Where the times of the call come from.
    pub time_quality: TimeQuality,
}

/// Where a run label sits.
#[derive(Clone, Debug, Serialize)]
pub struct LabelPlacement {
    /// The horizontal coordinate of the label anchor.
    pub x: f64,
    /// The vertical coordinate of the label anchor.
    pub y: f64,
    /// The rotation in degrees, following the slope of the path.
    pub angle: f64,
}

/// One train path.
#[derive(Clone, Debug, Serialize)]
pub struct DiagramRun {
    /// The identifier of the run.
    pub instance_id: String,
    /// The internal GTFS trip identifier. The renderers keep it out of
    /// the drawing unless the configuration asks for it in the hover
    /// details.
    pub source_trip_id: String,
    /// The line of the run.
    pub line: LineView,
    /// The label that the diagram writes on the path, when one exists.
    pub label: Option<String>,
    /// The GTFS direction.
    pub direction: Option<u8>,
    /// Which way the run travels along the axis.
    pub axis_direction: AxisDirection,
    /// The destination of the run.
    pub destination: String,
    /// The corridor panel that the run belongs to.
    pub panel: usize,
    /// Whether the times are exact.
    pub exactness: TimeExactness,
    /// The polyline, clipped to the window.
    pub points: Vec<DiagramPoint>,
    /// The calls of the run.
    pub calls: Vec<DiagramCallView>,
    /// Whether the path was cut at the start of the window.
    pub clipped_start: bool,
    /// Whether the path was cut at the end of the window.
    pub clipped_end: bool,
    /// Where the label sits, when the layout found room for one.
    pub label_placement: Option<LabelPlacement>,
}

/// A headway block, as a diagram band.
#[derive(Clone, Debug, Serialize)]
pub struct DiagramFrequencyBand {
    /// The identifier of the band.
    pub band_id: String,
    /// The line of the band.
    pub line: LineView,
    /// The GTFS direction.
    pub direction: Option<u8>,
    /// Which way the band travels along the axis.
    pub axis_direction: AxisDirection,
    /// The destination of the band.
    pub destination: String,
    /// The corridor panel that the band belongs to.
    pub panel: usize,
    /// The first departure of the block.
    pub start: GtfsTime,
    /// The end of the block.
    pub end: GtfsTime,
    /// The headway in seconds.
    pub headway_secs: u32,
    /// The headway in whole minutes.
    pub headway_minutes: u32,
    /// The ready-made caption.
    pub label: String,
    /// The path of the first run of the block.
    pub first_path: Vec<DiagramPoint>,
    /// The path of the last run of the block.
    pub last_path: Vec<DiagramPoint>,
}

/// Build a string diagram.
pub fn build_diagram(
    network: &RailNetwork,
    target: &DiagramTarget,
    service_date: ServiceDate,
    from: GtfsTime,
    until: GtfsTime,
    config: &PublicationConfig,
    seed: &DocumentSeed,
) -> Result<DiagramDocument, PublicationError> {
    config.check().map_err(PublicationError::Configuration)?;
    if until <= from {
        return Err(PublicationError::Configuration(format!(
            "the diagram window {from} to {until} is empty"
        )));
    }
    let labels = Labels::for_language(config.language);
    let plan = resolve_corridor(network, target, config)?;
    let mut diagnostics = plan.diagnostics.clone();
    let corridor = plan.corridor.clone();
    let spacing = effective_spacing(&corridor, config);

    let mut query = TripInstanceQuery::new(service_date)
        .window(from, until)
        .frequency_policy(config.frequency_policy)
        .missing_time_policy(config.missing_time_policy);
    match target {
        DiagramTarget::Line(line) => query = query.line(*line),
        DiagramTarget::Pattern(pattern) => query = query.pattern(*pattern),
        DiagramTarget::Corridor(id) => {
            if let Some(name) = config.corridor(id).and_then(|c| c.line.clone()) {
                let line = network
                    .line_by_route_id(&name)
                    .or_else(|| {
                        network
                            .lines()
                            .iter()
                            .position(|l| l.name.eq_ignore_ascii_case(&name))
                            .map(mrt_gtfs::LineId)
                    })
                    .ok_or_else(|| {
                        PublicationError::UnresolvedLine(format!(
                            "the corridor \"{id}\" names the line \"{name}\", \
                             which is not in the feed"
                        ))
                    })?;
                query = query.line(line);
            }
        }
    }
    let result = network.query_trip_instances(&query)?;
    diagnostics.extend(result.diagnostics.iter().cloned());

    let layout = build_layout(&corridor, from, until, config);
    let time_axis = build_time_axis(from, until, &layout, config);
    let to_x = |time: GtfsTime| time_to_x(time, from, until, &layout);

    // Step 1: the train paths.
    let mut runs: Vec<DiagramRun> = Vec::new();
    for trip in &result.trips {
        let stations: Vec<mrt_gtfs::StationId> = trip.calls.iter().map(|c| c.station).collect();
        let Some(mapping) = plan.map_run(&stations) else {
            diagnostics.push(
                Diagnostic::warning(
                    "run-off-corridor",
                    "the calls of the run do not follow the station axis of this corridor, \
                     so the diagram leaves the run out",
                )
                .about(trip.source_trip_id.clone()),
            );
            continue;
        };
        match build_run(
            network,
            trip,
            &mapping.nodes,
            mapping.direction,
            &corridor,
            from,
            until,
            &to_x,
            config,
            labels,
        ) {
            Some(run) => runs.push(run),
            None => diagnostics.push(
                Diagnostic::info(
                    "run-outside-window",
                    "the run carries no time inside the diagram window",
                )
                .about(trip.source_trip_id.clone()),
            ),
        }
    }
    runs.sort_by(|a, b| {
        let key = |r: &DiagramRun| {
            (
                r.points.first().map(|p| p.time).unwrap_or(from),
                r.line.route_id.clone(),
                r.instance_id.clone(),
            )
        };
        key(a).cmp(&key(b))
    });

    // Step 2: the headway bands.
    let mut bands: Vec<DiagramFrequencyBand> = Vec::new();
    for band in &result.frequency_bands {
        let stations: Vec<mrt_gtfs::StationId> = band.template.iter().map(|c| c.station).collect();
        let Some(mapping) = plan.map_run(&stations) else {
            diagnostics.push(
                Diagnostic::warning(
                    "band-off-corridor",
                    "the template of a headway block does not follow the station axis",
                )
                .about(band.source_trip_id.clone()),
            );
            continue;
        };
        // The last scheduled start of the block, not `end - headway`:
        // the two differ when the block length is not a whole multiple
        // of the headway.
        let shift = band
            .last_start()
            .seconds()
            .saturating_sub(band.start.seconds());
        let first_path = path_from_times(
            &band.template,
            &mapping.nodes,
            &corridor,
            0,
            from,
            until,
            &to_x,
            config,
        );
        let last_path = path_from_times(
            &band.template,
            &mapping.nodes,
            &corridor,
            shift,
            from,
            until,
            &to_x,
            config,
        );
        if first_path.0.is_empty() && last_path.0.is_empty() {
            continue;
        }
        let destination = band
            .headsign
            .clone()
            .or_else(|| {
                band.template
                    .last()
                    .map(|c| network.station(c.station).name.clone())
            })
            .unwrap_or_default();
        bands.push(DiagramFrequencyBand {
            band_id: band.band_id.clone(),
            line: LineView::of(network, band.line),
            direction: band.direction,
            axis_direction: mapping.direction,
            destination,
            panel: mapping
                .nodes
                .first()
                .map(|&n| corridor.node(n).panel)
                .unwrap_or(0),
            start: band.start,
            end: band.end,
            headway_secs: band.headway_secs,
            headway_minutes: band.headway_minutes(),
            label: labels.headway_band(
                &crate::common::service_hhmm(band.start),
                &crate::common::service_hhmm(band.end),
                band.headway_minutes(),
            ),
            first_path: first_path.0,
            last_path: last_path.0,
        });
    }
    bands.sort_by(|a, b| (a.start, a.band_id.as_str()).cmp(&(b.start, b.band_id.as_str())));

    // Step 3: place the run labels.
    place_labels(&mut runs, config);

    // Step 4: the legend and the filter vocabularies.
    let legend = build_legend(&runs, &bands, &time_axis, labels);
    let mut lines: Vec<LineView> = runs
        .iter()
        .map(|r| r.line.clone())
        .chain(bands.iter().map(|b| b.line.clone()))
        .collect();
    lines.sort_by(|a, b| a.route_id.cmp(&b.route_id));
    lines.dedup_by(|a, b| a.route_id == b.route_id);
    let mut destinations: Vec<String> = runs
        .iter()
        .map(|r| r.destination.clone())
        .chain(bands.iter().map(|b| b.destination.clone()))
        .collect();
    destinations.sort();
    destinations.dedup();

    if runs.is_empty() && bands.is_empty() {
        diagnostics.push(Diagnostic::warning(
            "diagram-empty",
            format!("no run of this corridor falls inside {from} to {until} on {service_date}"),
        ));
    }
    mrt_gtfs::normalize_diagnostics(&mut diagnostics);

    let title = config.diagram.title.fill(&[
        ("corridor", corridor.label.as_str()),
        ("line", corridor.label.as_str()),
        ("date", &labels.service_date_text(service_date)),
    ]);
    Ok(DiagramDocument {
        metadata: PublicationMetadata::new(seed, service_date, diagnostics),
        title,
        service_day_label: labels.service_date_text(service_date),
        layout,
        time_axis,
        corridor,
        station_spacing: spacing,
        runs,
        frequency_bands: bands,
        legend,
        lines,
        destinations,
    })
}

/// Build the plot geometry.
fn build_layout(
    corridor: &Corridor,
    from: GtfsTime,
    until: GtfsTime,
    config: &PublicationConfig,
) -> DiagramLayout {
    /// The space that the station names need.
    const MARGIN_LEFT: f64 = 168.0;
    const MARGIN_RIGHT: f64 = 40.0;
    const MARGIN_TOP: f64 = 64.0;
    const MARGIN_BOTTOM: f64 = 56.0;

    let hours = (until.seconds() - from.seconds()) as f64 / 3600.0;
    let plot_width = (hours * config.diagram.pixels_per_hour).max(120.0);
    let plot_height = (corridor.height).max(config.diagram.row_height);
    DiagramLayout {
        width: round2(MARGIN_LEFT + plot_width + MARGIN_RIGHT),
        height: round2(MARGIN_TOP + plot_height + MARGIN_BOTTOM),
        margin_left: MARGIN_LEFT,
        margin_right: MARGIN_RIGHT,
        margin_top: MARGIN_TOP,
        margin_bottom: MARGIN_BOTTOM,
        plot_width: round2(plot_width),
        plot_height: round2(plot_height),
    }
}

/// Map a time onto the horizontal axis.
fn time_to_x(time: GtfsTime, from: GtfsTime, until: GtfsTime, layout: &DiagramLayout) -> f64 {
    let span = (until.seconds() - from.seconds()) as f64;
    let offset = time.seconds() as f64 - from.seconds() as f64;
    round2(layout.margin_left + offset / span * layout.plot_width)
}

/// Build the grid lines of the time axis.
fn build_time_axis(
    from: GtfsTime,
    until: GtfsTime,
    layout: &DiagramLayout,
    config: &PublicationConfig,
) -> TimeAxis {
    let minor = config.diagram.minor_grid_minutes.max(1) * 60;
    let medium = config.diagram.medium_grid_minutes.max(1) * 60;
    let major = config.diagram.major_grid_minutes.max(1) * 60;

    let mut ticks = Vec::new();
    let mut seconds = from.seconds().div_ceil(minor) * minor;
    while seconds <= until.seconds() {
        let time = GtfsTime::from_seconds(seconds);
        let level = if seconds % 86_400 == 0 && seconds > 0 {
            TickLevel::DayBoundary
        } else if seconds % major == 0 {
            TickLevel::Major
        } else if seconds % medium == 0 {
            TickLevel::Medium
        } else {
            TickLevel::Minor
        };
        let label = match level {
            TickLevel::Major | TickLevel::DayBoundary => Some(crate::common::service_hhmm(time)),
            _ => None,
        };
        ticks.push(TimeTick {
            time,
            x: time_to_x(time, from, until, layout),
            level,
            label,
        });
        seconds += minor;
    }
    TimeAxis {
        start: from,
        end: until,
        ticks,
        major_minutes: config.diagram.major_grid_minutes,
        medium_minutes: config.diagram.medium_grid_minutes,
        minor_minutes: config.diagram.minor_grid_minutes,
    }
}

/// Build one train path.
#[allow(clippy::too_many_arguments)]
fn build_run(
    network: &RailNetwork,
    trip: &TripInstance,
    nodes: &[usize],
    axis_direction: AxisDirection,
    corridor: &Corridor,
    from: GtfsTime,
    until: GtfsTime,
    to_x: &impl Fn(GtfsTime) -> f64,
    config: &PublicationConfig,
    labels: &Labels,
) -> Option<DiagramRun> {
    let (points, clipped_start, clipped_end) =
        path_from_times(&trip.calls, nodes, corridor, 0, from, until, to_x, config);
    if points.len() < 2 {
        return None;
    }

    let calls: Vec<DiagramCallView> = trip
        .calls
        .iter()
        .zip(nodes.iter())
        .map(|(call, &node)| {
            let time = call.arrival_or_departure();
            DiagramCallView {
                node,
                node_key: corridor.node(node).key.clone(),
                station: corridor.node(node).station.name.clone(),
                arrival: call.arrival,
                departure: call.departure,
                platform: call.platform_code.clone(),
                x_arrival: call.arrival.map(to_x),
                x_departure: call.departure.map(to_x),
                y: corridor.node(node).y,
                stops: !call.is_pass_through(),
                in_window: time.is_some_and(|t| t >= from && t < until),
                time_quality: call.time_quality,
            }
        })
        .collect();

    let destination = trip
        .headsign
        .clone()
        .or_else(|| {
            trip.calls
                .last()
                .map(|c| network.station(c.station).name.clone())
        })
        .unwrap_or_else(|| labels.direction_number(trip.direction));

    let label = match config.diagram.show_trip_labels {
        TripLabelMode::Never => None,
        _ => trip.short_name.clone(),
    };

    Some(DiagramRun {
        instance_id: trip.instance_id.clone(),
        source_trip_id: trip.source_trip_id.clone(),
        line: LineView::of(network, trip.line),
        label,
        direction: trip.direction,
        axis_direction,
        destination,
        panel: nodes.first().map(|&n| corridor.node(n).panel).unwrap_or(0),
        exactness: trip.exactness,
        points,
        calls,
        clipped_start,
        clipped_end,
        label_placement: None,
    })
}

/// Build a clipped polyline from a list of calls.
///
/// `shift_secs` moves every time later, which turns the template of a
/// headway block into the path of a later run of that block.
#[allow(clippy::too_many_arguments)]
fn path_from_times(
    calls: &[mrt_gtfs::ScheduledCall],
    nodes: &[usize],
    corridor: &Corridor,
    shift_secs: u32,
    from: GtfsTime,
    until: GtfsTime,
    to_x: &impl Fn(GtfsTime) -> f64,
    config: &PublicationConfig,
) -> (Vec<DiagramPoint>, bool, bool) {
    // Step 1: the raw polyline, in service-day seconds.
    let mut raw: Vec<(u32, f64, Option<usize>)> = Vec::with_capacity(calls.len() * 2);
    for (call, &node) in calls.iter().zip(nodes.iter()) {
        let y = corridor.node(node).y;
        let arrival = call.arrival_or_departure();
        let departure = call.departure_or_arrival();
        let (Some(arrival), Some(departure)) = (arrival, departure) else {
            continue;
        };
        let arrival = arrival.seconds() + shift_secs;
        let departure = departure.seconds() + shift_secs;
        push_point(&mut raw, arrival, y, Some(node));
        if config.diagram.show_dwell && departure > arrival {
            push_point(&mut raw, departure, y, Some(node));
        }
    }
    if raw.len() < 2 {
        return (Vec::new(), false, false);
    }

    // Step 2: clip to the window. The time of a polyline never goes
    // backwards, so a single forward pass is enough.
    let (start, end) = (from.seconds(), until.seconds());
    let clipped_start = raw.first().is_some_and(|p| p.0 < start);
    let clipped_end = raw.last().is_some_and(|p| p.0 > end);
    let mut cut: Vec<(u32, f64, Option<usize>)> = Vec::with_capacity(raw.len() + 2);
    for window in raw.windows(2) {
        let (t0, y0, n0) = window[0];
        let (t1, y1, _) = window[1];
        if t1 < start || t0 > end {
            continue;
        }
        let lo = t0.max(start);
        let hi = t1.min(end);
        let at = |t: u32| -> f64 {
            if t1 == t0 {
                y0
            } else {
                y0 + (y1 - y0) * (t - t0) as f64 / (t1 - t0) as f64
            }
        };
        push_point(&mut cut, lo, at(lo), if lo == t0 { n0 } else { None });
        push_point(&mut cut, hi, at(hi), None);
    }
    // Restore the node identity of the interior points that survived.
    for (time, _, node) in cut.iter_mut() {
        if node.is_none() {
            if let Some((_, _, original)) = raw.iter().find(|(t, _, _)| t == time) {
                *node = *original;
            }
        }
    }

    let points = cut
        .into_iter()
        .map(|(t, y, node)| {
            let time = GtfsTime::from_seconds(t);
            DiagramPoint {
                time,
                node,
                x: to_x(time),
                y: round2(y),
            }
        })
        .collect();
    (points, clipped_start, clipped_end)
}

/// Add a point unless it repeats the previous one exactly.
fn push_point(out: &mut Vec<(u32, f64, Option<usize>)>, time: u32, y: f64, node: Option<usize>) {
    if let Some(last) = out.last() {
        if last.0 == time && (last.1 - y).abs() < f64::EPSILON {
            return;
        }
    }
    out.push((time, y, node));
}

/// Place the run labels without overlapping.
///
/// The algorithm is deterministic: runs are processed in the order
/// they already have, the longest travel segment of each run is tried
/// first, and a label that finds no free box stays hidden. The
/// renderers reveal a hidden label on hover and on keyboard focus.
fn place_labels(runs: &mut [DiagramRun], config: &PublicationConfig) {
    if config.diagram.show_trip_labels == TripLabelMode::Never {
        return;
    }
    /// The height of a label box, in user units.
    const BOX_HEIGHT: f64 = 13.0;
    /// The width of one character, in user units.
    const CHAR_WIDTH: f64 = 5.6;

    let always = config.diagram.show_trip_labels == TripLabelMode::Always;
    let mut taken: Vec<(f64, f64, f64, f64)> = Vec::new();
    for run in runs.iter_mut() {
        let Some(label) = run.label.clone() else {
            continue;
        };
        let width = label.chars().count() as f64 * CHAR_WIDTH + 6.0;

        // Candidate segments, longest first, so the label sits where
        // the path has the most room.
        let mut candidates: Vec<(f64, usize)> = run
            .points
            .windows(2)
            .enumerate()
            .filter(|(_, w)| (w[1].y - w[0].y).abs() > f64::EPSILON)
            .map(|(index, w)| ((w[1].x - w[0].x).hypot(w[1].y - w[0].y), index))
            .collect();
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        for (_, index) in candidates {
            let a = &run.points[index];
            let b = &run.points[index + 1];
            let (x, y) = ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let box_ = (x - width / 2.0, y - BOX_HEIGHT / 2.0, width, BOX_HEIGHT);
            if !always && taken.iter().any(|other| overlaps(&box_, other)) {
                continue;
            }
            let angle = (b.y - a.y).atan2(b.x - a.x).to_degrees();
            run.label_placement = Some(LabelPlacement {
                x: round2(x),
                y: round2(y),
                angle: round2(angle),
            });
            taken.push(box_);
            break;
        }
    }
}

/// Report whether two axis-aligned boxes overlap.
fn overlaps(a: &(f64, f64, f64, f64), b: &(f64, f64, f64, f64)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// Build the legend from what the diagram actually draws.
fn build_legend(
    runs: &[DiagramRun],
    bands: &[DiagramFrequencyBand],
    axis: &TimeAxis,
    labels: &Labels,
) -> Vec<LegendItem> {
    let mut legend = Vec::new();
    if runs.iter().any(|r| r.calls.iter().any(|c| c.stops)) {
        legend.push(LegendItem::plain("stop", labels.legend_dwell));
    }
    if runs.iter().any(|r| r.calls.iter().any(|c| !c.stops)) {
        legend.push(LegendItem::plain(
            "pass-through",
            labels.legend_pass_through,
        ));
    }
    if runs
        .iter()
        .any(|r| r.exactness == TimeExactness::Approximate)
        || !bands.is_empty()
    {
        legend.push(LegendItem::with_symbol(
            DepartureFlag::Approximate.key(),
            DepartureFlag::Approximate.symbol(),
            labels.legend_approximate,
        ));
    }
    if runs.iter().any(|r| {
        r.calls
            .iter()
            .any(|c| c.time_quality == TimeQuality::Interpolated)
    }) {
        legend.push(LegendItem::with_symbol(
            DepartureFlag::Interpolated.key(),
            DepartureFlag::Interpolated.symbol(),
            labels.legend_interpolated,
        ));
    }
    if axis.ticks.iter().any(|t| t.level == TickLevel::DayBoundary) {
        legend.push(LegendItem::plain(
            "day-boundary",
            labels.legend_day_boundary,
        ));
    }
    legend
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxes_overlap_only_when_they_intersect() {
        let a = (0.0, 0.0, 10.0, 10.0);
        assert!(overlaps(&a, &(5.0, 5.0, 10.0, 10.0)));
        assert!(!overlaps(&a, &(10.0, 0.0, 10.0, 10.0)));
        assert!(!overlaps(&a, &(0.0, 10.0, 10.0, 10.0)));
    }

    #[test]
    fn repeated_points_collapse() {
        let mut out = Vec::new();
        push_point(&mut out, 100, 5.0, Some(1));
        push_point(&mut out, 100, 5.0, Some(1));
        push_point(&mut out, 100, 9.0, Some(2));
        assert_eq!(out.len(), 2);
    }
}
