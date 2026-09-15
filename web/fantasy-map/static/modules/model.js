/**
 * The network model, free of the editor: how a line is constructed, how
 * its displayed geometry is derived, and where a station's label sits.
 *
 * Nothing here touches the DOM, Leaflet, or the editor's mutable state,
 * so it loads under Node for tests and is the half of `state.js` that
 * workstream 2's renderer can consume without the editor.
 */

import { G } from "./geom.js";
import { MODES, modeRules, lineRadius } from "./modes.js";

/** Line identity palette (see `docs/STYLE.md`, colour roles). */
export const PALETTE = ["#ff6a2b", "#3d63ff", "#5ee6b6", "#b255ef", "#78d9ff", "#ffd75e"];

export const uid = (prefix) =>
  `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

/**
 * A line object with every default filled in. `ordinal` is the position
 * the line takes in the project, which picks its default name and
 * colour; `partial` overrides all of it except the validated mode and
 * the radius, which follow the mode when unset.
 */
export function makeLine(partial = {}, ordinal = 0) {
  const mode = partial.mode && MODES[partial.mode] ? partial.mode : "Mainline";
  return {
    id: uid("line"),
    name: `Line ${ordinal + 1}`,
    color: PALETTE[ordinal % PALETTE.length],
    visible: true,
    nodes: [],
    segments: [],
    branchOf: null,
    stations: [],
    ...partial,
    mode,
    minRadius: Number.isFinite(partial.minRadius)
      ? partial.minRadius
      : modeRules(mode).radius,
  };
}

/**
 * Displayed geometry for one line, built from its own nodes, segments,
 * and curve minimum. `state.lineGeometry` memoises this; anything
 * without the editor's cache calls it directly.
 */
export function routeGeometry(line) {
  return G.buildRouteGeometry(line.nodes, line.segments, lineRadius(line), {
    straight: modeRules(line.mode).straight,
  });
}

// ------------------------------------------------------ station labels

/**
 * Label placement. A station's label sits on one of four sides of its
 * disc and may be hidden; the default (top, visible) is stored as no
 * `label` key at all, so a default station exports exactly as before.
 * Free-pixel offsets are deliberately not part of this: four sides plus
 * hidden clears real collisions, and a pixel offset would not survive a
 * zoom change, which is when labels actually collide.
 */
export const LABEL_SIDES = ["top", "right", "bottom", "left"];
export const DEFAULT_LABEL_SIDE = "top";

export const labelSide = (station) =>
  station.label && LABEL_SIDES.includes(station.label.side)
    ? station.label.side
    : DEFAULT_LABEL_SIDE;

export const labelHidden = (station) =>
  Boolean(station.label && station.label.hidden);

export const nextLabelSide = (side) =>
  LABEL_SIDES[(LABEL_SIDES.indexOf(side) + 1) % LABEL_SIDES.length];

/**
 * The stored form of a label, or `undefined` when it is the default.
 * Unknown sides fall back to the default rather than being kept, so an
 * imported file cannot introduce a placement the renderer cannot draw.
 */
export function normalizeLabel(value) {
  if (!value || typeof value !== "object") return undefined;
  const side = LABEL_SIDES.includes(value.side) ? value.side : DEFAULT_LABEL_SIDE;
  const hidden = value.hidden === true;
  if (side === DEFAULT_LABEL_SIDE && !hidden) return undefined;
  return hidden ? { side, hidden: true } : { side };
}

/** Apply a label change in place, dropping the key when it is default. */
export function setStationLabel(station, patch) {
  const label = normalizeLabel({
    side: labelSide(station),
    hidden: labelHidden(station),
    ...patch,
  });
  if (label) station.label = label;
  else delete station.label;
  return station;
}

/** Label text: `Name (CODE)` once an identity code is set. */
export const stationLabelText = (station) =>
  station.code ? `${station.name} (${station.code})` : station.name;
