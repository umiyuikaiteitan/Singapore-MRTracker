/** Branch anchors, interlined alignments, and cutting a line into pieces. */

import { G } from "./geom.js";
import { lineRadius } from "./modes.js";
import { normalizeNodeSettings } from "./model.js";
import {
  PALETTE, state, activeLine, lineById, createLine, lineGeometry,
  invalidate, clearSelection, pushHistory, afterGeometryChange, uid,
} from "./state.js";
import { toast } from "./ui.js";
import { lineVisible, render } from "./render.js";
import { updateToolButtons } from "./controls.js";

/** Clone the settings aligned with a range of nodes, omitting empty data. */
function nodeSettingsSlice(line, from = 0, to = line.nodes.length) {
  const settings = normalizeNodeSettings(line.nodeSettings, line.nodes.length);
  return settings
    ? normalizeNodeSettings(settings.slice(from, to), to - from)
    : undefined;
}

// -------------------------------------------- branches & interlining

/** Keep branch anchor nodes glued to their parent line node. */
export function syncBranches() {
  for (const line of state.lines) {
    const branch = line.branchOf;
    if (!branch) continue;
    const parent = lineById(branch.lineId);
    if (!parent || !parent.nodes.length) {
      line.branchOf = null;
      continue;
    }
    branch.nodeIndex = Math.max(
      0,
      Math.min(parent.nodes.length - 1, branch.nodeIndex),
    );
    branch.branchNodeIndex = Math.max(
      0,
      Math.min(Math.max(0, line.nodes.length - 1), branch.branchNodeIndex),
    );
    const anchor = parent.nodes[branch.nodeIndex];
    const current = line.nodes[branch.branchNodeIndex];
    if (
      current &&
      (current[0] !== anchor[0] || current[1] !== anchor[1])
    ) {
      line.nodes[branch.branchNodeIndex] = [anchor[0], anchor[1]];
      invalidate(line.id);
    }
  }
}

/**
 * Propagate the geometry of shared (interlined) segments from the
 * edited line to every other line carrying the same interline id.
 */
function syncInterlines(sourceLineId) {
  const source = lineById(sourceLineId);
  if (!source) return;
  const shared = new Map();
  source.segments.forEach((segment, index) => {
    if (segment && segment.interlineId) {
      shared.set(segment.interlineId, {
        segment,
        start: source.nodes[index],
        end: source.nodes[index + 1],
      });
    }
  });
  if (!shared.size) return;
  for (const line of state.lines) {
    if (line.id === sourceLineId) continue;
    let changed = false;
    line.segments.forEach((segment, index) => {
      const master = segment && segment.interlineId
        ? shared.get(segment.interlineId)
        : null;
      if (!master) return;
      segment.profile = master.segment.profile;
      segment.guide = master.segment.guide
        ? master.segment.guide.map((point) => [point[0], point[1]])
        : [];
      if (master.start) line.nodes[index] = [...master.start];
      if (master.end) line.nodes[index + 1] = [...master.end];
      changed = true;
    });
    if (changed) invalidate(line.id);
  }
}

export function syncSharedGeometry(editedLineId) {
  syncInterlines(editedLineId);
  syncBranches();
}

/** Number of visible services sharing a segment, and this line's slot. */
export function interlineSlot(line, segment) {
  if (!segment || !segment.interlineId) return null;
  const members = state.lines
    .filter(
      (candidate) =>
        lineVisible(candidate) &&
        candidate.segments.some(
          (item) => item && item.interlineId === segment.interlineId,
        ),
    )
    .map((candidate) => candidate.id)
    .sort();
  if (members.length < 2) return null;
  return { count: members.length, index: members.indexOf(line.id) };
}

/**
 * Cut a line into pieces at the given interior node indexes. Each cut
 * node is duplicated so both pieces keep an endpoint there; stations,
 * branches, and the line's own branch anchor follow their piece.
 */
export function splitLineAt(line, cutIndexes, options = {}) {
  const cuts = [...new Set(cutIndexes)]
    .filter((index) => Number.isInteger(index) && index > 0 && index < line.nodes.length - 1)
    .sort((a, b) => a - b);
  if (!cuts.length) {
    toast("Select an interior node (not an endpoint) to split a line.");
    return;
  }
  if (!options.skipHistory) pushHistory();
  const bounds = [0, ...cuts, line.nodes.length - 1];
  const geometry = lineGeometry(line);
  const suffixes = "ABCDEFGH";
  const pieces = [];
  for (let piece = 0; piece < bounds.length - 1; piece += 1) {
    const from = bounds[piece];
    const to = bounds[piece + 1];
    const nodeSettings = nodeSettingsSlice(line, from, to + 1);
    pieces.push(
      createLine({
        name: `${line.name} ${suffixes[piece] || piece + 1}`,
        mode: line.mode,
        minRadius: lineRadius(line),
        color: line.color,
        visible: line.visible,
        nodes: line.nodes.slice(from, to + 1).map((node) => [...node]),
        ...(nodeSettings ? { nodeSettings } : {}),
        segments: line.segments.slice(from, to).map((segment) => ({
          profile: segment ? segment.profile : "manual",
          guide: segment && segment.guide ? segment.guide.map((p) => [...p]) : [],
          interlineId: segment ? segment.interlineId : undefined,
        })),
        stations: [],
        // The piece holding the old branch anchor keeps the parent link.
        branchOf:
          line.branchOf &&
          line.branchOf.branchNodeIndex >= from &&
          line.branchOf.branchNodeIndex <= to
            ? {
                ...line.branchOf,
                branchNodeIndex: line.branchOf.branchNodeIndex - from,
              }
            : null,
      }),
    );
  }
  // Re-home branches that anchored to this line's nodes.
  for (const other of state.lines) {
    if (!other.branchOf || other.branchOf.lineId !== line.id) continue;
    const anchorIndex = other.branchOf.nodeIndex;
    let target = 0;
    while (target < bounds.length - 2 && anchorIndex > bounds[target + 1]) {
      target += 1;
    }
    other.branchOf = {
      ...other.branchOf,
      lineId: pieces[target].id,
      nodeIndex: anchorIndex - bounds[target],
    };
  }
  // Stations go to whichever piece now runs closest to them.
  for (const station of line.stations) {
    const anchor =
      station.coordinate || G.pointAtFraction(geometry.points, station.t);
    if (!anchor) continue;
    let best = null;
    for (const piece of pieces) {
      const projection = G.projectToPolyline(
        anchor,
        lineGeometry(piece).points,
      );
      if (projection && (!best || projection.distanceMeters < best.distance)) {
        best = { piece, distance: projection.distanceMeters, projection };
      }
    }
    if (!best) continue;
    best.piece.stations.push({
      ...station,
      id: uid("station"),
      t: best.projection.t,
      coordinate: best.projection.coordinate,
    });
  }
  state.lines = state.lines.filter((item) => item.id !== line.id);
  state.activeLineId = pieces[0].id;
  clearSelection();
  afterGeometryChange(pieces[0]);
  toast(`Split into ${pieces.length} lines.`);
  return pieces;
}

// ------------------------------------------------- branch & interline

export function branchFromSelection() {
  const selection = state.selected;
  if (!selection || selection.kind !== "node") {
    toast("Click a node to select it, then press Branch (B).");
    return;
  }
  const parent = lineById(selection.lineId);
  if (!parent) return;
  pushHistory();
  const anchor = parent.nodes[selection.index];
  const anchorSettings = nodeSettingsSlice(
    parent,
    selection.index,
    selection.index + 1,
  );
  createLine({
    name: `${parent.name} branch`,
    mode: parent.mode,
    minRadius: lineRadius(parent),
    color: PALETTE[state.lines.length % PALETTE.length],
    nodes: [[anchor[0], anchor[1]]],
    ...(anchorSettings ? { nodeSettings: anchorSettings } : {}),
    segments: [],
    branchOf: {
      lineId: parent.id,
      nodeIndex: selection.index,
      branchNodeIndex: 0,
    },
  });
  state.tool = "draw";
  clearSelection();
  toast("Branch started — draw its route from the anchor node.");
  render();
  updateToolButtons();
}

export function interlineFromActive() {
  const source = activeLine();
  if (!source || source.nodes.length < 2) {
    toast("Select a line with at least two nodes to interline.");
    return;
  }
  pushHistory();
  source.segments = source.segments.map((segment, index) => ({
    profile: "manual",
    guide: [],
    ...segment,
    interlineId:
      (segment && segment.interlineId) || uid(`il-${index}`),
  }));
  const nodeSettings = nodeSettingsSlice(source);
  createLine({
    name: `${source.name} interline`,
    mode: source.mode,
    minRadius: lineRadius(source),
    color: PALETTE[state.lines.length % PALETTE.length],
    nodes: source.nodes.map((node) => [node[0], node[1]]),
    ...(nodeSettings ? { nodeSettings } : {}),
    segments: source.segments.map((segment) => ({
      profile: segment.profile,
      guide: (segment.guide || []).map((point) => [point[0], point[1]]),
      interlineId: segment.interlineId,
    })),
    stations: [],
  });
  invalidate();
  toast("Interlined service created — extend either end or restyle it.");
  render();
}
