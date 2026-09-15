/** The project model: editor state, lines, selection, undo history, and persistence. */

import { G } from "./geom.js";
import { MODES, LEGACY_MODES, modeRules, lineRadius } from "./modes.js";
import { PALETTE, uid, makeLine, routeGeometry } from "./model.js";
import { updateRadiusControl } from "./controls.js";
import { syncSharedGeometry } from "./topology.js";
import { render, selectLine } from "./render.js";

const STORAGE_KEY = "openfantasymap-project-v1";
// The palette and the id helper are the model's, re-exported here
// because the editor reaches for them through `state`.
export { PALETTE, uid };

// ---------------------------------------------------------------- state

export const state = {
  lines: [],
  activeLineId: null,
  tool: "draw",
  snapMode: "manual",
  extendFrom: "end", // which end of the active line drawing extends
  showCycleways: true,
  overlayEnabled: false,
  selected: null, // {kind:"node", lineId, index} | {kind:"station", lineId, stationId}
  // Extra node indexes on the active line, for multi-node operations.
  // The primary `selected` node is always a member while it is a node.
  selectedNodes: [],
  polygonDraft: [], // vertices while drawing an area station
  pendingSnaps: 0,
  notice: "",
};

export const history = { past: [], future: [] };
const geometryCache = new Map(); // lineId -> {points, segmentPoints}


export const activeLine = () =>
  state.lines.find((line) => line.id === state.activeLineId) || null;

export const lineById = (id) => state.lines.find((line) => line.id === id) || null;

export function createLine(partial = {}) {
  const line = makeLine(partial, state.lines.length);
  state.lines.push(line);
  state.activeLineId = line.id;
  state.extendFrom = "end";
  updateRadiusControl();
  return line;
}

export function lineGeometry(line) {
  let cached = geometryCache.get(line.id);
  if (!cached) {
    cached = routeGeometry(line);
    geometryCache.set(line.id, cached);
  }
  return cached;
}

/**
 * Curvature issues on a line's displayed geometry, memoised on its
 * geometry cache entry: the check only has to rerun when the geometry it
 * reads is rebuilt, which is also the only way the radius can change.
 * Straight-span modes (cableways) have no curves to measure.
 */
export function lineIssues(line) {
  const geometry = lineGeometry(line);
  if (!geometry.issues) {
    geometry.issues = modeRules(line.mode).straight
      ? []
      : G.findRadiusIssues(geometry.points, lineRadius(line));
  }
  return geometry.issues;
}

export function invalidate(lineId) {
  if (lineId) geometryCache.delete(lineId);
  else geometryCache.clear();
}

/**
 * Re-anchor every station to its last physical position after a route
 * edit. Stations are stored as arc-length fractions, so structural
 * changes (inserted nodes, extensions) would otherwise slide them.
 */
function reprojectStations(line) {
  const geometry = lineGeometry(line);
  for (const station of line.stations) {
    if (geometry.points.length < 2) continue;
    if (!station.coordinate) {
      station.coordinate = G.pointAtFraction(geometry.points, station.t);
      continue;
    }
    const projection = G.projectToPolyline(station.coordinate, geometry.points);
    if (projection) {
      station.t = projection.t;
      station.coordinate = projection.coordinate;
    }
  }
}

// ------------------------------------------------------- selection

/** Selected node indexes on the active line, ascending. */
export function selectedNodeIndexes() {
  const line = activeLine();
  if (!line) return [];
  return [...new Set(state.selectedNodes)]
    .filter((index) => index >= 0 && index < line.nodes.length)
    .sort((a, b) => a - b);
}

export function clearSelection() {
  state.selected = null;
  state.selectedNodes = [];
}

export function selectNode(lineId, index, mode) {
  const line = lineById(lineId);
  if (!line) return;
  if (lineId !== state.activeLineId) {
    // Nodes are only drawn for the active line, so switching lines
    // always starts a fresh selection.
    selectLine(lineId);
    mode = "replace";
  }
  if (mode === "toggle") {
    const existing = state.selectedNodes.indexOf(index);
    if (existing >= 0 && state.selectedNodes.length > 1) {
      state.selectedNodes.splice(existing, 1);
      const remaining = state.selectedNodes[state.selectedNodes.length - 1];
      state.selected = { kind: "node", lineId, index: remaining };
      return;
    }
    if (existing < 0) state.selectedNodes.push(index);
  } else if (mode === "range" && state.selected?.kind === "node") {
    const from = Math.min(state.selected.index, index);
    const to = Math.max(state.selected.index, index);
    for (let i = from; i <= to; i += 1) {
      if (!state.selectedNodes.includes(i)) state.selectedNodes.push(i);
    }
  } else {
    state.selectedNodes = [index];
  }
  state.selected = { kind: "node", lineId, index };
}

/** Run after any edit that changes route geometry, then re-render. */
export function afterGeometryChange(line) {
  if (line) syncSharedGeometry(line.id);
  invalidate();
  state.lines.forEach(reprojectStations);
  render();
}

// ------------------------------------------------------- persistence

export function save() {
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        version: 2,
        lines: state.lines,
        snapMode: state.snapMode,
        showCycleways: state.showCycleways,
      }),
    );
  } catch (error) {
    console.warn("Could not save project", error);
  }
}

export function load() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    const data = JSON.parse(raw);
    if (!Array.isArray(data.lines)) return;
    state.lines = data.lines;
    // Migrate v1 projects: coarse mode names, one global radius.
    for (const line of state.lines) {
      line.mode = LEGACY_MODES[line.mode] || line.mode;
      if (!MODES[line.mode]) line.mode = "Mainline";
      if (!Number.isFinite(line.minRadius)) {
        line.minRadius = data.minRadius || modeRules(line.mode).radius;
      }
    }
    state.snapMode = data.snapMode || "manual";
    state.showCycleways = data.showCycleways !== false;
    state.activeLineId = state.lines.length
      ? state.lines[state.lines.length - 1].id
      : null;
  } catch (error) {
    console.warn("Could not load project", error);
  }
}

/**
 * Snapshot the project for undo. `snapshot` lets a caller that has to
 * work before it knows the edit will land (import) record the state it
 * captured beforehand rather than the state after.
 */
export function pushHistory(snapshot = JSON.stringify(state.lines)) {
  history.past.push(snapshot);
  if (history.past.length > 60) history.past.shift();
  history.future = [];
}

export function timeTravel(from, to) {
  if (!from.length) return;
  to.push(JSON.stringify(state.lines));
  state.lines = JSON.parse(from.pop());
  if (!lineById(state.activeLineId)) {
    state.activeLineId = state.lines.length ? state.lines[0].id : null;
  }
  clearSelection();
  invalidate();
  render();
}
