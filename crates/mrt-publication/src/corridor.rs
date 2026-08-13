//! Corridor resolution for the string diagram.
//!
//! A string diagram needs one vertical axis of stations. A line does
//! not always give one: a branch leaves the main line, and a loop
//! visits the same station twice. This module turns the stop patterns
//! of a line into an explicit corridor and decides, without guessing,
//! which runs can be drawn on it.
//!
//! # The identity problem
//!
//! A [`StationId`] cannot name a position on the axis, because an
//! unrolled loop contains the same station more than once. A
//! [`CorridorNode`] therefore carries the station *and* an occurrence
//! index, and every mapping works on node indexes.
//!
//! # How the axis is chosen
//!
//! 1. When the configuration names a corridor, that axis wins.
//! 2. Otherwise the longest pattern of the selected trips becomes the
//!    spine. A pattern that is a subsequence of the spine, forwards or
//!    backwards, joins it. A pattern that contains the spine replaces
//!    it.
//! 3. A pattern that fits neither way starts a new panel. The
//!    projection reports it and never forces it onto the main axis.

use std::collections::BTreeMap;

use mrt_gtfs::{Diagnostic, LineId, RailNetwork, StationId, StopPattern};
use serde::Serialize;

use crate::common::StationView;
use crate::config::{BranchConfig, CorridorConfig, PublicationConfig, StationSpacing};
use crate::error::PublicationError;
use crate::text::{Labels, Language};

/// One position on the vertical axis of a diagram.
#[derive(Clone, Debug, Serialize)]
pub struct CorridorNode {
    /// A stable key, `<station gtfs id>#<occurrence>`.
    pub key: String,
    /// The station at this position.
    pub station: StationView,
    /// How often the station has already appeared on this axis,
    /// starting at zero.
    ///
    /// An unrolled loop repeats a station, and so does a station that
    /// a branch panel shares with the main line. The count runs across
    /// the whole corridor, so `key` names exactly one position.
    pub occurrence: u16,
    /// The vertical coordinate in user units.
    pub y: f64,
    /// The distance from the first node of its panel, in metres, when
    /// the feed supplies positions.
    pub cumulative_distance: Option<f64>,
    /// The panel that this node belongs to.
    pub panel: usize,
}

/// A block of the axis with its own heading.
///
/// A simple line has one panel. A branch, or a group of patterns that
/// no single axis can hold, gets its own panel below the main one.
#[derive(Clone, Debug, Serialize)]
pub struct CorridorPanel {
    /// A stable key.
    pub id: String,
    /// The heading of the panel.
    pub label: String,
    /// The index of the first node of the panel.
    pub first_node: usize,
    /// The index of the last node of the panel.
    pub last_node: usize,
}

/// The vertical axis of a diagram.
#[derive(Clone, Debug, Serialize)]
pub struct Corridor {
    /// The identifier of the corridor.
    pub id: String,
    /// The heading of the corridor.
    pub label: String,
    /// Every node of the axis, from top to bottom.
    pub nodes: Vec<CorridorNode>,
    /// The blocks of the axis.
    pub panels: Vec<CorridorPanel>,
    /// The total height of the axis in user units.
    pub height: f64,
    /// Whether the vertical positions follow the distance between the
    /// stations. `false` means equal spacing.
    pub spaced_by_distance: bool,
}

impl Corridor {
    /// Get the node at an index.
    pub fn node(&self, index: usize) -> &CorridorNode {
        &self.nodes[index]
    }
}

/// A path through the corridor that a run may follow.
///
/// The main axis is one path. Each branch adds the path that runs
/// along the main axis up to the junction and then along the branch.
#[derive(Clone, Debug)]
struct Path {
    nodes: Vec<usize>,
    stations: Vec<StationId>,
}

/// A resolved corridor with everything needed to map runs onto it.
#[derive(Clone, Debug)]
pub struct CorridorPlan {
    /// The axis.
    pub corridor: Corridor,
    /// The paths that a run may follow.
    paths: Vec<Path>,
    /// What the resolver had to report.
    pub diagnostics: Vec<Diagnostic>,
}

/// Which way a run travels along the corridor.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AxisDirection {
    /// The run follows the axis from top to bottom.
    Down,
    /// The run follows the axis from bottom to top.
    Up,
}

/// The result of mapping the calls of one run onto the corridor.
#[derive(Clone, Debug)]
pub struct RunMapping {
    /// One node index per call of the run.
    pub nodes: Vec<usize>,
    /// Which way the run travels.
    pub direction: AxisDirection,
}

impl CorridorPlan {
    /// Map the calls of one run onto the corridor.
    ///
    /// Returns `None` when no path of the corridor contains the calls
    /// in order, for example when a loop run traverses the axis more
    /// often than the corridor was unrolled for.
    pub fn map_run(&self, stations: &[StationId]) -> Option<RunMapping> {
        if stations.len() < 2 {
            return None;
        }
        for path in &self.paths {
            if let Some(indexes) = match_forward(&path.stations, stations) {
                return Some(RunMapping {
                    nodes: indexes.into_iter().map(|i| path.nodes[i]).collect(),
                    direction: AxisDirection::Down,
                });
            }
            if let Some(indexes) = match_backward(&path.stations, stations) {
                return Some(RunMapping {
                    nodes: indexes.into_iter().map(|i| path.nodes[i]).collect(),
                    direction: AxisDirection::Up,
                });
            }
        }
        None
    }
}

/// Which trips a diagram draws.
#[derive(Clone, Debug)]
pub enum DiagramTarget {
    /// Every pattern of one line.
    Line(LineId),
    /// One stop pattern.
    Pattern(mrt_gtfs::PatternId),
    /// A corridor from the configuration.
    Corridor(String),
}

/// Build the corridor for a diagram target.
pub fn resolve_corridor(
    network: &RailNetwork,
    target: &DiagramTarget,
    config: &PublicationConfig,
) -> Result<CorridorPlan, PublicationError> {
    let labels = Labels::for_language(config.language);
    match target {
        DiagramTarget::Corridor(id) => {
            let definition = config.corridor(id).ok_or_else(|| {
                PublicationError::UnresolvedCorridor(format!(
                    "the configuration defines no corridor \"{id}\""
                ))
            })?;
            from_config(network, definition, config, labels)
        }
        DiagramTarget::Pattern(pattern) => {
            let stations = network.pattern(*pattern).stations.clone();
            let label = corridor_label_from_stations(network, &stations);
            Ok(assemble(
                network,
                &format!("pattern-{}", pattern.0),
                &label,
                vec![Group {
                    label: label.clone(),
                    stations,
                    is_branch_of: None,
                }],
                config,
                Vec::new(),
            ))
        }
        DiagramTarget::Line(line) => {
            let patterns: Vec<&StopPattern> = network.patterns_for_line(*line).collect();
            if patterns.is_empty() {
                return Err(PublicationError::UnresolvedCorridor(format!(
                    "the line \"{}\" has no stop pattern",
                    network.line(*line).name
                )));
            }
            let (groups, diagnostics) = derive_groups(network, &patterns, labels);
            let line_name = network.line(*line).name.clone();
            Ok(assemble(
                network,
                &crate::common::css_key(&network.line(*line).route_id),
                &line_name,
                groups,
                config,
                diagnostics,
            ))
        }
    }
}

/// One block of the axis before the coordinates are known.
struct Group {
    label: String,
    stations: Vec<StationId>,
    /// The node index on the main axis where a branch starts.
    is_branch_of: Option<usize>,
}

/// Build a corridor from an explicit configuration.
fn from_config(
    network: &RailNetwork,
    definition: &CorridorConfig,
    config: &PublicationConfig,
    labels: &Labels,
) -> Result<CorridorPlan, PublicationError> {
    let resolve = |code: &str| -> Result<StationId, PublicationError> {
        network
            .station_by_alias(code)
            .or_else(|| network.station_by_code(code))
            .or_else(|| network.station_by_gtfs_id(code))
            .or_else(|| network.station_by_name(code))
            .ok_or_else(|| {
                PublicationError::UnresolvedStation(format!(
                    "the corridor \"{}\" names the station \"{code}\", which is not in the feed",
                    definition.id
                ))
            })
    };

    let main: Vec<StationId> = definition
        .axis
        .iter()
        .map(|code| resolve(code))
        .collect::<Result<_, _>>()?;
    let label = definition
        .label
        .as_ref()
        .map(|text| text.get(config.language).to_string())
        .or_else(|| {
            config
                .labels
                .corridor_overrides
                .get(&definition.id)
                .map(|text| text.get(config.language).to_string())
        })
        .unwrap_or_else(|| definition.id.clone());

    let mut groups = vec![Group {
        label: label.clone(),
        stations: main.clone(),
        is_branch_of: None,
    }];
    for branch in &definition.branches {
        let junction = resolve(&branch.junction)?;
        let junction_index = main.iter().position(|s| *s == junction).ok_or_else(|| {
            PublicationError::UnresolvedCorridor(format!(
                "the junction \"{}\" is not on the axis of corridor \"{}\"",
                branch.junction, definition.id
            ))
        })?;
        let stations: Vec<StationId> = branch
            .axis
            .iter()
            .map(|code| resolve(code))
            .collect::<Result<_, _>>()?;
        groups.push(Group {
            label: branch_label(branch, network, &stations, config.language, labels),
            stations,
            is_branch_of: Some(junction_index),
        });
    }
    Ok(assemble(
        network,
        &definition.id,
        &label,
        groups,
        config,
        Vec::new(),
    ))
}

fn branch_label(
    branch: &BranchConfig,
    network: &RailNetwork,
    stations: &[StationId],
    language: Language,
    labels: &Labels,
) -> String {
    if let Some(text) = &branch.label {
        return text.get(language).to_string();
    }
    match stations.last() {
        Some(&last) => labels.towards(&network.station(last).name),
        None => branch.junction.clone(),
    }
}

/// Derive the axis groups from the patterns of a line.
fn derive_groups(
    network: &RailNetwork,
    patterns: &[&StopPattern],
    labels: &Labels,
) -> (Vec<Group>, Vec<Diagnostic>) {
    // Deterministic order: the longest pattern first, then direction 0
    // before direction 1, then the station identifiers.
    let mut ordered: Vec<&&StopPattern> = patterns.iter().collect();
    ordered.sort_by(|a, b| {
        b.stations
            .len()
            .cmp(&a.stations.len())
            .then_with(|| {
                a.direction
                    .unwrap_or(u8::MAX)
                    .cmp(&b.direction.unwrap_or(u8::MAX))
            })
            .then_with(|| a.stations.cmp(&b.stations))
    });

    let mut spines: Vec<Vec<StationId>> = Vec::new();
    let mut rejected: Vec<&StopPattern> = Vec::new();
    for pattern in ordered {
        let forward = pattern.stations.clone();
        let backward: Vec<StationId> = forward.iter().rev().copied().collect();
        let mut placed = false;
        for spine in spines.iter_mut() {
            if is_subsequence(spine, &forward) || is_subsequence(spine, &backward) {
                placed = true;
                break;
            }
            if is_subsequence(&forward, spine) {
                *spine = forward.clone();
                placed = true;
                break;
            }
            if is_subsequence(&backward, spine) {
                *spine = backward.clone();
                placed = true;
                break;
            }
        }
        if !placed {
            if spines.is_empty() {
                spines.push(forward);
            } else {
                rejected.push(pattern);
                spines.push(forward);
            }
        }
    }

    let mut diagnostics = Vec::new();
    if spines.len() > 1 {
        diagnostics.push(Diagnostic::warning(
            "corridor-split",
            format!(
                "{} stop pattern group(s) do not fit on one station axis, so the diagram \
                 draws them in separate panels; define a corridor in the configuration to \
                 place them on one axis",
                spines.len()
            ),
        ));
        for pattern in rejected {
            diagnostics.push(Diagnostic::info(
                "corridor-separate-panel",
                format!(
                    "the pattern {} needs its own panel",
                    describe_pattern(network, pattern)
                ),
            ));
        }
    }

    let groups = spines
        .into_iter()
        .map(|stations| Group {
            label: corridor_label(network, &stations, labels),
            stations,
            is_branch_of: None,
        })
        .collect();
    (groups, diagnostics)
}

fn describe_pattern(network: &RailNetwork, pattern: &StopPattern) -> String {
    let first = pattern
        .stations
        .first()
        .map(|&id| network.station(id).name.as_str())
        .unwrap_or("?");
    let last = pattern
        .stations
        .last()
        .map(|&id| network.station(id).name.as_str())
        .unwrap_or("?");
    format!("{first} \u{2192} {last}")
}

fn corridor_label(network: &RailNetwork, stations: &[StationId], labels: &Labels) -> String {
    let _ = labels;
    corridor_label_from_stations(network, stations)
}

fn corridor_label_from_stations(network: &RailNetwork, stations: &[StationId]) -> String {
    match (stations.first(), stations.last()) {
        (Some(&first), Some(&last)) => format!(
            "{} \u{2013} {}",
            network.station(first).name,
            network.station(last).name
        ),
        _ => String::new(),
    }
}

/// Turn the groups into a corridor with coordinates and paths.
fn assemble(
    network: &RailNetwork,
    id: &str,
    label: &str,
    groups: Vec<Group>,
    config: &PublicationConfig,
    mut diagnostics: Vec<Diagnostic>,
) -> CorridorPlan {
    /// The vertical gap between two panels, in row heights.
    const PANEL_GAP_ROWS: f64 = 1.5;

    let row_height = config.diagram.row_height;
    let want_distance = config.diagram.station_spacing == StationSpacing::Distance;
    let mut spaced_by_distance = want_distance;
    let mut nodes: Vec<CorridorNode> = Vec::new();
    let mut panels: Vec<CorridorPanel> = Vec::new();
    let mut paths: Vec<Path> = Vec::new();
    let mut main_nodes: Vec<usize> = Vec::new();
    let mut main_stations: Vec<StationId> = Vec::new();
    let mut y = 0.0f64;
    // The counter runs across the whole corridor, not per panel, so
    // that a station a branch shares with the main line gets its own
    // node key rather than a duplicate of it.
    let mut occurrences: BTreeMap<StationId, u16> = BTreeMap::new();

    for (panel_index, group) in groups.iter().enumerate() {
        if group.stations.is_empty() {
            continue;
        }
        // The measured distance always reaches the node, because the
        // hover details show it. Only the spacing mode decides whether
        // it also drives the vertical positions.
        let distances = network.cumulative_station_distance(&group.stations);
        let usable = distances
            .as_ref()
            .is_some_and(|d| d.last().copied().unwrap_or(0.0) > 0.0);
        if want_distance && !usable {
            spaced_by_distance = false;
        }
        let offsets = panel_offsets(
            &distances,
            row_height,
            group.stations.len(),
            want_distance && usable,
        );
        let first_node = nodes.len();
        let mut group_nodes = Vec::with_capacity(group.stations.len());
        for (index, &station) in group.stations.iter().enumerate() {
            let occurrence = occurrences.entry(station).or_insert(0);
            let view = StationView::of(network, station);
            nodes.push(CorridorNode {
                key: format!("{}#{}", view.gtfs_id, *occurrence),
                station: view,
                occurrence: *occurrence,
                y: crate::common::round2(y + offsets[index]),
                cumulative_distance: distances.as_ref().map(|d| crate::common::round2(d[index])),
                panel: panel_index,
            });
            *occurrence += 1;
            group_nodes.push(nodes.len() - 1);
        }
        let last_node = nodes.len() - 1;
        panels.push(CorridorPanel {
            id: format!("{id}-p{panel_index}"),
            label: group.label.clone(),
            first_node,
            last_node,
        });
        y = nodes[last_node].y + row_height * PANEL_GAP_ROWS;

        match group.is_branch_of {
            None if panel_index == 0 => {
                main_nodes = group_nodes.clone();
                main_stations = group.stations.clone();
                paths.push(Path {
                    nodes: group_nodes,
                    stations: group.stations.clone(),
                });
            }
            None => paths.push(Path {
                nodes: group_nodes,
                stations: group.stations.clone(),
            }),
            Some(junction) => {
                // The branch path runs along the main axis to the
                // junction and then along the branch.
                let mut path_nodes = main_nodes[..=junction].to_vec();
                let mut path_stations = main_stations[..=junction].to_vec();
                path_nodes.extend(group_nodes.iter().copied());
                path_stations.extend(group.stations.iter().copied());
                paths.push(Path {
                    nodes: path_nodes,
                    stations: path_stations,
                });
            }
        }
    }

    // Longer paths first, so a branch run matches the branch path
    // rather than the shorter main-axis prefix.
    paths.sort_by_key(|path| std::cmp::Reverse(path.stations.len()));

    if want_distance && !spaced_by_distance {
        diagnostics.push(Diagnostic::warning(
            "distance-spacing-unavailable",
            "the feed carries no usable station positions for this corridor, so the \
             diagram falls back to equal station spacing",
        ));
    }

    let height = nodes.last().map(|n| n.y).unwrap_or(0.0);
    CorridorPlan {
        corridor: Corridor {
            id: id.to_string(),
            label: label.to_string(),
            nodes,
            panels,
            height: crate::common::round2(height),
            spaced_by_distance,
        },
        paths,
        diagnostics,
    }
}

/// Turn the distances into vertical offsets inside one panel.
///
/// With `use_distance` off, or with degenerate distances, the panel
/// falls back to equal spacing, which is what most diagrams want.
fn panel_offsets(
    distances: &Option<Vec<f64>>,
    row_height: f64,
    count: usize,
    use_distance: bool,
) -> Vec<f64> {
    let equal: Vec<f64> = (0..count).map(|i| i as f64 * row_height).collect();
    if !use_distance {
        return equal;
    }
    let Some(values) = distances else {
        return equal;
    };
    let total = values.last().copied().unwrap_or(0.0);
    if total <= 0.0 || values.len() != count {
        return equal;
    }
    // Keep the same overall height as equal spacing, so a page laid
    // out for one mode stays laid out for the other.
    let span = (count.saturating_sub(1)) as f64 * row_height;
    values.iter().map(|d| d / total * span).collect()
}

/// Report whether `needle` appears inside `haystack` in order.
///
/// The scan is greedy from the left, which is optimal for a
/// subsequence test and handles a station that repeats in a loop.
fn is_subsequence(haystack: &[StationId], needle: &[StationId]) -> bool {
    match_forward(haystack, needle).is_some()
}

/// Match the calls of a run against a path, from top to bottom.
///
/// Returns the index in `haystack` of each entry of `needle`.
fn match_forward(haystack: &[StationId], needle: &[StationId]) -> Option<Vec<usize>> {
    let mut out = Vec::with_capacity(needle.len());
    let mut cursor = 0usize;
    for wanted in needle {
        let found = haystack[cursor..].iter().position(|s| s == wanted)?;
        out.push(cursor + found);
        cursor += found + 1;
    }
    Some(out)
}

/// Match the calls of a run against a path, from bottom to top.
fn match_backward(haystack: &[StationId], needle: &[StationId]) -> Option<Vec<usize>> {
    let reversed: Vec<StationId> = haystack.iter().rev().copied().collect();
    let indexes = match_forward(&reversed, needle)?;
    Some(
        indexes
            .into_iter()
            .map(|i| haystack.len() - 1 - i)
            .collect(),
    )
}

/// Get the spacing mode that the corridor really used.
///
/// The configured mode is a wish. A feed without usable station
/// positions turns [`StationSpacing::Distance`] into
/// [`StationSpacing::Equal`], and the document states which one it got.
pub fn effective_spacing(corridor: &Corridor, config: &PublicationConfig) -> StationSpacing {
    match config.diagram.station_spacing {
        StationSpacing::Distance if !corridor.spaced_by_distance => StationSpacing::Equal,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[usize]) -> Vec<StationId> {
        values.iter().map(|&v| StationId(v)).collect()
    }

    #[test]
    fn a_subsequence_matches_in_order() {
        let spine = ids(&[1, 2, 3, 4, 5]);
        assert_eq!(match_forward(&spine, &ids(&[1, 3, 5])), Some(vec![0, 2, 4]));
        assert_eq!(match_forward(&spine, &ids(&[2, 4])), Some(vec![1, 3]));
        assert_eq!(match_forward(&spine, &ids(&[3, 2])), None);
        assert_eq!(match_forward(&spine, &ids(&[1, 9])), None);
    }

    #[test]
    fn a_backward_match_reads_the_axis_upwards() {
        let spine = ids(&[1, 2, 3, 4]);
        assert_eq!(
            match_backward(&spine, &ids(&[4, 3, 2, 1])),
            Some(vec![3, 2, 1, 0])
        );
        assert_eq!(match_backward(&spine, &ids(&[4, 2])), Some(vec![3, 1]));
        assert_eq!(match_backward(&spine, &ids(&[1, 2])), None);
    }

    #[test]
    fn a_repeated_station_matches_its_own_occurrence() {
        // An unrolled loop: A B C A.
        let spine = ids(&[1, 2, 3, 1]);
        assert_eq!(
            match_forward(&spine, &ids(&[1, 2, 3, 1])),
            Some(vec![0, 1, 2, 3])
        );
        // A run that goes round twice does not fit on this axis.
        assert_eq!(match_forward(&spine, &ids(&[1, 2, 3, 1, 2, 3, 1])), None);
    }

    #[test]
    fn equal_offsets_are_evenly_spaced() {
        let offsets = panel_offsets(&None, 30.0, 4, true);
        assert_eq!(offsets, vec![0.0, 30.0, 60.0, 90.0]);
        // Measured distances stay unused when the mode is equal.
        let measured = Some(vec![0.0, 100.0, 900.0, 1000.0]);
        assert_eq!(
            panel_offsets(&measured, 30.0, 4, false),
            vec![0.0, 30.0, 60.0, 90.0]
        );
    }

    #[test]
    fn distance_offsets_keep_the_total_height() {
        let distances = Some(vec![0.0, 1000.0, 5000.0]);
        let offsets = panel_offsets(&distances, 30.0, 3, true);
        assert_eq!(offsets[0], 0.0);
        assert_eq!(offsets[2], 60.0);
        assert!((offsets[1] - 12.0).abs() < 1e-9, "got {}", offsets[1]);
    }

    #[test]
    fn degenerate_distances_fall_back_to_equal_spacing() {
        let offsets = panel_offsets(&Some(vec![0.0, 0.0, 0.0]), 30.0, 3, true);
        assert_eq!(offsets, vec![0.0, 30.0, 60.0]);
    }
}
