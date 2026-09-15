/**
 * GeoJSON export and import as the editor performs them: reading a file,
 * pushing history, reporting counts, and framing the result. The build
 * and parse themselves are pure and live in `geojson-model.js`, which
 * the Node tests exercise without a browser.
 *
 * The download helper both export paths use lives here.
 */

import {
  state, activeLine, lineGeometry, invalidate, pushHistory,
} from "./state.js";
import { map } from "./map-setup.js";
import { toast } from "./ui.js";
import { render } from "./render.js";
import { updateRadiusControl } from "./controls.js";
import { collectFeatures, buildFeatures, parseFeatures } from "./geojson-model.js";

// ------------------------------------------------------ GeoJSON I/O

export function exportGeoJson() {
  const features = buildFeatures(state.lines, lineGeometry);
  downloadFile(
    JSON.stringify({ type: "FeatureCollection", features }, null, 2),
    "openfantasymap.geojson",
    "application/geo+json",
  );
  toast(`Exported ${features.length} features.`);
}

export function importGeoJson(text) {
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    toast("That file is not valid JSON.");
    return;
  }
  const features = collectFeatures(data);
  if (!features.length) {
    toast("No importable features found.");
    return;
  }
  // Captured before the parse: the import commits in one step at the
  // end, so undo has to restore the project as it is right now.
  const snapshot = JSON.stringify(state.lines);
  let result;
  try {
    // The parse commits to `state.lines` only once the whole document
    // has been read, so a failure here has changed nothing to undo.
    result = parseFeatures(features, {
      lines: state.lines,
      geometryOf: lineGeometry,
    });
  } catch {
    toast("That file could not be imported.");
    return;
  }
  pushHistory(snapshot);
  if (result.created.length) {
    // Imported lines land like drawn ones: the last becomes active and
    // drawing extends from its end.
    state.activeLineId = result.created[result.created.length - 1].id;
    state.extendFrom = "end";
    updateRadiusControl();
  }
  invalidate();
  render();
  const geometryOfActive = activeLine() && lineGeometry(activeLine()).points;
  if (geometryOfActive && geometryOfActive.length) {
    map.fitBounds(L.latLngBounds(geometryOfActive), { padding: [40, 40] });
  }
  toast(
    `Imported ${result.imported} feature(s)` +
      `${result.skipped ? `, skipped ${result.skipped}` : ""}.`,
  );
}

export function downloadFile(content, filename, type) {
  const blob = new Blob([content], { type });
  const anchor = document.createElement("a");
  anchor.href = URL.createObjectURL(blob);
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(anchor.href);
}
