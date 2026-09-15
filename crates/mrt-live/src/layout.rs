//! The schematic layout, and its binding to the rail network.
//!
//! The map is schematic, so the drawing is data. It is authored in
//! OpenFantasyMap, exported as GeoJSON, and committed to this
//! repository: `config/layout-mini.geojson` is the layout of the
//! miniature fixture network. [`Layout`] reads that export, and
//! [`Layout::bind`] joins its stations to the stations of a
//! [`RailNetwork`].
//!
//! # The export
//!
//! OpenFantasyMap round-trips its whole model through feature
//! `properties`. A line is a `LineString` with `ofm: "line"`, carrying
//! the editor's anchor `nodes`, its per-segment guides, the mode, the
//! colour, and the branch link. A station is a `Point` with
//! `ofm: "station"`, or a `Polygon` with `ofm: "station-area"`, and it
//! carries the name, the optional station `code`, and the arc-length
//! position `t` of the station along its line.
//!
//! # Binding by code, never by name
//!
//! Two stations of the real feed share the name `Bukit Panjang`, so a
//! name cannot identify a station. The layout carries the official
//! station code instead, and the binder resolves it through
//! [`RailNetwork::station_by_alias`], which accepts any spelling. A
//! layout station without a code cannot bind, and says so.
//!
//! # Coordinates
//!
//! The editor draws on a map, so it writes GeoJSON positions as
//! longitude and latitude. The schematic makes no geographic claim: it
//! reads the pair as a plain plane coordinate, `x` from the first
//! value and `y` from the second, and a renderer scales that plane to
//! its viewport.
//!
//! # No input, no output, no panic
//!
//! The reader takes the export as text or as a parsed value; the
//! caller reads the file. A feature the reader cannot use leaves a
//! [`Diagnostic`] behind and never a panic, and both directions of the
//! binding report what they could not match: a layout station that no
//! network station answers, and a network station that the layout
//! draws nowhere.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use mrt_gtfs::{alias, normalize_diagnostics, Diagnostic, RailNetwork, StationId};

// ----------------------------------------------------------------------
// The layout
// ----------------------------------------------------------------------

/// A position on the schematic plane.
///
/// The values come from a GeoJSON position, so `x` is what the editor
/// wrote as a longitude and `y` is what it wrote as a latitude. The
/// schematic treats them as plane coordinates and nothing else.
#[derive(Copy, Clone, Debug, PartialEq, Serialize)]
pub struct LayoutPoint {
    /// The horizontal coordinate.
    pub x: f64,
    /// The vertical coordinate.
    pub y: f64,
}

/// The link from a branch to the line it leaves.
#[derive(Debug, Clone, Serialize)]
pub struct LayoutBranch {
    /// The identifier of the parent layout line.
    pub line: String,
    /// The index of the parent node that the branch leaves from.
    pub node_index: Option<usize>,
}

/// One drawn line of the layout.
#[derive(Debug, Clone, Serialize)]
pub struct LayoutLine {
    /// The identifier that the editor gave the line. Stations name it,
    /// and a branch names its parent by it.
    pub id: String,
    /// The display name, for example `North South Line`.
    pub name: Option<String>,
    /// The editor's drawing mode, for example `Metro`. It selects the
    /// corner radius in the editor and carries no meaning here.
    pub mode: Option<String>,
    /// The line colour as the editor wrote it, usually `#rrggbb`. A
    /// renderer filters it before it reaches a stylesheet.
    pub color: Option<String>,
    /// Whether the editor had the line switched on.
    pub visible: bool,
    /// The line this one branches from, where it is a branch.
    pub branch_of: Option<LayoutBranch>,
    /// The drawn polyline, in draw order. These are the rendered
    /// points of the export, with the corners already rounded, so the
    /// arc-length position `t` of a station measures along them.
    pub points: Vec<LayoutPoint>,
}

/// One station of the layout.
#[derive(Debug, Clone, Serialize)]
pub struct LayoutStation {
    /// The identifier that the editor gave the station.
    pub id: String,
    /// The identifier of the layout line that the station sits on.
    pub line: String,
    /// The name as the layout carries it. The network name is the
    /// authoritative one; see [`BoundStation::name`].
    pub name: Option<String>,
    /// The official station code, the key that binds the station to
    /// the network. A station without one cannot bind.
    pub code: Option<String>,
    /// The position of the station along its line, from 0 to 1 of the
    /// drawn arc length.
    pub t: Option<f64>,
    /// The anchor of the station on the plane. For a station area it
    /// is the centroid of the drawn ring, as the editor's own import
    /// reads it.
    pub point: LayoutPoint,
    /// The drawn ring of a station area, in draw order and without the
    /// repeated closing position. The editor also writes the same ring
    /// as metre offsets from the anchor; the ring is already in the
    /// plane, so the layout keeps that form.
    pub area: Option<Vec<LayoutPoint>>,
}

/// The schematic drawing: the lines, the stations, and what the reader
/// could not use.
///
/// # Example
///
/// ```no_run
/// use mrt_gtfs::{GtfsFeed, RailNetwork};
/// use mrt_live::Layout;
///
/// let network = RailNetwork::from_feed(&GtfsFeed::from_dir("feed").unwrap()).unwrap();
/// let text = std::fs::read_to_string("config/layout-mini.geojson").unwrap();
/// let bound = Layout::from_geojson_str(&text).unwrap().bind(&network);
/// for station in &bound.uncovered {
///     println!("the layout draws no {}", station.name);
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct Layout {
    /// The lines, in file order.
    pub lines: Vec<LayoutLine>,
    /// The stations, in file order.
    pub stations: Vec<LayoutStation>,
    /// The number of features that carry no OpenFantasyMap kind this
    /// reader knows. They are counted, reported, and drawn by nothing.
    pub unknown_features: usize,
    /// Everything the reader could not use.
    pub diagnostics: Vec<Diagnostic>,
}

/// An error that stops the layout from being read at all.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LayoutError {
    /// The layout file is not valid JSON.
    #[error("the layout is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

impl Layout {
    /// Read a layout from the text of an OpenFantasyMap GeoJSON export.
    ///
    /// The function fails only when the text is not JSON. Everything
    /// else — a missing property, a geometry with too few positions, a
    /// feature from another tool — is a diagnostic on the layout it
    /// returns.
    pub fn from_geojson_str(text: &str) -> Result<Layout, LayoutError> {
        let value: Value = serde_json::from_str(text)?;
        Ok(Layout::from_geojson(&value))
    }

    /// Read a layout from a parsed GeoJSON value.
    ///
    /// The function never fails and never panics. A value that is not
    /// a feature collection produces an empty layout with one error
    /// diagnostic.
    pub fn from_geojson(value: &Value) -> Layout {
        let mut layout = Layout {
            lines: Vec::new(),
            stations: Vec::new(),
            unknown_features: 0,
            diagnostics: Vec::new(),
        };
        if value.get("type").and_then(Value::as_str) != Some("FeatureCollection") {
            layout.diagnostics.push(Diagnostic::error(
                "layout-not-a-feature-collection",
                "the layout is not a GeoJSON FeatureCollection, so it carries no line \
                 and no station",
            ));
            return layout;
        }
        let Some(features) = value.get("features").and_then(Value::as_array) else {
            layout.diagnostics.push(Diagnostic::error(
                "layout-without-features",
                "the feature collection carries no features array, so the layout is empty",
            ));
            return layout;
        };

        for feature in features {
            let properties = feature.get("properties").unwrap_or(&Value::Null);
            let geometry = feature.get("geometry").unwrap_or(&Value::Null);
            match properties.get("ofm").and_then(Value::as_str) {
                Some("line") => layout.read_line(properties, geometry),
                Some("station") | Some("station-area") => layout.read_station(properties, geometry),
                _ => layout.unknown_features += 1,
            }
        }

        if layout.unknown_features > 0 {
            layout.diagnostics.push(Diagnostic::warning(
                "layout-unknown-feature",
                format!(
                    "{} feature(s) of the layout are neither an OpenFantasyMap line nor a \
                     station, so the layout draws nothing for them",
                    layout.unknown_features
                ),
            ));
        }
        layout.check_station_lines();
        normalize_diagnostics(&mut layout.diagnostics);
        layout
    }

    /// Get one line by its layout identifier.
    pub fn line(&self, id: &str) -> Option<&LayoutLine> {
        self.lines.iter().find(|line| line.id == id)
    }

    /// Read one `ofm: "line"` feature.
    fn read_line(&mut self, properties: &Value, geometry: &Value) {
        let Some(id) = identifier(properties) else {
            self.diagnostics.push(Diagnostic::warning(
                "layout-line-without-id",
                "a line feature of the layout carries no identifier, so no station can \
                 name it and the layout drops it",
            ));
            return;
        };
        if self.lines.iter().any(|line| line.id == id) {
            self.diagnostics.push(
                Diagnostic::warning(
                    "layout-duplicate-line",
                    "two line features of the layout share one identifier, so the layout \
                     keeps the first and drops the second",
                )
                .about(id),
            );
            return;
        }
        if geometry.get("type").and_then(Value::as_str) != Some("LineString") {
            self.diagnostics.push(
                Diagnostic::warning(
                    "layout-line-without-geometry",
                    "the geometry of this line feature is not a LineString, so the layout \
                     cannot draw it",
                )
                .about(id),
            );
            return;
        }
        let points = positions(geometry.get("coordinates"));
        if points.len() < 2 {
            self.diagnostics.push(
                Diagnostic::warning(
                    "layout-line-without-geometry",
                    "this line feature carries fewer than two usable positions, so the \
                     layout cannot draw it",
                )
                .about(id),
            );
            return;
        }
        let name = text(properties, "name");
        if name.is_none() {
            self.diagnostics.push(
                Diagnostic::info(
                    "layout-line-without-name",
                    "this line feature carries no name, so a renderer has no label for it",
                )
                .about(id.clone()),
            );
        }
        self.lines.push(LayoutLine {
            id,
            name,
            mode: text(properties, "mode"),
            color: text(properties, "color"),
            // The editor writes `visible: false` for a line it has
            // switched off, and the property is absent in older files.
            visible: properties.get("visible").and_then(Value::as_bool) != Some(false),
            branch_of: branch(properties.get("branchOf")),
            points,
        });
    }

    /// Read one `ofm: "station"` or `ofm: "station-area"` feature.
    fn read_station(&mut self, properties: &Value, geometry: &Value) {
        let Some(id) = identifier(properties) else {
            self.diagnostics.push(Diagnostic::warning(
                "layout-station-without-id",
                "a station feature of the layout carries no identifier, so the layout \
                 cannot report on it and drops it",
            ));
            return;
        };
        let placed = match geometry.get("type").and_then(Value::as_str) {
            Some("Point") => position(geometry.get("coordinates")).map(|point| (point, None)),
            Some("Polygon") => {
                let ring = ring(geometry.get("coordinates"));
                centroid(&ring).map(|point| (point, Some(ring)))
            }
            _ => None,
        };
        let Some((point, area)) = placed else {
            self.diagnostics.push(
                Diagnostic::warning(
                    "layout-station-without-position",
                    "the geometry of this station feature is not a usable Point or \
                     Polygon, so the layout cannot place it",
                )
                .about(id),
            );
            return;
        };

        let code = text(properties, "code");
        if code.is_none() {
            self.diagnostics.push(
                Diagnostic::warning(
                    "layout-station-without-code",
                    "this station feature carries no station code, so it cannot bind to a \
                     network station",
                )
                .about(id.clone()),
            );
        }
        let t = properties.get("t").and_then(Value::as_f64);
        if t.is_none() {
            self.diagnostics.push(
                Diagnostic::info(
                    "layout-station-without-t",
                    "this station feature carries no arc position, so a renderer places it \
                     by its anchor alone",
                )
                .about(id.clone()),
            );
        }
        self.stations.push(LayoutStation {
            id,
            line: text(properties, "lineId").unwrap_or_default(),
            name: text(properties, "name"),
            code,
            t,
            point,
            area,
        });
    }

    /// Report every station that names a line the layout does not
    /// carry.
    fn check_station_lines(&mut self) {
        for station in &self.stations {
            if self.lines.iter().any(|line| line.id == station.line) {
                continue;
            }
            self.diagnostics.push(
                Diagnostic::warning(
                    "layout-station-without-line",
                    format!(
                        "this station names the layout line \"{}\", which the layout does \
                         not carry, so a renderer has no polyline to place it on",
                        station.line
                    ),
                )
                .about(station.id.clone()),
            );
        }
    }

    /// Join the layout to a network, and report both directions of
    /// what did not match.
    ///
    /// See [`BoundLayout`].
    pub fn bind(self, network: &RailNetwork) -> BoundLayout {
        BoundLayout::bind(self, network)
    }
}

// ----------------------------------------------------------------------
// The binding
// ----------------------------------------------------------------------

/// One layout station joined to a network station.
#[derive(Debug, Clone, Serialize)]
pub struct BoundStation {
    /// The identifier of the layout station.
    pub layout_station: String,
    /// The identifier of the layout line it sits on.
    pub layout_line: String,
    /// The code that bound it, as the layout spells it.
    pub code: String,
    /// The network station.
    pub station: StationId,
    /// The name of the network station. It is the authoritative one:
    /// the layout may spell its own label differently.
    pub name: String,
}

/// Why a layout station bound to nothing.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnmatchedReason {
    /// The layout station carries no code, so there is nothing to
    /// resolve.
    NoCode,
    /// The layout station carries a code that no network station
    /// answers to.
    UnknownCode,
}

/// A layout station that no network station answers.
#[derive(Debug, Clone, Serialize)]
pub struct UnmatchedStation {
    /// The identifier of the layout station.
    pub layout_station: String,
    /// The identifier of the layout line it sits on.
    pub layout_line: String,
    /// The name the layout carries for it.
    pub name: Option<String>,
    /// The code the layout carries for it, where it carries one.
    pub code: Option<String>,
    /// Why it bound to nothing.
    pub reason: UnmatchedReason,
}

/// A network station that the layout draws nowhere.
#[derive(Debug, Clone, Serialize)]
pub struct UncoveredStation {
    /// The network station.
    pub station: StationId,
    /// The public name of the station.
    pub name: String,
    /// The public station codes of the station.
    pub codes: Vec<String>,
}

/// A layout joined to a network.
///
/// The join is by station code, in both directions. Every layout
/// station is either in [`BoundLayout::stations`] or in
/// [`BoundLayout::unmatched`], and every network station that carries
/// a code is either covered by a layout station or in
/// [`BoundLayout::uncovered`]. Nothing is dropped in silence: each
/// list has its diagnostic, so a page can show the gap.
#[derive(Debug, Clone, Serialize)]
pub struct BoundLayout {
    /// The layout as it was read.
    pub layout: Layout,
    /// The bound stations, in layout order.
    pub stations: Vec<BoundStation>,
    /// The layout stations that bound to nothing, in layout order.
    pub unmatched: Vec<UnmatchedStation>,
    /// The network stations that carry a code and that no layout
    /// station covers, in station order.
    pub uncovered: Vec<UncoveredStation>,
    /// Everything the layout reported, plus everything the binding
    /// could not match.
    pub diagnostics: Vec<Diagnostic>,
}

impl BoundLayout {
    /// Join a layout to a network.
    pub fn bind(layout: Layout, network: &RailNetwork) -> BoundLayout {
        let mut diagnostics = layout.diagnostics.clone();
        let mut stations = Vec::new();
        let mut unmatched = Vec::new();
        // The comparison key of every code the layout has already
        // bound, and the layout station that bound it.
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        let mut covered: Vec<bool> = vec![false; network.stations().len()];

        for station in &layout.stations {
            let Some(code) = station.code.clone() else {
                unmatched.push(UnmatchedStation {
                    layout_station: station.id.clone(),
                    layout_line: station.line.clone(),
                    name: station.name.clone(),
                    code: None,
                    reason: UnmatchedReason::NoCode,
                });
                continue;
            };
            let Some(id) = network.station_by_alias(&code) else {
                diagnostics.push(
                    Diagnostic::warning(
                        "layout-station-unmatched",
                        format!(
                            "no network station answers to the code \"{code}\", so the \
                             layout draws a station the network does not have"
                        ),
                    )
                    .about(station.id.clone()),
                );
                unmatched.push(UnmatchedStation {
                    layout_station: station.id.clone(),
                    layout_line: station.line.clone(),
                    name: station.name.clone(),
                    code: Some(code),
                    reason: UnmatchedReason::UnknownCode,
                });
                continue;
            };
            if let Some(first) = seen.insert(alias::normalize(&code), station.id.clone()) {
                diagnostics.push(
                    Diagnostic::warning(
                        "layout-duplicate-code",
                        format!(
                            "the layout station \"{first}\" already carries the code \
                             \"{code}\", so the network station is drawn twice"
                        ),
                    )
                    .about(station.id.clone()),
                );
            }
            covered[id.0] = true;
            stations.push(BoundStation {
                layout_station: station.id.clone(),
                layout_line: station.line.clone(),
                code,
                station: id,
                name: network.station(id).name.clone(),
            });
        }

        let mut uncovered = Vec::new();
        for (index, station) in network.stations().iter().enumerate() {
            if covered[index] {
                continue;
            }
            if station.codes.is_empty() {
                diagnostics.push(
                    Diagnostic::info(
                        "network-station-without-code",
                        "this network station carries no station code, so no layout \
                         station can bind to it",
                    )
                    .about(station.gtfs_id.clone()),
                );
                continue;
            }
            diagnostics.push(
                Diagnostic::warning(
                    "network-station-uncovered",
                    format!(
                        "the layout draws no station for \"{}\" ({}), so the network \
                         reaches a place the map does not show",
                        station.name,
                        station.codes.join(", ")
                    ),
                )
                .about(station.gtfs_id.clone()),
            );
            uncovered.push(UncoveredStation {
                station: StationId(index),
                name: station.name.clone(),
                codes: station.codes.clone(),
            });
        }

        normalize_diagnostics(&mut diagnostics);
        BoundLayout {
            layout,
            stations,
            unmatched,
            uncovered,
            diagnostics,
        }
    }

    /// Get the network station that one layout station binds to.
    pub fn station(&self, layout_station: &str) -> Option<StationId> {
        self.stations
            .iter()
            .find(|bound| bound.layout_station == layout_station)
            .map(|bound| bound.station)
    }

    /// Report whether every station matched, in both directions.
    pub fn is_complete(&self) -> bool {
        self.unmatched.is_empty() && self.uncovered.is_empty()
    }
}

// ----------------------------------------------------------------------
// GeoJSON reading
// ----------------------------------------------------------------------

/// Read a non-empty string property.
fn text(properties: &Value, key: &str) -> Option<String> {
    properties
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Read the identifier of a feature.
fn identifier(properties: &Value) -> Option<String> {
    text(properties, "id")
}

/// Read the branch link of a line.
fn branch(value: Option<&Value>) -> Option<LayoutBranch> {
    let value = value?;
    let line = text(value, "lineId")?;
    Some(LayoutBranch {
        line,
        node_index: value
            .get("nodeIndex")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok()),
    })
}

/// Read one GeoJSON position.
///
/// A position with fewer than two numbers, or with a value that is not
/// a number, is not a position.
fn position(value: Option<&Value>) -> Option<LayoutPoint> {
    let array = value?.as_array()?;
    let x = array.first()?.as_f64()?;
    let y = array.get(1)?.as_f64()?;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some(LayoutPoint { x, y })
}

/// Read an array of GeoJSON positions, dropping the ones that are not
/// positions.
fn positions(value: Option<&Value>) -> Vec<LayoutPoint> {
    let Some(array) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|item| position(Some(item)))
        .collect()
}

/// Read the outer ring of a polygon, without the repeated closing
/// position.
fn ring(value: Option<&Value>) -> Vec<LayoutPoint> {
    let mut points = positions(
        value
            .and_then(Value::as_array)
            .and_then(|rings| rings.first()),
    );
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    points
}

/// Get the centroid of a ring, the anchor of a station area.
///
/// The editor's own import reads a station area the same way.
fn centroid(points: &[LayoutPoint]) -> Option<LayoutPoint> {
    if points.is_empty() {
        return None;
    }
    let count = points.len() as f64;
    Some(LayoutPoint {
        x: points.iter().map(|point| point.x).sum::<f64>() / count,
        y: points.iter().map(|point| point.y).sum::<f64>() / count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_needs_two_numbers() {
        let two: Value = serde_json::json!([103.8, 1.3]);
        assert_eq!(position(Some(&two)), Some(LayoutPoint { x: 103.8, y: 1.3 }));
        let one: Value = serde_json::json!([103.8]);
        assert_eq!(position(Some(&one)), None);
        let text: Value = serde_json::json!(["103.8", "1.3"]);
        assert_eq!(position(Some(&text)), None);
        assert_eq!(position(None), None);
    }

    #[test]
    fn a_ring_drops_its_closing_position() {
        let value: Value = serde_json::json!([[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 0.0]]]);
        let ring = ring(Some(&value));
        assert_eq!(ring.len(), 3);
        assert_eq!(
            centroid(&ring),
            Some(LayoutPoint {
                x: 4.0 / 3.0,
                y: 2.0 / 3.0
            })
        );
    }

    #[test]
    fn an_empty_ring_has_no_centroid() {
        assert_eq!(centroid(&[]), None);
    }
}
