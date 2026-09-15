/** Select standard OSM tiles or a minimal Overpass canvas basemap. */
import { map, baseLayer, baseRenderer } from "./map-setup.js";
import { overpass, viewportBounds, emptyFeatures } from "./overpass.js";

const STORAGE_KEY = "ofm-basemap-mode";
let mode = "tiles";
try { if (localStorage.getItem(STORAGE_KEY) === "overpass") mode = "overpass"; } catch (_) { /* Storage is optional. */ }
let features = emptyFeatures();
let generation = 0, timer = null, tileErrors = false, railError = false;
const listeners = new Set();
const status = document.getElementById("osm-status");
const railStatus = document.getElementById("rail-status");
const retry = document.getElementById("retry-basemap");
const selector = document.getElementById("basemap-mode");
const hint = document.getElementById("basemap-hint");
const overlayToggle = document.getElementById("overlay-toggle");
const style = { pane: "osm-base", renderer: baseRenderer, interactive: false };
const tiles = L.tileLayer("https://tile.openstreetmap.org/{z}/{x}/{y}.png", {
  maxNativeZoom: 19, maxZoom: 20, updateWhenIdle: true, keepBuffer: 2,
  referrerPolicy: "strict-origin-when-cross-origin",
});
function updateRetry() { retry.hidden = !(railError || (mode === "tiles" && tileErrors)); }
// Use normal browser HTTP caching; no prefetch, offline download, or cache busting.
tiles.on("loading", () => {
  if (mode !== "tiles") return;
  tileErrors = false;
  status.textContent = "Loading OSM tiles…";
  updateRetry();
});
tiles.on("tileerror", () => {
  if (mode !== "tiles") return;
  tileErrors = true;
  status.textContent = "Some OSM tiles could not load. Retry or choose Overpass.";
  updateRetry();
});
tiles.on("load", () => {
  if (mode !== "tiles" || tileErrors) return;
  status.textContent = "OSM tiles";
  updateRetry();
});

export function onBasemapData(listener) { listeners.add(listener); }
export function currentBasemap() { return features; }
export function getBasemapMode() { return mode; }
export function getViewportFeatures() {
  return overpass.get(viewportBounds(map.getBounds()), map.getZoom(), mode === "tiles" ? "rails" : "boundaries");
}
function updateControls() {
  selector.value = mode;
  overlayToggle.disabled = false;
  hint.textContent = mode === "tiles"
    ? "Fast OSM map with rail alignments from Overpass. SVG omits the tile background."
    : "Rail lines, borders, and terrain outlines. SVG includes these vectors when available.";
}
function draw(data) {
  baseLayer.clearLayers();
  for (const terrain of data.terrain) L.polyline(terrain.coordinates, {
    ...style, color: "#53877a", weight: 1, opacity: 0.8,
  }).addTo(baseLayer);
  for (const border of data.borders) L.polyline(border.coordinates, {
    ...style, color: "#a18db3", weight: 1.3, dashArray: "6 4", opacity: 0.8,
  }).addTo(baseLayer);
  for (const rail of data.railways) L.polyline(rail.coordinates, {
    ...style, color: "#9cabbc", weight: 1.5, dashArray: "5 3", opacity: 0.8,
  }).addTo(baseLayer);
}
export async function refreshBasemap() {
  const token = ++generation;
  railError = false;
  updateRetry();
  if (mode === "tiles" && !map.hasLayer(tiles)) {
    tileErrors = false;
    status.textContent = "Loading OSM tiles…";
    tiles.addTo(map);
  }
  if (mode === "overpass") status.textContent = "Overpass · rail and boundaries";
  railStatus.textContent = mode === "tiles" ? "Loading OSM rail alignments…" : "Loading rail lines, borders, and terrain…";
  try {
    const data = await getViewportFeatures();
    if (token !== generation) return;
    if (mode === "overpass") draw(data);
    features = data;
    listeners.forEach(listener => listener());
    railStatus.textContent = mode === "tiles" ? `Overpass · ${data.railways.length} rail alignments`
      : `${data.railways.length} rail lines · ${data.borders.length} borders · ${data.terrain.length} terrain outlines`;
    updateRetry();
  } catch (error) {
    if (token !== generation) return;
    railError = true;
    railStatus.textContent = error.message + (Object.values(features).some(items => items.length) ? " · previous rail/context view retained" : "");
    updateRetry();
  }
}
export async function setBasemapMode(value) {
  if (!["tiles", "overpass"].includes(value) || value === mode) return;
  generation += 1;
  clearTimeout(timer);
  overpass.cancel();
  if (map.hasLayer(tiles)) map.removeLayer(tiles);
  baseLayer.clearLayers();
  features = emptyFeatures();
  mode = value;
  tileErrors = railError = false;
  try { localStorage.setItem(STORAGE_KEY, mode); } catch (_) { /* Storage is optional. */ }
  updateControls();
  listeners.forEach(listener => listener());
  await refreshBasemap();
}
map.on("movestart", () => { generation += 1; clearTimeout(timer); overpass.cancel(); });
map.on("moveend", () => {
  clearTimeout(timer);
  timer = setTimeout(refreshBasemap, 800);
});
selector.addEventListener("change", () => setBasemapMode(selector.value));
retry.addEventListener("click", () => {
  if (mode === "tiles" && tileErrors && map.hasLayer(tiles)) tiles.redraw();
  return refreshBasemap();
});
updateControls();
