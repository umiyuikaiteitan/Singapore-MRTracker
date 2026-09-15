/** The two transient UI surfaces: the toast and the status bar. */

import { G } from "./geom.js";
import { lineRadius } from "./modes.js";
import { state, activeLine, lineGeometry, selectedNodeIndexes } from "./state.js";

// ------------------------------------------------------------ UI bits

const toastElement = document.getElementById("toast");
let toastTimer = null;
export function toast(message) {
  toastElement.textContent = message;
  toastElement.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toastElement.hidden = true;
  }, 3200);
}

/**
 * Tooltip content as a DOM node. Leaflet assigns *string* tooltip
 * content to `innerHTML`, so every label carrying OSM, imported, or
 * typed-in text has to reach `bindTooltip` as a node instead.
 */
export function tooltipContent(text) {
  const element = document.createElement("span");
  element.textContent = text;
  return element;
}

export function setStatus() {
  const line = activeLine();
  const parts = [`Tool: ${state.tool}`, `Snap: ${state.snapMode}`];
  if (line) {
    const km = G.routeLengthMeters(lineGeometry(line).points) / 1000;
    parts.push(
      `${line.name} (${line.mode}, r≥${lineRadius(line)} m): ` +
        `${km.toFixed(2)} km, ${line.stations.length} stations`,
    );
  }
  const selectedCount = selectedNodeIndexes().length;
  if (selectedCount > 1) parts.push(`${selectedCount} nodes selected`);
  if (state.pendingSnaps > 0) parts.push("Matching corridor…");
  if (state.notice) parts.push(state.notice);
  document.getElementById("status-text").textContent = parts.join("  ·  ");
}
