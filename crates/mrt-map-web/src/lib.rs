//! Render the live map as one self-contained page.
//!
//! The map is a schematic: the geometry comes from the
//! OpenFantasyMap layout that [`mrt_live::BoundLayout`] binds to the
//! network, and the content comes from a [`NetworkSnapshot`]. Nothing
//! here queries a [`mrt_gtfs::RailNetwork`]; the renderer is strictly
//! downstream of the view models, as `docs/LIVE-MAP-POC.md` requires.
//!
//! # Why the builder lives here
//!
//! The map is its own site, deployable on its own subdomain, and the
//! board UI is untouched by it. The site has two deployments, exactly
//! as the board does: the `mrt-map-web` server and the
//! `mrt-map-static` generator. Both write the same page from the same
//! markup, so the shared renderer lives in this library, which the
//! server binary sits on top of and which `mrt-map-static` depends
//! on. It stays out of `mrt-live`, where markup does not belong.
//!
//! # Two layers, one page
//!
//! [`render_map_page`] writes the whole network — ribbons, station
//! discs, names, headway bands — into the document as SVG, so the page
//! is complete with JavaScript switched off. That is the
//! progressive-enhancement floor. The script adds trains and re-polls
//! [`map_snapshot_json`] from one URL, and nothing else.
//!
//! # Escaping
//!
//! Every string that came from a feed or from a layout file passes
//! [`text`] or [`attr`] before it reaches markup, and every colour
//! passes [`feed_color`] before it reaches a presentation attribute.
//! The rules mirror `mrt-publication-html/src/escape.rs`; they are
//! copied rather than shared, because the live stack does not depend
//! on the publication stack.

use std::collections::{BTreeMap, BTreeSet};

use mrt_gtfs::{alias, Diagnostic, LineId, RailNetwork, StationId};
use mrt_live::{BoundLayout, Layout, LayoutPoint, LineState, NetworkSnapshot, PositionQuality};

// ----------------------------------------------------------------------
// The layout
// ----------------------------------------------------------------------

/// The layout the map draws, unless `MRT_MAP_LAYOUT` names another.
pub const DEFAULT_LAYOUT: &str = "config/layout-mini.geojson";

/// Read the schematic layout named by `MRT_MAP_LAYOUT` — or
/// [`DEFAULT_LAYOUT`] — and bind it to the network.
///
/// A layout that cannot be read is not fatal. The map then draws
/// nothing and lists every station the layout failed to cover, which
/// is the diagnostic the plan asks for rather than a blank error. Both
/// deployments — the server and the static generator — load the layout
/// through this one function, so they agree on the environment
/// variable, the default, and the failure story.
pub fn load_layout(network: &RailNetwork) -> BoundLayout {
    let path = std::env::var("MRT_MAP_LAYOUT").unwrap_or_else(|_| DEFAULT_LAYOUT.to_string());
    let empty = || {
        Layout::from_geojson(&serde_json::json!({
            "type": "FeatureCollection",
            "features": [],
        }))
    };
    let layout = match std::fs::read_to_string(&path) {
        Ok(text) => match Layout::from_geojson_str(&text) {
            Ok(layout) => {
                eprintln!(
                    "Layout {path}: {} line(s), {} station(s).",
                    layout.lines.len(),
                    layout.stations.len()
                );
                layout
            }
            Err(error) => {
                eprintln!("Layout {path} is not valid JSON ({error}); the map draws nothing.");
                empty()
            }
        },
        Err(error) => {
            eprintln!("Cannot read the layout {path} ({error}); the map draws nothing.");
            empty()
        }
    };
    let bound = layout.bind(network);
    eprintln!(
        "Layout bound: {} station(s) drawn, {} unmatched, {} network station(s) uncovered.",
        bound.stations.len(),
        bound.unmatched.len(),
        bound.uncovered.len()
    );
    bound
}

// ----------------------------------------------------------------------
// Escaping
// ----------------------------------------------------------------------

/// Escape a string for element content in HTML and SVG.
///
/// The function escapes `&`, `<`, and `>`, and also `"` and `'`, which
/// costs nothing and makes the result safe in an attribute too.
pub fn text(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape a string for an attribute value in double quotes.
///
/// The rules are the same as for [`text`]; the separate name keeps the
/// call sites self-documenting.
pub fn attr(value: &str) -> String {
    text(value)
}

/// Escape a JSON document so that it is inert inside a `<script>`
/// element.
///
/// A feed string may contain `</script`, which would end the element
/// early. Escaping `<`, `>`, and `&` as `\uXXXX` keeps the document
/// valid JSON and inert as markup.
pub fn json_island(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(c),
        }
    }
    out
}

/// Filter a colour from a feed or a layout file.
///
/// GTFS writes `route_color` as six hexadecimal digits with no `#`,
/// and OpenFantasyMap writes `#rrggbb`, so both forms pass. Three,
/// four, six, and eight digits are accepted; anything else yields
/// `None` and the caller falls back to the palette. Nothing but a
/// literal colour can therefore reach a presentation attribute.
pub fn feed_color(value: &str) -> Option<String> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    let valid =
        matches!(digits.len(), 3 | 4 | 6 | 8) && digits.bytes().all(|b| b.is_ascii_hexdigit());
    valid.then(|| format!("#{digits}"))
}

// ----------------------------------------------------------------------
// The palette
// ----------------------------------------------------------------------

/// The official LTA line colours, by station-code prefix.
///
/// The same table the board uses for its station-code chips
/// (`crates/mrt-board-web/assets/index.html`). It is the last resort:
/// a feed colour and a layout colour both come first.
const LTA_PALETTE: &[(&str, &str)] = &[
    ("NS", "#d42e12"),
    ("EW", "#009645"),
    ("CG", "#009645"),
    ("NE", "#9900aa"),
    ("CC", "#fa9e0d"),
    ("CE", "#fa9e0d"),
    ("DT", "#005ec4"),
    ("TE", "#9d5b25"),
    ("BP", "#748477"),
    ("SK", "#748477"),
    ("SE", "#748477"),
    ("SW", "#748477"),
    ("STC", "#748477"),
    ("PG", "#748477"),
    ("PE", "#748477"),
    ("PW", "#748477"),
    ("PTC", "#748477"),
];

/// The colour of a line that no source names.
const NEUTRAL: &str = "#7a7f8c";

/// Get the palette colour for a station code, for example `NS1`.
fn palette_color(code: &str) -> Option<&'static str> {
    let prefix: String = code
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .flat_map(char::to_uppercase)
        .collect();
    LTA_PALETTE
        .iter()
        .find(|(key, _)| *key == prefix)
        .map(|&(_, color)| color)
}

// ----------------------------------------------------------------------
// The plane
// ----------------------------------------------------------------------

/// The widest the drawing may be, in viewBox units.
const VIEW_WIDTH: f64 = 1000.0;

/// The tallest the drawing may be, in viewBox units.
const VIEW_HEIGHT: f64 = 700.0;

/// The margin around the drawing, in viewBox units. Station names sit
/// beside their discs, so the drawing needs room outside the ribbons.
const VIEW_MARGIN: f64 = 56.0;

/// The share of the fitted width beyond which a station writes its
/// name to the left of its disc, so that a name near the right edge
/// stays on the drawing.
///
/// The threshold is a fraction of the width the drawing was actually
/// fitted to, not of [`VIEW_WIDTH`]. A tall layout fits into a box
/// narrower than the full view, and a threshold measured in the wide
/// box would sit outside it and flip every label on the drawing.
const LABEL_LEFT_FRACTION: f64 = 0.72;

/// A position in the viewBox.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ViewPoint {
    /// The horizontal coordinate.
    pub x: f64,
    /// The vertical coordinate.
    pub y: f64,
}

/// The transform from the layout plane to the viewBox.
///
/// The layout writes GeoJSON positions, so `y` grows north while an
/// SVG `y` grows down. The transform flips it. The schematic makes no
/// geographic claim either way: this is a fit to a box, not a
/// projection.
#[derive(Copy, Clone, Debug)]
struct Fit {
    min_x: f64,
    max_y: f64,
    scale: f64,
    width: f64,
    height: f64,
}

impl Fit {
    fn of(points: &[LayoutPoint]) -> Fit {
        let (mut min_x, mut max_x) = (f64::MAX, f64::MIN);
        let (mut min_y, mut max_y) = (f64::MAX, f64::MIN);
        for point in points {
            min_x = min_x.min(point.x);
            max_x = max_x.max(point.x);
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
        }
        if points.is_empty() {
            return Fit {
                min_x: 0.0,
                max_y: 0.0,
                scale: 1.0,
                width: VIEW_MARGIN * 2.0,
                height: VIEW_MARGIN * 2.0,
            };
        }
        let span_x = max_x - min_x;
        let span_y = max_y - min_y;
        // A layout drawn as one straight line has no span in one
        // direction. Fitting to the other keeps the drawing finite.
        let scale_x = (span_x > 0.0).then(|| (VIEW_WIDTH - 2.0 * VIEW_MARGIN) / span_x);
        let scale_y = (span_y > 0.0).then(|| (VIEW_HEIGHT - 2.0 * VIEW_MARGIN) / span_y);
        let scale = match (scale_x, scale_y) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => 1.0,
        };
        Fit {
            min_x,
            max_y,
            scale,
            width: span_x * scale + 2.0 * VIEW_MARGIN,
            height: span_y * scale + 2.0 * VIEW_MARGIN,
        }
    }

    fn apply(&self, point: LayoutPoint) -> ViewPoint {
        ViewPoint {
            x: (point.x - self.min_x) * self.scale + VIEW_MARGIN,
            y: (self.max_y - point.y) * self.scale + VIEW_MARGIN,
        }
    }
}

// ----------------------------------------------------------------------
// The geometry
// ----------------------------------------------------------------------

/// One drawn ribbon.
#[derive(Debug, Clone)]
pub struct GeoLine {
    /// The identifier of the layout line.
    pub layout_id: String,
    /// The display name the layout carries.
    pub name: Option<String>,
    /// The colour, already filtered.
    pub color: String,
    /// The drawn polyline, in viewBox units.
    pub points: Vec<ViewPoint>,
    /// The network line the ribbon carries, where the edges of the
    /// snapshot name one.
    pub line: Option<LineId>,
}

/// One drawn station.
///
/// A network station drawn on several layout lines — an interchange —
/// is one entry, anchored at the mean of its layout positions, so the
/// page carries one disc and one name for it.
#[derive(Debug, Clone)]
pub struct GeoStation {
    /// The network station.
    pub station: StationId,
    /// The public name.
    pub name: String,
    /// The public station codes.
    pub codes: Vec<String>,
    /// The anchor, in viewBox units.
    pub point: ViewPoint,
    /// Whether the layout draws the station on more than one line.
    pub interchange: bool,
    /// The colour of the line the station sits on.
    pub color: String,
    /// Whether the name goes to the left of the disc. A station near
    /// the right edge would otherwise write its name off the drawing.
    pub label_left: bool,
}

/// The drawing, in viewBox units.
#[derive(Debug, Clone)]
pub struct MapGeometry {
    /// The width of the viewBox.
    pub width: f64,
    /// The height of the viewBox.
    pub height: f64,
    /// The ribbons, in layout order.
    pub lines: Vec<GeoLine>,
    /// The stations, in station order.
    pub stations: Vec<GeoStation>,
    /// The polyline of every edge, keyed `line-from-to` by the index
    /// values of the snapshot. A train rides one of these.
    pub sections: BTreeMap<String, Vec<ViewPoint>>,
    /// Where a headway band writes its label, keyed by line index.
    pub band_anchors: BTreeMap<usize, ViewPoint>,
    /// The disrupted lines, keyed by line index, with the polyline of
    /// every affected section.
    ///
    /// A key is present for every line the alerts disrupt, so the
    /// ribbon can be greyed. The list of polylines is empty when the
    /// alert names no segment the map can join, which is a line marked
    /// as a whole and never a guessed section.
    pub disrupted: BTreeMap<usize, Vec<Vec<ViewPoint>>>,
    /// Everything the drawing could not represent.
    pub diagnostics: Vec<Diagnostic>,
}

/// One bound station, placed.
#[derive(Debug, Clone)]
struct Placed {
    layout_line: String,
    /// The arc position along that layout line, from 0 to 1.
    t: Option<f64>,
    point: ViewPoint,
    /// The station code the layout bound with. It is the last resort
    /// of the palette when no source names a colour.
    code: String,
    color: String,
}

/// Build the drawing from a snapshot and a bound layout.
///
/// The function reads no clock, touches nothing outside its
/// arguments, and returns the same drawing for the same inputs.
pub fn map_geometry(snapshot: &NetworkSnapshot, bound: &BoundLayout) -> MapGeometry {
    let mut diagnostics = Vec::new();

    // Step 1: fit the layout plane to the viewBox. Only the lines the
    // editor left switched on, and the stations that bound, take part.
    let drawable: Vec<&mrt_live::LayoutLine> = bound
        .layout
        .lines
        .iter()
        .filter(|line| line.visible && line.points.len() >= 2)
        .collect();
    let mut extent: Vec<LayoutPoint> = Vec::new();
    for line in &drawable {
        extent.extend(line.points.iter().copied());
    }
    for station in &bound.stations {
        if let Some(layout) = layout_station(bound, &station.layout_station) {
            extent.push(layout.point);
        }
    }
    let fit = Fit::of(&extent);
    if drawable.is_empty() {
        diagnostics.push(Diagnostic::error(
            "map-layout-empty",
            "the layout draws no visible line, so the page shows no network",
        ));
    }

    // Step 2: place every bound station on every layout line that
    // draws it.
    let mut placed: BTreeMap<usize, Vec<Placed>> = BTreeMap::new();
    for station in &bound.stations {
        let Some(layout) = layout_station(bound, &station.layout_station) else {
            continue;
        };
        placed.entry(station.station.0).or_default().push(Placed {
            layout_line: layout.line.clone(),
            t: layout.t,
            point: fit.apply(layout.point),
            code: station.code.clone(),
            color: String::new(),
        });
    }

    // Step 3: which network line every layout line carries, and the
    // colour that follows from it. The vote reads the placements
    // alone, so it runs before any colour is picked and every colour
    // on the drawing comes from the same answer.
    let carried = carried_lines(snapshot, &placed);
    for group in placed.values_mut() {
        for placement in group.iter_mut() {
            placement.color = line_color(
                snapshot,
                bound,
                &placement.layout_line,
                carried.get(&placement.layout_line).copied(),
                &placement.code,
            );
        }
    }
    let mut lines = Vec::new();
    for line in &drawable {
        let sample = bound
            .stations
            .iter()
            .find(|station| {
                layout_station(bound, &station.layout_station)
                    .is_some_and(|layout| layout.line == line.id)
            })
            .map(|station| station.code.clone())
            .unwrap_or_default();
        let network_line = carried.get(&line.id).copied();
        lines.push(GeoLine {
            layout_id: line.id.clone(),
            name: line.name.clone(),
            color: line_color(snapshot, bound, &line.id, network_line, &sample),
            points: line.points.iter().map(|&p| fit.apply(p)).collect(),
            line: network_line,
        });
    }

    // Step 4: one disc per network station, anchored at the mean of
    // its placements, so an interchange drawn on two ribbons carries
    // one disc between them. Everything else that has to meet that
    // disc — a chord, and the trains that ride it — reads the same
    // anchor from here.
    let right_edge = fit.width * LABEL_LEFT_FRACTION;
    let mut anchors: BTreeMap<usize, ViewPoint> = BTreeMap::new();
    let mut stations = Vec::new();
    for (index, group) in &placed {
        let count = group.len() as f64;
        let point = ViewPoint {
            x: group.iter().map(|p| p.point.x).sum::<f64>() / count,
            y: group.iter().map(|p| p.point.y).sum::<f64>() / count,
        };
        anchors.insert(*index, point);
        let Some(record) = snapshot.stations.iter().find(|s| s.station.0 == *index) else {
            continue;
        };
        stations.push(GeoStation {
            station: record.station,
            name: record.name.clone(),
            codes: record.codes.clone(),
            point,
            interchange: group.len() > 1,
            color: group[0].color.clone(),
            label_left: point.x > right_edge,
        });
    }

    // Step 5: one polyline per edge. Several patterns share an edge,
    // so the map keys them by line and by the two stations.
    let mut sections: BTreeMap<String, Vec<ViewPoint>> = BTreeMap::new();
    let mut chords = 0usize;
    for edge in &snapshot.edges {
        let key = section_key(edge.line, edge.from, edge.to);
        if sections.contains_key(&key) {
            continue;
        }
        match edge_polyline(&lines, &placed, edge.from, edge.to) {
            Some(points) => {
                sections.insert(key, points);
            }
            None => {
                // The chord joins the two discs, so it starts and ends
                // at the anchors the discs are drawn at. Reading one
                // raw placement instead would leave the chord, and the
                // trains that ride it, off the disc at an interchange.
                let (Some(&from), Some(&to)) = (anchors.get(&edge.from.0), anchors.get(&edge.to.0))
                else {
                    continue;
                };
                chords += 1;
                sections.insert(key, vec![from, to]);
            }
        }
    }
    if chords > 0 {
        diagnostics.push(Diagnostic::warning(
            "map-edge-without-geometry",
            format!(
                "{chords} edge(s) join two stations that the layout draws on no common \
                 line, so the map draws them as a straight chord between the two discs"
            ),
        ));
    }

    // Step 6: where a headway band writes its label.
    let mut band_anchors = BTreeMap::new();
    for line in &lines {
        let Some(id) = line.line else {
            continue;
        };
        if line.points.is_empty() {
            continue;
        }
        let cumulative = cumulative(&line.points);
        band_anchors.insert(id.0, point_at(&line.points, &cumulative, 0.5));
    }

    // Step 7: the affected section of every disrupted line.
    //
    // The alert names station codes and a direction, and nothing else.
    // So the mark is exactly the edges of the line whose two stations
    // the alert both names, joined along the layout path that already
    // draws them. A line whose alert names no station, or names
    // stations that no edge of it joins, is marked as a whole line and
    // leaves a diagnostic: the map never guesses which part is out.
    let mut disrupted: BTreeMap<usize, Vec<Vec<ViewPoint>>> = BTreeMap::new();
    for line in &snapshot.lines {
        let LineState::Disrupted {
            stations: codes, ..
        } = &line.state
        else {
            continue;
        };
        let mut affected: BTreeSet<usize> = BTreeSet::new();
        for code in codes {
            match station_by_code(snapshot, code) {
                Some(station) if placed.contains_key(&station.0) => {
                    affected.insert(station.0);
                }
                Some(_) => diagnostics.push(
                    Diagnostic::warning(
                        "map-disruption-station-undrawn",
                        "the alert names a station that the layout draws nowhere, so the \
                         mark cannot reach it",
                    )
                    .about(code.clone()),
                ),
                None => diagnostics.push(
                    Diagnostic::warning(
                        "map-disruption-station-unknown",
                        "no station of the network answers the code the alert names, so \
                         the mark cannot reach it",
                    )
                    .about(code.clone()),
                ),
            }
        }
        // One mark per section, not per edge: the two directions of a
        // line run over the same drawn section, and marking it twice
        // would lay one set of cuts across the other.
        let mut marked: BTreeSet<(usize, usize)> = BTreeSet::new();
        let mut paths: Vec<Vec<ViewPoint>> = Vec::new();
        for edge in &snapshot.edges {
            if edge.line != line.line
                || !affected.contains(&edge.from.0)
                || !affected.contains(&edge.to.0)
            {
                continue;
            }
            let pair = (edge.from.0.min(edge.to.0), edge.from.0.max(edge.to.0));
            if marked.contains(&pair) {
                continue;
            }
            let Some(points) = sections.get(&section_key(edge.line, edge.from, edge.to)) else {
                continue;
            };
            marked.insert(pair);
            paths.push(points.clone());
        }
        if paths.is_empty() {
            diagnostics.push(
                Diagnostic::info(
                    "map-disruption-without-segment",
                    "the alert names no two neighbouring stations of this line, so the \
                     map marks the whole line rather than a guessed section",
                )
                .about(line.name.clone()),
            );
        }
        disrupted.insert(line.line.0, paths);
    }

    MapGeometry {
        width: fit.width,
        height: fit.height,
        lines,
        stations,
        sections,
        band_anchors,
        disrupted,
        diagnostics,
    }
}

/// Find the station of the snapshot that answers one public code.
///
/// The comparison is `mrt-gtfs`'s alias key, the same one the layout
/// binder resolves a layout station with, so `NS1`, `ns-1`, and `NS 1`
/// all name the same station.
fn station_by_code(snapshot: &NetworkSnapshot, code: &str) -> Option<StationId> {
    let key = alias::normalize(code);
    snapshot
        .stations
        .iter()
        .find(|station| {
            station
                .codes
                .iter()
                .any(|candidate| alias::normalize(candidate) == key)
        })
        .map(|station| station.station)
}

/// The key of one edge in [`MapGeometry::sections`].
fn section_key(line: LineId, from: StationId, to: StationId) -> String {
    format!("{}-{}-{}", line.0, from.0, to.0)
}

/// Get the layout station behind a bound station.
fn layout_station<'a>(bound: &'a BoundLayout, id: &str) -> Option<&'a mrt_live::LayoutStation> {
    bound
        .layout
        .stations
        .iter()
        .find(|station| station.id == id)
}

/// Decide which network line a layout line carries.
///
/// The answer comes from the snapshot alone: an edge whose two
/// stations the layout both draws on this line is a vote for the line
/// of that edge. The line with the most votes wins, and the lowest
/// identifier breaks a tie, so the result never moves between runs.
fn carried_lines(
    snapshot: &NetworkSnapshot,
    placed: &BTreeMap<usize, Vec<Placed>>,
) -> BTreeMap<String, LineId> {
    let mut votes: BTreeMap<String, BTreeMap<usize, usize>> = BTreeMap::new();
    for edge in &snapshot.edges {
        let (Some(from), Some(to)) = (placed.get(&edge.from.0), placed.get(&edge.to.0)) else {
            continue;
        };
        for candidate in from {
            if to.iter().any(|p| p.layout_line == candidate.layout_line) {
                *votes
                    .entry(candidate.layout_line.clone())
                    .or_default()
                    .entry(edge.line.0)
                    .or_default() += 1;
            }
        }
    }
    votes
        .into_iter()
        .filter_map(|(layout_line, tally)| {
            tally
                .into_iter()
                .max_by_key(|&(line, count)| (count, std::cmp::Reverse(line)))
                .map(|(line, _)| (layout_line, LineId(line)))
        })
        .collect()
}

/// Pick the colour of one layout line.
///
/// The feed colour of the network line the ribbon carries comes first,
/// because the operator publishes it; the layout colour second,
/// because a person drew it; the official palette last. Every one of
/// them passes [`feed_color`] first, so a hostile `route_color`
/// becomes a palette colour rather than a CSS declaration.
///
/// `network_line` is [`carried_lines`]'s answer, the majority vote over
/// the edges the layout draws on the ribbon. The first station of a
/// ribbon is not that answer: at an interchange it belongs to two
/// lines, and reading its first touching edge would paint a whole line
/// in a neighbour's colour.
fn line_color(
    snapshot: &NetworkSnapshot,
    bound: &BoundLayout,
    layout_line: &str,
    network_line: Option<LineId>,
    sample_code: &str,
) -> String {
    let feed = network_line
        .and_then(|id| snapshot.lines.iter().find(|line| line.line == id))
        .and_then(|line| line.color.as_deref())
        .and_then(feed_color);
    feed.or_else(|| {
        bound
            .layout
            .line(layout_line)
            .and_then(|line| line.color.as_deref())
            .and_then(feed_color)
    })
    .or_else(|| palette_color(sample_code).map(str::to_string))
    .unwrap_or_else(|| NEUTRAL.to_string())
}

/// Build the polyline of one edge along the ribbon that carries it.
///
/// Both stations must sit on one layout line and carry an arc
/// position. A closed ribbon — a loop — always runs forward and wraps
/// past its own start; an open one runs backwards when the edge does,
/// which is what the return direction of a pattern looks like.
fn edge_polyline(
    lines: &[GeoLine],
    placed: &BTreeMap<usize, Vec<Placed>>,
    from: StationId,
    to: StationId,
) -> Option<Vec<ViewPoint>> {
    let (behind, ahead) = (placed.get(&from.0)?, placed.get(&to.0)?);
    for line in lines {
        let Some(back) = behind
            .iter()
            .find(|p| p.layout_line == line.layout_id && p.t.is_some())
        else {
            continue;
        };
        let Some(front) = ahead
            .iter()
            .find(|p| p.layout_line == line.layout_id && p.t.is_some())
        else {
            continue;
        };
        if line.points.len() < 2 {
            continue;
        }
        let (a, b) = (back.t?, front.t?);
        let cumulative = cumulative(&line.points);
        if a <= b {
            return Some(slice(&line.points, &cumulative, a, b));
        }
        if is_closed(&line.points) {
            // The loop carries on past its own start.
            let mut points = slice(&line.points, &cumulative, a, 1.0);
            points.extend(slice(&line.points, &cumulative, 0.0, b).into_iter().skip(1));
            return Some(points);
        }
        let mut points = slice(&line.points, &cumulative, b, a);
        points.reverse();
        return Some(points);
    }
    None
}

/// Report whether a polyline returns to where it started.
fn is_closed(points: &[ViewPoint]) -> bool {
    match (points.first(), points.last()) {
        (Some(first), Some(last)) => {
            (first.x - last.x).abs() < 1e-6 && (first.y - last.y).abs() < 1e-6
        }
        _ => false,
    }
}

/// The cumulative arc length at each vertex of a polyline.
fn cumulative(points: &[ViewPoint]) -> Vec<f64> {
    let mut out = Vec::with_capacity(points.len());
    let mut total = 0.0;
    for (index, point) in points.iter().enumerate() {
        if index > 0 {
            let previous = points[index - 1];
            total += (point.x - previous.x).hypot(point.y - previous.y);
        }
        out.push(total);
    }
    out
}

/// Get the position at a fraction of the arc length of a polyline.
///
/// This is OpenFantasyMap's `pointAtFraction`, which is literally
/// "place a train at fraction t along this edge".
fn point_at(points: &[ViewPoint], cumulative: &[f64], fraction: f64) -> ViewPoint {
    let total = cumulative.last().copied().unwrap_or(0.0);
    if points.is_empty() {
        return ViewPoint { x: 0.0, y: 0.0 };
    }
    if total <= 0.0 {
        return points[0];
    }
    let target = fraction.clamp(0.0, 1.0) * total;
    for index in 1..points.len() {
        if cumulative[index] < target {
            continue;
        }
        let span = cumulative[index] - cumulative[index - 1];
        let share = if span > 0.0 {
            (target - cumulative[index - 1]) / span
        } else {
            0.0
        };
        let (a, b) = (points[index - 1], points[index]);
        return ViewPoint {
            x: a.x + (b.x - a.x) * share,
            y: a.y + (b.y - a.y) * share,
        };
    }
    points[points.len() - 1]
}

/// Cut the part of a polyline between two arc fractions, `a <= b`.
fn slice(points: &[ViewPoint], cumulative: &[f64], a: f64, b: f64) -> Vec<ViewPoint> {
    let total = cumulative.last().copied().unwrap_or(0.0);
    let (start, end) = (a.clamp(0.0, 1.0) * total, b.clamp(0.0, 1.0) * total);
    let mut out = vec![point_at(points, cumulative, a)];
    for (index, point) in points.iter().enumerate() {
        if cumulative[index] > start && cumulative[index] < end {
            out.push(*point);
        }
    }
    out.push(point_at(points, cumulative, b));
    out
}

// ----------------------------------------------------------------------
// The static drawing
// ----------------------------------------------------------------------

/// Format a coordinate for an SVG attribute.
///
/// Two decimals are finer than one viewBox unit is ever drawn, and a
/// fixed number of them keeps the committed snapshot stable across
/// platforms.
fn n(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    let mut out = format!("{rounded:.2}");
    if out.contains('.') {
        out = out.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if out == "-0" {
        out = "0".to_string();
    }
    out
}

/// Turn a polyline into an SVG path.
fn path_of(points: &[ViewPoint]) -> String {
    let mut out = String::with_capacity(points.len() * 14);
    for (index, point) in points.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push(if index == 0 { 'M' } else { 'L' });
        out.push_str(&n(point.x));
        out.push(' ');
        out.push_str(&n(point.y));
    }
    out
}

/// Render the network as SVG: the ribbons, the discs, the names, and
/// the headway band labels.
///
/// The `<g id="map-trains">` group is left empty. Trains are the one
/// thing the script adds, and a page without a script correctly shows
/// none.
pub fn render_network_svg(snapshot: &NetworkSnapshot, geometry: &MapGeometry) -> String {
    let mut out = String::with_capacity(64 * 1024);
    out.push_str(&format!(
        "<svg class=\"map\" xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" \
         preserveAspectRatio=\"xMidYMid meet\" role=\"img\" \
         aria-labelledby=\"map-svg-title map-svg-desc\">\n",
        n(geometry.width),
        n(geometry.height)
    ));
    out.push_str("<title id=\"map-svg-title\">The rail network as a schematic</title>\n");
    out.push_str("<desc id=\"map-svg-desc\">");
    out.push_str(&text(&describe(snapshot, geometry)));
    out.push_str("</desc>\n");

    // The ribbons, one casing and one colour stroke each, in layout
    // order. The casing is the page background at twice the weight, so
    // the line drawn last cuts a clean gap through the ones beneath
    // it. That is OpenFantasyMap's dual-stroke idiom adapted to a void
    // background, where a dark casing would be invisible.
    out.push_str("<g class=\"ribbons\" aria-hidden=\"true\">\n");
    for line in &geometry.lines {
        if line.points.len() < 2 {
            continue;
        }
        let d = path_of(&line.points);
        // A disrupted line is not deleted from the map: it keeps its
        // shape and loses its identity colour, which is the whole
        // statement the alert supports about the line as a whole.
        let out_of_service = line
            .line
            .is_some_and(|id| geometry.disrupted.contains_key(&id.0));
        out.push_str(if out_of_service {
            "<g class=\"ribbon-group disrupted\" data-layout-line=\""
        } else {
            "<g class=\"ribbon-group\" data-layout-line=\""
        });
        out.push_str(&attr(&line.layout_id));
        out.push_str("\">\n");
        out.push_str(&format!("<path class=\"casing\" d=\"{d}\"/>\n"));
        out.push_str(&format!(
            "<path class=\"ribbon\" stroke=\"{}\" d=\"{d}\"/>\n",
            attr(&line.color)
        ));
        out.push_str("</g>\n");
    }
    out.push_str("</g>\n");

    // The affected sections, over the greyed ribbons. Each is drawn
    // twice: once in the grey of a disrupted line, and once in the
    // background colour as a dashed cut through it, so the section the
    // alert names reads as a broken line while the rest of the line
    // stays whole. The grey stroke carries the mark on its own, which
    // matters where the layout draws no ribbon under the section and
    // the map falls back to a chord between the two discs — that edge
    // is the one the trains ride, and marking it there is the same
    // claim, not a second one. The group only exists when an alert
    // names a section: nothing is drawn on a normal network.
    let marks: Vec<&Vec<ViewPoint>> = geometry
        .disrupted
        .values()
        .flatten()
        .filter(|points| points.len() >= 2)
        .collect();
    if !marks.is_empty() {
        out.push_str("<g class=\"disruptions\" aria-hidden=\"true\">\n");
        for points in &marks {
            out.push_str(&format!(
                "<path class=\"disrupted-section\" d=\"{}\"/>\n",
                path_of(points)
            ));
        }
        for points in &marks {
            out.push_str(&format!(
                "<path class=\"disrupted-cut\" d=\"{}\"/>\n",
                path_of(points)
            ));
        }
        out.push_str("</g>\n");
    }

    // The headway bands. A block with exact_times=0 has no individual
    // runs, so the line carries a label instead of trains.
    out.push_str("<g class=\"bands\" id=\"map-bands\">\n");
    out.push_str(&render_bands(snapshot, geometry));
    out.push_str("</g>\n");

    // The stations. A hollow disc ringed in the line colour, and one
    // distinct shape for an interchange: a larger pale disc with a
    // dark ring, which stays legible with a train pill sitting on it.
    out.push_str("<g class=\"stations\">\n");
    for station in &geometry.stations {
        out.push_str("<g class=\"station");
        if station.interchange {
            out.push_str(" interchange");
        }
        out.push_str("\">\n<title>");
        out.push_str(&text(&station_title(station)));
        out.push_str("</title>\n");
        if station.interchange {
            out.push_str(&format!(
                "<circle class=\"disc\" cx=\"{}\" cy=\"{}\" r=\"6.4\"/>\n",
                n(station.point.x),
                n(station.point.y)
            ));
        } else {
            out.push_str(&format!(
                "<circle class=\"disc\" cx=\"{}\" cy=\"{}\" r=\"4.4\" stroke=\"{}\"/>\n",
                n(station.point.x),
                n(station.point.y),
                attr(&station.color)
            ));
        }
        let offset = if station.interchange { 10.5 } else { 8.5 };
        let (x, anchor) = if station.label_left {
            (station.point.x - offset, "end")
        } else {
            (station.point.x + offset, "start")
        };
        out.push_str(&format!(
            "<text class=\"station-name\" x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\">{}</text>\n",
            n(x),
            n(station.point.y + 3.4),
            text(&station.name)
        ));
        out.push_str("</g>\n");
    }
    out.push_str("</g>\n");

    // The script fills this group and nothing else.
    out.push_str("<g class=\"trains\" id=\"map-trains\"></g>\n");
    out.push_str("</svg>\n");
    out
}

/// Render the headway band labels.
fn render_bands(snapshot: &NetworkSnapshot, geometry: &MapGeometry) -> String {
    let mut out = String::new();
    // One label per line: several blocks of one line in one day would
    // otherwise stack their labels on one another.
    let mut seen: Vec<usize> = Vec::new();
    for band in &snapshot.bands {
        if seen.contains(&band.line.0) {
            continue;
        }
        let Some(anchor) = geometry.band_anchors.get(&band.line.0) else {
            continue;
        };
        let color = geometry
            .lines
            .iter()
            .find(|line| line.line == Some(band.line))
            .map(|line| line.color.clone())
            .unwrap_or_else(|| NEUTRAL.to_string());
        seen.push(band.line.0);
        out.push_str(&format!(
            "<text class=\"band-label\" x=\"{}\" y=\"{}\" fill=\"{}\">{}</text>\n",
            n(anchor.x),
            n(anchor.y - 9.0),
            attr(&color),
            text(&format!("every {} min approximately", band.headway_minutes))
        ));
    }
    out
}

/// Build the accessible title of one station.
fn station_title(station: &GeoStation) -> String {
    let mut out = station.name.clone();
    if !station.codes.is_empty() {
        out.push_str(" (");
        out.push_str(&station.codes.join(", "));
        out.push(')');
    }
    if station.interchange {
        out.push_str(" \u{00B7} interchange");
    }
    out
}

/// Build the text alternative of the drawing.
///
/// A disrupted line is greyed and its affected section is drawn
/// broken, so the description says how many lines are in that state:
/// the drawing is not the only place the page states it, but a reader
/// who cannot see the drawing gets the same count.
fn describe(snapshot: &NetworkSnapshot, geometry: &MapGeometry) -> String {
    let mut out = format!(
        "A schematic of {} line(s) and {} station(s), on the service day {} at {}. \
         The drawing is not geographic.",
        geometry.lines.len(),
        geometry.stations.len(),
        snapshot.freshness.service_date,
        snapshot.freshness.clock
    );
    if !geometry.disrupted.is_empty() {
        out.push_str(&format!(
            " {} line(s) are marked disrupted; the notices name them.",
            geometry.disrupted.len()
        ));
    }
    out
}

// ----------------------------------------------------------------------
// The transported snapshot
// ----------------------------------------------------------------------

/// Build the body of `/api/map-snapshot`, and of the static
/// `data/map.json`.
///
/// The two deployments carry the same document, so the page has one
/// code path. `generated` is the POSIX time the caller built it at,
/// which is the only clock in the whole map stack and belongs to the
/// caller, not to the builder.
pub fn map_snapshot_json(
    snapshot: &NetworkSnapshot,
    live: bool,
    generated: i64,
) -> serde_json::Value {
    serde_json::json!({
        "generated": generated,
        "live": live,
        "snapshot": snapshot,
    })
}

/// Build the geometry island that the script reads.
///
/// It carries only what the script needs to place a train: the size of
/// the viewBox, the anchor of every station, the polyline of every
/// edge, and the filtered colour and name of every line. The colours
/// have already passed [`feed_color`], so the script never handles an
/// unfiltered one.
fn geometry_json(geometry: &MapGeometry, snapshot_url: &str, poll_secs: u32) -> serde_json::Value {
    let stations: serde_json::Map<String, serde_json::Value> = geometry
        .stations
        .iter()
        .map(|station| {
            (
                station.station.0.to_string(),
                serde_json::json!([round(station.point.x), round(station.point.y)]),
            )
        })
        .collect();
    let sections: serde_json::Map<String, serde_json::Value> = geometry
        .sections
        .iter()
        .map(|(key, points)| {
            let coordinates: Vec<serde_json::Value> = points
                .iter()
                .map(|point| serde_json::json!([round(point.x), round(point.y)]))
                .collect();
            (key.clone(), serde_json::json!(coordinates))
        })
        .collect();
    let lines: serde_json::Map<String, serde_json::Value> = geometry
        .lines
        .iter()
        .filter_map(|line| {
            line.line.map(|id| {
                (
                    id.0.to_string(),
                    serde_json::json!({ "color": line.color, "name": line.name }),
                )
            })
        })
        .collect();
    let bands: serde_json::Map<String, serde_json::Value> = geometry
        .band_anchors
        .iter()
        .map(|(line, point)| {
            (
                line.to_string(),
                serde_json::json!([round(point.x), round(point.y)]),
            )
        })
        .collect();
    serde_json::json!({
        "snapshotUrl": snapshot_url,
        "pollSecs": poll_secs,
        "stations": stations,
        "sections": sections,
        "lines": lines,
        "bands": bands,
    })
}

/// Round a coordinate to two decimals for the island.
fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

// ----------------------------------------------------------------------
// The page
// ----------------------------------------------------------------------

/// The page shell, with its stylesheet and its script.
///
/// The asset lives beside this crate's server binary, as the board
/// page lives beside the board server. The static generator writes the
/// same document.
const SHELL: &str = include_str!("../assets/map.html");

/// How often the page re-polls, in seconds.
///
/// The board polls every 15 s behind a 20 s server-side TTL. The map
/// snapshot is a heavier document and the positions move no faster, so
/// the map polls at the slower end of the same range.
pub const POLL_SECS: u32 = 20;

/// Everything the page needs that is not the snapshot.
pub struct MapPageInput<'a> {
    /// The snapshot that the static layer is drawn from.
    pub snapshot: &'a NetworkSnapshot,
    /// The layout, bound to the network.
    pub layout: &'a BoundLayout,
    /// The URL the script polls. It is the only request the page
    /// makes.
    pub snapshot_url: &'a str,
    /// One sentence naming how often the data behind that URL is
    /// refreshed, which differs between the server and the static
    /// site.
    pub deployment: &'a str,
}

/// Render the whole map page.
pub fn render_map_page(input: &MapPageInput) -> String {
    let geometry = map_geometry(input.snapshot, input.layout);
    let svg = render_network_svg(input.snapshot, &geometry);
    let island = geometry_json(&geometry, input.snapshot_url, POLL_SECS);

    let mut diagnostics = input.layout.diagnostics.clone();
    diagnostics.extend(geometry.diagnostics.iter().cloned());
    diagnostics.extend(input.snapshot.diagnostics.iter().cloned());

    let dated = format!(
        "{} at {}",
        input.snapshot.freshness.service_date, input.snapshot.freshness.clock
    );
    SHELL
        .replace("<!--MAP-SVG-->", &svg)
        .replace("<!--MAP-GEOMETRY-->", &json_island(&island.to_string()))
        .replace("<!--MAP-STATUS-->", &text(&freshness_words(input.snapshot)))
        .replace("<!--MAP-LAMP-->", freshness_class(input.snapshot))
        .replace("<!--MAP-NOTICES-->", &render_notices(input.snapshot))
        .replace("<!--MAP-DATE-->", &text(&dated))
        .replace("<!--MAP-DEPLOYMENT-->", &text(input.deployment))
        .replace("<!--MAP-DIAGNOSTICS-->", &render_diagnostics(&diagnostics))
        .replace("<!--MAP-CONNECT-->", &connect_extra(input.snapshot_url))
        .replace("<!--MAP-SNAPSHOT-URL-->", &attr(input.snapshot_url))
}

/// The extra `connect-src` source for a cross-origin snapshot URL.
///
/// The page's Content-Security-Policy allows exactly one fetch. A
/// same-origin snapshot — the server endpoint, or the static site's
/// bundled `data/map.json` — is covered by `'self'` and adds nothing.
/// A static site configured to poll a fast-refresh snapshot on another
/// origin needs that origin allowed too, and nothing else: the value
/// is reduced to `scheme://host[:port]` and dropped entirely unless
/// every character is inert inside the `content` attribute, so a
/// hostile URL cannot widen the policy.
fn connect_extra(snapshot_url: &str) -> String {
    if !snapshot_url.starts_with("https://") && !snapshot_url.starts_with("http://") {
        return String::new();
    }
    let (scheme, rest) = snapshot_url.split_once("://").expect("checked above");
    let host = rest.split('/').next().unwrap_or("");
    let valid = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'));
    if !valid {
        return String::new();
    }
    format!(" {scheme}://{host}")
}

/// The lamp class for the freshness of a snapshot.
///
/// The grammar is the board's three lamps, and the three states of
/// [`mrt_live::FreshnessState`] map onto them one for one: green while
/// the realtime layer is current, amber while it is ageing and its
/// predictions still apply, red once it is stale — and red too when
/// there is no realtime layer at all, because both mean the same thing
/// to a reader, that nothing on the map is live. The age in words sits
/// beside the lamp either way. Nothing else on the page uses these
/// three colours.
fn freshness_class(snapshot: &NetworkSnapshot) -> &'static str {
    match snapshot.freshness.state {
        mrt_live::FreshnessState::Live => "live",
        mrt_live::FreshnessState::Ageing => "ageing",
        mrt_live::FreshnessState::Stale | mrt_live::FreshnessState::Unavailable => "stale",
    }
}

/// Say the freshness of a snapshot in words.
fn freshness_words(snapshot: &NetworkSnapshot) -> String {
    match (snapshot.freshness.state, snapshot.freshness.age_secs) {
        (mrt_live::FreshnessState::Live, Some(age)) => {
            format!("live \u{00B7} realtime feed {}", ago(age))
        }
        (mrt_live::FreshnessState::Live, None) => "live".to_string(),
        (mrt_live::FreshnessState::Ageing, Some(age)) => {
            format!("ageing \u{00B7} realtime feed {}", ago(age))
        }
        (mrt_live::FreshnessState::Ageing, None) => "ageing".to_string(),
        (mrt_live::FreshnessState::Stale, Some(age)) => {
            format!("schedule only \u{00B7} realtime feed {}", ago(age))
        }
        (mrt_live::FreshnessState::Stale, None) => {
            "schedule only \u{00B7} the realtime feed carries no timestamp".to_string()
        }
        (mrt_live::FreshnessState::Unavailable, _) => {
            "schedule only \u{00B7} no realtime layer".to_string()
        }
    }
}

/// Render the notice area: what the operator said, and what the map
/// can say about its own realtime coverage.
///
/// Everything here comes from the snapshot and passes [`text`] on the
/// way in. The area does not exist when there is nothing to say, so a
/// normal network carries no empty panel.
///
/// One notice per disrupted line names the line, the direction, the
/// stations, and the free bus service — all of them fields the alert
/// itself carries. The alert messages follow, once, as the network
/// notices they are: the legacy payload attaches no message to a
/// segment, so the page never claims one belongs to one line.
fn render_notices(snapshot: &NetworkSnapshot) -> String {
    let mut items: Vec<String> = Vec::new();
    for line in &snapshot.lines {
        let LineState::Disrupted {
            stations,
            direction,
            free_public_bus,
        } = &line.state
        else {
            continue;
        };
        let mut parts = vec![format!("{} disrupted", text(&line.name))];
        if !direction.trim().is_empty() {
            parts.push(format!("direction {}", text(direction)));
        }
        if stations.is_empty() {
            parts.push("the alert names no station".to_string());
        } else {
            parts.push(text(&stations.join(", ")));
        }
        if !free_public_bus.is_empty() {
            parts.push(format!(
                "free public bus at {}",
                text(&free_public_bus.join(", "))
            ));
        }
        items.push(format!(
            "<p class=\"notice disrupted\">{}</p>\n",
            parts.join(" \u{00B7} ")
        ));
    }
    for message in &snapshot.notices {
        items.push(format!("<p class=\"notice\">{}</p>\n", text(message)));
    }
    // The realtime layer is current, and yet not one run on the map
    // carries a position from it. That is the empty-snapshot state,
    // and it is worth a sentence: without one the page looks live and
    // shows nothing but the timetable.
    if snapshot.freshness.state.is_current()
        && !snapshot.trains.is_empty()
        && snapshot
            .trains
            .iter()
            .all(|train| train.quality == PositionQuality::ScheduleOnly)
    {
        items.push(
            "<p class=\"notice\">The realtime layer names no run drawn on this map, so \
             every train here comes from the schedule alone.</p>\n"
                .to_string(),
        );
    }
    if items.is_empty() {
        return String::new();
    }
    format!(
        "<section class=\"notices\" aria-label=\"Service notices\">\n{}</section>\n",
        items.concat()
    )
}

/// Say an age in words, as the board's status line does.
fn ago(seconds: u64) -> String {
    if seconds < 10 {
        return "just now".to_string();
    }
    if seconds < 90 {
        return format!("{seconds} s ago");
    }
    let minutes = seconds / 60;
    if minutes < 90 {
        return format!("{minutes} min ago");
    }
    format!("{} h ago", minutes / 60)
}

/// Render the diagnostics as a list.
///
/// An unmatched layout station, an unplaceable run, and a stale feed
/// are all reported here rather than hidden: the page says what it
/// could not draw.
fn render_diagnostics(diagnostics: &[Diagnostic]) -> String {
    if diagnostics.is_empty() {
        return "<p class=\"no-diagnostics\">Nothing went unreported.</p>".to_string();
    }
    let mut out = String::from("<ul class=\"diagnostics\">\n");
    for diagnostic in diagnostics {
        out.push_str("<li><code>");
        out.push_str(&text(&diagnostic.code));
        out.push_str("</code> ");
        out.push_str(&text(&diagnostic.message));
        if let Some(subject) = &diagnostic.subject {
            out.push_str(" <span class=\"subject\">");
            out.push_str(&text(subject));
            out.push_str("</span>");
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f64, y: f64) -> ViewPoint {
        ViewPoint { x, y }
    }

    #[test]
    fn only_real_colors_reach_a_presentation_attribute() {
        // The GTFS form has no leading hash; the OpenFantasyMap form
        // has one. Both are colours.
        assert_eq!(feed_color("D42E12").as_deref(), Some("#D42E12"));
        assert_eq!(feed_color("#d42e12").as_deref(), Some("#d42e12"));
        assert_eq!(feed_color("#abc").as_deref(), Some("#abc"));
        // Nothing else is.
        assert_eq!(feed_color("red"), None);
        assert_eq!(feed_color("#12345"), None);
        assert_eq!(feed_color("\" onload=\"alert(1)"), None);
        assert_eq!(feed_color("#fff;}body{display:none"), None);
    }

    #[test]
    fn the_palette_answers_a_station_code() {
        assert_eq!(palette_color("NS1"), Some("#d42e12"));
        assert_eq!(palette_color("EW24"), Some("#009645"));
        assert_eq!(palette_color("PTC"), Some("#748477"));
        assert_eq!(palette_color("ZZ9"), None);
        assert_eq!(palette_color(""), None);
    }

    #[test]
    fn markup_characters_cannot_survive() {
        assert_eq!(
            text("<script>alert('x')&</script>"),
            "&lt;script&gt;alert(&#39;x&#39;)&amp;&lt;/script&gt;"
        );
        let escaped = json_island(r#"{"n":"</script>"}"#);
        assert!(!escaped.contains('<'));
        let parsed: serde_json::Value = serde_json::from_str(&escaped).unwrap();
        assert_eq!(parsed["n"], "</script>");
    }

    #[test]
    fn a_polyline_becomes_a_path() {
        assert_eq!(
            path_of(&[point(1.0, 2.0), point(3.5, 4.25)]),
            "M1 2 L3.5 4.25"
        );
        assert_eq!(path_of(&[]), "");
    }

    #[test]
    fn a_fraction_finds_a_point_on_a_polyline() {
        let points = [point(0.0, 0.0), point(10.0, 0.0), point(10.0, 10.0)];
        let cumulative = cumulative(&points);
        assert_eq!(cumulative, vec![0.0, 10.0, 20.0]);
        assert_eq!(point_at(&points, &cumulative, 0.0), point(0.0, 0.0));
        assert_eq!(point_at(&points, &cumulative, 0.25), point(5.0, 0.0));
        assert_eq!(point_at(&points, &cumulative, 0.75), point(10.0, 5.0));
        assert_eq!(point_at(&points, &cumulative, 1.0), point(10.0, 10.0));
        // A fraction outside the range clamps rather than extrapolates.
        assert_eq!(point_at(&points, &cumulative, 2.0), point(10.0, 10.0));
    }

    #[test]
    fn a_slice_keeps_the_vertices_between_its_ends() {
        let points = [point(0.0, 0.0), point(10.0, 0.0), point(10.0, 10.0)];
        let cumulative = cumulative(&points);
        let cut = slice(&points, &cumulative, 0.25, 0.75);
        assert_eq!(
            cut,
            vec![point(5.0, 0.0), point(10.0, 0.0), point(10.0, 5.0)]
        );
    }

    #[test]
    fn a_closed_polyline_is_recognised() {
        assert!(is_closed(&[
            point(0.0, 0.0),
            point(1.0, 1.0),
            point(0.0, 0.0)
        ]));
        assert!(!is_closed(&[point(0.0, 0.0), point(1.0, 1.0)]));
        assert!(!is_closed(&[]));
    }

    #[test]
    fn only_a_clean_origin_widens_the_policy() {
        // A relative snapshot URL is same-origin and adds nothing.
        assert_eq!(connect_extra("/api/map-snapshot"), "");
        assert_eq!(connect_extra("data/map.json"), "");
        // An absolute URL contributes its origin and nothing after it.
        assert_eq!(
            connect_extra("https://raw.example.com/live/map.json"),
            " https://raw.example.com"
        );
        assert_eq!(
            connect_extra("http://127.0.0.1:8601/api/map-snapshot"),
            " http://127.0.0.1:8601"
        );
        // A host that could escape the attribute or the policy is
        // dropped, not repaired.
        assert_eq!(connect_extra("https://evil.example'; script-src *"), "");
        assert_eq!(connect_extra("https://a\"b/x"), "");
        assert_eq!(connect_extra("https:///x"), "");
    }

    #[test]
    fn ages_read_as_words() {
        assert_eq!(ago(3), "just now");
        assert_eq!(ago(45), "45 s ago");
        assert_eq!(ago(600), "10 min ago");
        assert_eq!(ago(7200), "2 h ago");
    }

    #[test]
    fn coordinates_round_to_two_decimals() {
        assert_eq!(n(1.0), "1");
        assert_eq!(n(1.006), "1.01");
        assert_eq!(n(-0.001), "0");
        assert_eq!(n(123.456), "123.46");
    }
}
