/** Sidebar wiring: tool and snap buttons, the radius slider, and keyboard shortcuts. */

import { scheduleRematchAdjacentSegments } from "./snapping.js";
import { supportsLocalRoad } from "./road-matching.js";
import { supportsLocalRail } from "./rail-matching.js";
import { apiEnabled } from "./config.js";
import { modeRules, lineRadius } from "./modes.js";
import {
  state, history, activeLine, lineById, createLine, invalidate,
  selectedNodeIndexes, clearSelection, pushHistory, afterGeometryChange,
  save, timeTravel,
} from "./state.js";
import { map } from "./map-setup.js";
import { toast, setStatus } from "./ui.js";
import { render, renderDraft } from "./render.js";
import { hideMenu } from "./context-menu.js";
import { splitLineAt, branchFromSelection, interlineFromActive } from "./topology.js";
import {
  closePolygonStation, deleteSelectedNodes, deleteSelection,
} from "./interactions.js";
import { refreshOverlay, drawOverlay } from "./overlay.js";
import { exportGeoJson, importGeoJson } from "./geojson.js";
import { exportSvg } from "./svg-export.js";

// ---------------------------------------------------------- controls

export function updateToolButtons() {
  document.querySelectorAll("#tools button").forEach((button) => {
    button.classList.toggle("active", button.dataset.tool === state.tool);
    button.setAttribute("aria-pressed", String(button.dataset.tool === state.tool));
  });
  const line = activeLine();
  const allowed = line ? modeRules(line.mode).snaps : ["road", "corridor"];
  document.querySelectorAll("#snap-modes button").forEach((button) => {
    button.classList.toggle("active", button.dataset.snap === state.snapMode);
    button.setAttribute("aria-pressed", String(button.dataset.snap === state.snapMode));
    if (button.dataset.snap === "corridor") button.textContent = line?.mode === "Ferry" ? "Follow ferry" : "Follow rails";
    button.disabled =
      button.dataset.snap !== "manual" &&
      (!allowed.includes(button.dataset.snap) || (!apiEnabled && !(button.dataset.snap === "corridor" && supportsLocalRail(line?.mode || "Mainline")) && !(button.dataset.snap === "road" && supportsLocalRoad(line?.mode || "Mainline"))));
  });
  map.getContainer().style.cursor =
    state.tool === "draw" || state.tool === "polygon" ? "crosshair" : "";
}

export function setTool(tool) {
  state.tool = tool;
  if (tool !== "polygon") {
    state.polygonDraft = [];
    renderDraft();
  }
  clearSelection();
  updateToolButtons();
  render();
}

document.querySelectorAll("#tools button").forEach((button) =>
  button.addEventListener("click", () => setTool(button.dataset.tool)),
);
document.querySelectorAll("#snap-modes button").forEach((button) =>
  button.addEventListener("click", () => {
    state.snapMode = button.dataset.snap;
    updateToolButtons();
    setStatus();
    save();
  }),
);

// The radius slider edits the active line (each mode has its own default).
const radiusInput = document.getElementById("radius");
export function updateRadiusControl() {
  const line = activeLine();
  radiusInput.disabled = !line || Boolean(modeRules(line.mode).straight);
  const value = line ? lineRadius(line) : Number(radiusInput.value);
  radiusInput.value = value;
  document.getElementById("radius-value").textContent = radiusInput.disabled
    ? "—"
    : `${value} m`;
}
radiusInput.addEventListener("input", () => {
  const line = activeLine();
  if (!line) return;
  line.minRadius = Number(radiusInput.value);
  document.getElementById("radius-value").textContent = `${line.minRadius} m`;
  invalidate(line.id);
  render();
});

document.getElementById("new-line").addEventListener("click", () => {
  pushHistory();
  createLine();
  setTool("draw");
});
document
  .getElementById("branch-line")
  .addEventListener("click", branchFromSelection);
document
  .getElementById("interline-line")
  .addEventListener("click", interlineFromActive);
document.getElementById("split-line").addEventListener("click", () => {
  const line = activeLine();
  if (!line) return;
  splitLineAt(line, selectedNodeIndexes());
});
document.getElementById("delete-nodes").addEventListener("click", () => {
  const line = activeLine();
  const selected = selectedNodeIndexes();
  if (!line || !selected.length) {
    toast("Select one or more nodes first.");
    return;
  }
  deleteSelectedNodes(line, selected);
});

export const cyclewayToggle = document.getElementById("cycleway-toggle");
cyclewayToggle.addEventListener("change", () => {
  state.showCycleways = cyclewayToggle.checked;
  render();
});

const overlayToggle = document.getElementById("overlay-toggle");
overlayToggle.addEventListener("change", () => {
  state.overlayEnabled = overlayToggle.checked;
  drawOverlay();
  if (state.overlayEnabled) refreshOverlay();
});

document.getElementById("export-geojson").addEventListener("click", exportGeoJson);
document.getElementById("export-svg").addEventListener("click", exportSvg);
document
  .getElementById("import-geojson")
  .addEventListener("click", () =>
    document.getElementById("import-file").click(),
  );
document.getElementById("import-file").addEventListener("change", (event) => {
  const file = event.target.files && event.target.files[0];
  if (!file) return;
  file.text().then(importGeoJson);
  event.target.value = "";
});
document.getElementById("clear-project").addEventListener("click", () => {
  if (!confirm("Delete every line and station?")) return;
  pushHistory();
  state.lines = [];
  state.activeLineId = null;
  clearSelection();
  invalidate();
  render();
});

// Nudge state: one undo entry per burst of arrow presses.
let lastNudgeAt = 0;
const NUDGE_DIRECTIONS = {
  arrowup: [0, -1],
  arrowdown: [0, 1],
  arrowleft: [-1, 0],
  arrowright: [1, 0],
};

function nudgeSelectedNode(direction, big) {
  const selection = state.selected;
  if (!selection || selection.kind !== "node") return false;
  const line = lineById(selection.lineId);
  if (!line) return false;
  // Branch anchors follow their parent line and never move on their own.
  const movable = selectedNodeIndexes().filter(
    (index) => !(line.branchOf && line.branchOf.branchNodeIndex === index),
  );
  if (!movable.length) return false;
  const now = Date.now();
  if (now - lastNudgeAt > 800) pushHistory();
  lastNudgeAt = now;
  const step = big ? 20 : 4; // screen pixels, so it scales with zoom
  for (const index of movable) {
    const point = map.latLngToContainerPoint(line.nodes[index]);
    const moved = map.containerPointToLatLng([
      point.x + direction[0] * step,
      point.y + direction[1] * step,
    ]);
    line.nodes[index] = [moved.lat, moved.lng];
  }
  afterGeometryChange(line);
  scheduleRematchAdjacentSegments(line, movable);
  return true;
}

document.addEventListener("keydown", (event) => {
  const tag = event.target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
  const key = event.key.toLowerCase();
  const control = event.ctrlKey || event.metaKey;
  if (control && key === "z" && !event.shiftKey) {
    event.preventDefault();
    timeTravel(history.past, history.future);
  } else if (control && (key === "y" || (key === "z" && event.shiftKey))) {
    event.preventDefault();
    timeTravel(history.future, history.past);
  } else if (control && key === "s") {
    event.preventDefault();
    exportGeoJson();
  } else if (control && key === "a") {
    const line = activeLine();
    if (line && line.nodes.length) {
      event.preventDefault();
      state.selectedNodes = line.nodes.map((_, index) => index);
      state.selected = {
        kind: "node",
        lineId: line.id,
        index: line.nodes.length - 1,
      };
      render();
    }
  } else if (control && event.shiftKey && key === "x") {
    const line = activeLine();
    event.preventDefault();
    if (line) splitLineAt(line, selectedNodeIndexes());
  } else if (NUDGE_DIRECTIONS[key]) {
    if (nudgeSelectedNode(NUDGE_DIRECTIONS[key], event.shiftKey)) {
      event.preventDefault();
    }
  } else if (key === "v") setTool("select");
  else if (key === "d") setTool("draw");
  else if (key === "s") setTool("station");
  else if (key === "p") setTool("polygon");
  else if (key === "n") {
    pushHistory();
    createLine();
    setTool("draw");
  } else if (key === "b") branchFromSelection();
  else if (key === "i") interlineFromActive();
  else if (key === "enter" && state.tool === "polygon") closePolygonStation();
  else if (key === "escape") {
    hideMenu();
    state.polygonDraft = [];
    clearSelection();
    renderDraft();
    render();
  } else if (key === "delete" || key === "backspace") deleteSelection();
});
