/** Pointer tools: route and segment clicks, marquee selection, node and station edits. */

import { supportsLocalRoad } from "./road-matching.js";
import { supportsLocalRail } from "./rail-matching.js";
import { apiEnabled } from "./config.js";
import { G } from "./geom.js";
import { modeRules } from "./modes.js";
import { normalizeNodeSettings } from "./model.js";
import {
  state, activeLine, lineById, createLine, lineGeometry,
  selectedNodeIndexes, clearSelection, pushHistory, afterGeometryChange,
  uid,
} from "./state.js";
import { map } from "./map-setup.js";
import { toast } from "./ui.js";
import { snapSegment } from "./snapping.js";
import { lineVisible, render, renderDraft, selectLine } from "./render.js";
import { menuElement, hideMenu } from "./context-menu.js";

const STATION_PICK_PX = 16;
const POLYGON_ANCHOR_MAX_M = 250;

// ------------------------------------------------------ interactions

export function onRouteClick(line, latlng) {
  if (state.tool === "station") {
    addStationAt(line, [latlng.lat, latlng.lng]);
  } else if (state.tool === "select") {
    selectLine(line.id);
  } else if (state.tool === "draw") {
    // Drawing across an existing line still places a node.
    addRouteNode([latlng.lat, latlng.lng]);
  } else if (state.tool === "polygon") {
    state.polygonDraft.push([latlng.lat, latlng.lng]);
    renderDraft();
  }
}

/**
 * Every (line, segment index) sharing this segment's alignment —
 * the segment itself plus interlined copies on other lines.
 */
function segmentSharers(line, segmentIndex) {
  const segment = line.segments[segmentIndex];
  const sharers = [{ line, index: segmentIndex }];
  if (!segment || !segment.interlineId) return sharers;
  for (const other of state.lines) {
    if (other.id === line.id) continue;
    const index = other.segments.findIndex(
      (item) => item && item.interlineId === segment.interlineId,
    );
    if (index >= 0) sharers.push({ line: other, index });
  }
  return sharers;
}

/** Shift branch anchors after a node was inserted at `nodeIndex`. */
function shiftAnchorsAfterInsert(line, nodeIndex) {
  if (line.branchOf && line.branchOf.branchNodeIndex >= nodeIndex) {
    line.branchOf.branchNodeIndex += 1;
  }
  for (const other of state.lines) {
    if (
      other.branchOf &&
      other.branchOf.lineId === line.id &&
      other.branchOf.nodeIndex >= nodeIndex
    ) {
      other.branchOf.nodeIndex += 1;
    }
  }
}

/** Keep the optional per-node settings array aligned with a node edit. */
function spliceNodeSettings(line, index, deleteCount, ...items) {
  if (!("nodeSettings" in line)) return;
  const settings = Array.isArray(line.nodeSettings)
    ? line.nodeSettings.map((setting) => (setting ? { ...setting } : null))
    : [];
  settings.splice(index, deleteCount, ...items);
  const normalized = normalizeNodeSettings(settings, line.nodes.length);
  if (normalized) line.nodeSettings = normalized;
  else delete line.nodeSettings;
}

/**
 * Insert a manual node where the user clicked on a segment. The
 * segment's guide is split in place, so matched geometry is kept on
 * both halves; interlined copies are split identically.
 */
function insertNodeOnSegment(line, segmentIndex, coordinate) {
  pushHistory();
  for (const sharer of segmentSharers(line, segmentIndex)) {
    const target = sharer.line;
    const segment = target.segments[sharer.index] || {
      profile: "manual",
      guide: [],
    };
    const guide = G.anchoredGuide(
      segment.guide,
      target.nodes[sharer.index],
      target.nodes[sharer.index + 1],
    );
    const split = G.splitAtProjection(guide, coordinate);
    if (!split) continue;
    target.nodes.splice(sharer.index + 1, 0, [...split.coordinate]);
    spliceNodeSettings(target, sharer.index + 1, 0, null);
    const half = (guidePart, suffix) => ({
      profile: segment.profile,
      guide: guidePart,
      interlineId: segment.interlineId
        ? segment.interlineId + suffix
        : undefined,
    });
    target.segments.splice(
      sharer.index,
      1,
      half(split.before, "-a"),
      half(split.after, "-b"),
    );
    shiftAnchorsAfterInsert(target, sharer.index + 1);
  }
  clearSelection();
  afterGeometryChange(line);
}

/** Change one segment's profile: straight, or re-match a corridor. */
function setSegmentProfile(line, segmentIndex, kind) {
  pushHistory();
  if (kind === "manual") {
    const segment = line.segments[segmentIndex];
    if (segment) {
      segment.profile = "manual";
      segment.guide = [];
    }
    afterGeometryChange(line);
  } else {
    snapSegment(line, segmentIndex, kind);
  }
}

export function segmentMenuItems(line, segmentIndex, latlng) {
  const coordinate = [latlng.lat, latlng.lng];
  const segment = line.segments[segmentIndex];
  const profile = segment ? segment.profile : "manual";
  const allowed = modeRules(line.mode).snaps.filter(kind => apiEnabled || (kind === "corridor" && supportsLocalRail(line.mode)) || (kind === "road" && supportsLocalRoad(line.mode)));
  return [
    {
      label: "Insert node here",
      action: () => insertNodeOnSegment(line, segmentIndex, coordinate),
    },
    {
      label: "Add station here",
      action: () => addStationAt(line, coordinate),
    },
    profile !== "manual" && {
      label: "Make segment straight",
      action: () => setSegmentProfile(line, segmentIndex, "manual"),
    },
    allowed.includes("road") &&
      profile !== "road" && {
        label: "Segment: follow streets",
        action: () => setSegmentProfile(line, segmentIndex, "road"),
      },
    allowed.includes("corridor") &&
      profile !== "rail" && {
        label: "Segment: follow corridor",
        action: () => setSegmentProfile(line, segmentIndex, "corridor"),
      },
  ];
}

function addStationAt(line, coordinate) {
  const geometry = lineGeometry(line);
  const projection = G.projectToPolyline(coordinate, geometry.points);
  if (!projection) {
    toast("Draw the route first, then add stations along it.");
    return;
  }
  pushHistory();
  line.stations.push({
    id: uid("station"),
    name: `Station ${line.stations.length + 1}`,
    t: projection.t,
    coordinate: projection.coordinate,
  });
  clearSelection();
  render();
}

map.on("click", (event) => {
  // A click that dismisses the context menu, or that merely ends a
  // marquee drag, must not also place a node or clear the selection.
  if (!menuElement.hidden) {
    hideMenu();
    return;
  }
  if (clickSwallowed()) return;
  const coordinate = [event.latlng.lat, event.latlng.lng];
  if (state.tool === "draw") {
    addRouteNode(coordinate);
  } else if (state.tool === "station") {
    // Stations may only be placed on a route: find the closest line
    // within a small pixel radius of the click.
    const hit = findRouteNear(event.latlng);
    if (hit) addStationAt(hit.line, coordinate);
    else toast("Stations can only be placed on a route.");
  } else if (state.tool === "polygon") {
    state.polygonDraft.push(coordinate);
    renderDraft();
  } else {
    clearSelection();
    render();
  }
});

map.on("dblclick", (event) => {
  if (state.tool === "polygon") {
    L.DomEvent.stop(event);
    closePolygonStation();
  }
});

// ------------------------------------------------- marquee selection

// Shift+drag (or Ctrl/Cmd+drag) over the map rubber-bands a box that
// selects the active line's nodes inside it. Leaflet's own shift+drag
// box zoom is disabled so the gesture is unambiguous.
map.boxZoom.disable();

const marqueeElement = document.createElement("div");
marqueeElement.id = "marquee";
marqueeElement.hidden = true;
document.getElementById("map-wrap").append(marqueeElement);
let marqueeStart = null;
// A drag inside the map still ends with a synthetic click; that click
// must not fall through to the tool and clear the fresh selection.
let swallowNextClick = false;

export function clickSwallowed() {
  if (!swallowNextClick) return false;
  swallowNextClick = false;
  return true;
}

function marqueeRectangle(current) {
  return {
    left: Math.min(marqueeStart.x, current.x),
    top: Math.min(marqueeStart.y, current.y),
    right: Math.max(marqueeStart.x, current.x),
    bottom: Math.max(marqueeStart.y, current.y),
  };
}

map.getContainer().addEventListener("mousedown", (event) => {
  if (event.button !== 0) return;
  const additive = event.ctrlKey || event.metaKey;
  if (!event.shiftKey && !additive) return;
  const line = activeLine();
  if (!line || !line.nodes.length) return;
  marqueeStart = map.mouseEventToContainerPoint(event);
  marqueeStart.additive = additive;
  map.dragging.disable();
  marqueeElement.hidden = false;
  marqueeElement.style.width = "0px";
  marqueeElement.style.height = "0px";
  marqueeElement.style.left = marqueeStart.x + "px";
  marqueeElement.style.top = marqueeStart.y + "px";
  event.preventDefault();
});

map.getContainer().addEventListener("mousemove", (event) => {
  if (!marqueeStart) return;
  const rectangle = marqueeRectangle(map.mouseEventToContainerPoint(event));
  marqueeElement.style.left = rectangle.left + "px";
  marqueeElement.style.top = rectangle.top + "px";
  marqueeElement.style.width = rectangle.right - rectangle.left + "px";
  marqueeElement.style.height = rectangle.bottom - rectangle.top + "px";
});

document.addEventListener("mouseup", (event) => {
  if (!marqueeStart) return;
  const rectangle = marqueeRectangle(map.mouseEventToContainerPoint(event));
  const additive = marqueeStart.additive;
  marqueeStart = null;
  marqueeElement.hidden = true;
  map.dragging.enable();
  swallowNextClick = true;
  const line = activeLine();
  if (!line) return;
  if (rectangle.right - rectangle.left < 4 && rectangle.bottom - rectangle.top < 4) {
    return; // a click, not a drag
  }
  const inside = [];
  line.nodes.forEach((node, index) => {
    const point = map.latLngToContainerPoint(node);
    if (
      point.x >= rectangle.left &&
      point.x <= rectangle.right &&
      point.y >= rectangle.top &&
      point.y <= rectangle.bottom
    ) {
      inside.push(index);
    }
  });
  if (!inside.length) {
    if (!additive) clearSelection();
    render();
    return;
  }
  const base = additive ? state.selectedNodes : [];
  state.selectedNodes = [...new Set([...base, ...inside])];
  state.selected = {
    kind: "node",
    lineId: line.id,
    index: inside[inside.length - 1],
  };
  render();
  toast(`${state.selectedNodes.length} nodes selected.`);
});

function findRouteNear(latlng) {
  const clickPoint = map.latLngToContainerPoint(latlng);
  let best = null;
  for (const line of state.lines) {
    if (!lineVisible(line)) continue;
    const geometry = lineGeometry(line);
    const projection = G.projectToPolyline(
      [latlng.lat, latlng.lng],
      geometry.points,
    );
    if (!projection) continue;
    const pixel = map.latLngToContainerPoint(projection.coordinate);
    const distance = clickPoint.distanceTo(pixel);
    if (distance <= STATION_PICK_PX && (!best || distance < best.distance)) {
      best = { line, distance, projection };
    }
  }
  return best;
}

/** Add a node to the active line at whichever end drawing extends. */
function addRouteNode(coordinate) {
  let line = activeLine();
  if (!line) line = createLine();
  pushHistory();
  const prepend = state.extendFrom === "start" && line.nodes.length > 0;
  let segmentIndex = -1;
  if (prepend) {
    line.nodes.unshift(coordinate);
    spliceNodeSettings(line, 0, 0, null);
    line.segments.unshift({ profile: "manual", guide: [] });
    shiftAnchorsAfterInsert(line, 0);
    segmentIndex = 0;
  } else {
    line.nodes.push(coordinate);
    spliceNodeSettings(line, line.nodes.length - 1, 0, null);
    if (line.nodes.length >= 2) {
      segmentIndex = line.nodes.length - 2;
      line.segments[segmentIndex] = { profile: "manual", guide: [] };
    }
  }
  if (segmentIndex >= 0 && state.snapMode !== "manual") {
    if (modeRules(line.mode).snaps.includes(state.snapMode)) {
      // The segment stays a straight manual line until matching
      // finishes; the clicked nodes are never moved by the match.
      snapSegment(line, segmentIndex, state.snapMode);
    } else {
      toast(
        `${line.mode} lines do not snap to ${
          state.snapMode === "road" ? "streets" : "a corridor"
        } — segment drawn manually.`,
      );
    }
  }
  afterGeometryChange(line);
}

export function closePolygonStation() {
  const draft = state.polygonDraft;
  if (draft.length < 3) {
    toast("An area station needs at least 3 corners.");
    return;
  }
  const line = activeLine();
  if (!line) {
    toast("Select a line first.");
    state.polygonDraft = [];
    renderDraft();
    return;
  }
  const geometry = lineGeometry(line);
  const centroid = [
    draft.reduce((sum, point) => sum + point[0], 0) / draft.length,
    draft.reduce((sum, point) => sum + point[1], 0) / draft.length,
  ];
  const projection = G.projectToPolyline(centroid, geometry.points);
  if (!projection || projection.distanceMeters > POLYGON_ANCHOR_MAX_M) {
    toast(
      `Area stations must sit on the route (within ${POLYGON_ANCHOR_MAX_M} m).`,
    );
    return;
  }
  pushHistory();
  // Store corners as metre offsets from the on-route anchor so the
  // polygon follows the route when it moves.
  line.stations.push({
    id: uid("station"),
    name: `Station ${line.stations.length + 1}`,
    t: projection.t,
    coordinate: projection.coordinate,
    polygon: draft.map((vertex) =>
      G.coordinateToOffset(projection.coordinate, vertex),
    ),
  });
  state.polygonDraft = [];
  render();
}

/** Remove one node; adjacent segments merge into one manual segment. */
function deleteNodeAt(line, index, options = {}) {
  if (line.branchOf && line.branchOf.branchNodeIndex === index) {
    if (!options.skipHistory) {
      toast("Branch anchors are removed by deleting the branch line.");
    }
    return;
  }
  line.nodes.splice(index, 1);
  spliceNodeSettings(line, index, 1);
  if (index === 0) {
    line.segments.shift();
  } else if (index >= line.segments.length) {
    line.segments.pop();
  } else {
    line.segments.splice(index - 1, 2, { profile: "manual", guide: [] });
  }
  if (line.branchOf && line.branchOf.branchNodeIndex > index) {
    line.branchOf.branchNodeIndex -= 1;
  }
  state.lines.forEach((other) => {
    if (other.branchOf && other.branchOf.lineId === line.id) {
      if (other.branchOf.nodeIndex > index) other.branchOf.nodeIndex -= 1;
    }
  });
  clearSelection();
  afterGeometryChange(line);
}

/** Delete several nodes at once (highest index first). */
export function deleteSelectedNodes(line, indexes) {
  const removable = indexes.filter(
    (index) =>
      !(line.branchOf && line.branchOf.branchNodeIndex === index),
  );
  if (!removable.length) {
    toast("Branch anchors are removed by deleting the branch line.");
    return;
  }
  if (line.nodes.length - removable.length < 1) {
    toast("A line needs at least one node — delete the line instead.");
    return;
  }
  pushHistory();
  for (const index of [...removable].sort((a, b) => b - a)) {
    deleteNodeAt(line, index, { skipHistory: true });
  }
  if (removable.length < indexes.length) {
    toast("Kept the branch anchor node.");
  }
}

export function deleteSelection() {
  const selection = state.selected;
  if (!selection) return;
  const line = lineById(selection.lineId);
  if (!line) return;
  if (selection.kind === "station") {
    pushHistory();
    line.stations = line.stations.filter(
      (station) => station.id !== selection.stationId,
    );
    clearSelection();
    render();
  } else if (selection.kind === "node") {
    const selected = selectedNodeIndexes();
    if (selected.length > 1) {
      deleteSelectedNodes(line, selected);
    } else {
      pushHistory();
      deleteNodeAt(line, selection.index);
    }
  }
}
