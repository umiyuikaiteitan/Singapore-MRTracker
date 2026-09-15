/** Draw Overpass vectors under the editor, using Leaflet's browser canvas. */
import { map, baseLayer, baseRenderer } from "./map-setup.js";
import { overpass, viewportBounds } from "./overpass.js";

let features = { roads: [], railways: [], stations: [], waterways: [], areas: [] };
let generation = 0, timer = null;
const listeners = new Set();
const status = document.getElementById("osm-status");
const retry = document.getElementById("retry-basemap");
const style = { pane: "osm-base", renderer: baseRenderer, interactive: false };
export function onBasemapData(listener) { listeners.add(listener); }
export function currentBasemap() { return features; }
export function getViewportFeatures() { return overpass.get(viewportBounds(map.getBounds()), map.getZoom()); }

function draw(data) {
  baseLayer.clearLayers();
  for (const area of data.areas) L.polygon(area.coordinates, {
    ...style, stroke: false, fillColor: area.kind === "water" ? "#183648" : "#1a302b", fillOpacity: 1,
  }).addTo(baseLayer);
  for (const water of data.waterways) L.polyline(water.coordinates, { ...style, color: "#285064", weight: 2 }).addTo(baseLayer);
  for (const road of data.roads) L.polyline(road.coordinates, {
    ...style, color: "#52657b", weight: /^(motorway|trunk|primary)/.test(road.kind) ? 2.5 : 1.2, opacity: 0.75,
  }).addTo(baseLayer);
  for (const rail of data.railways) L.polyline(rail.coordinates, {
    ...style, color: "#647386", weight: 1, dashArray: "5 3", opacity: 0.65,
  }).addTo(baseLayer);
}

export async function refreshBasemap() {
  const token = ++generation;
  status.textContent = "Loading OSM map…";
  retry.hidden = true;
  try {
    const data = await getViewportFeatures();
    if (token !== generation) return;
    draw(data);
    features = data;
    listeners.forEach(listener => listener());
    status.textContent = "OSM · " + data.roads.length + " roads · " + data.stations.length + " stations";
  } catch (error) {
    if (token !== generation) return;
    status.textContent = error.message + (baseLayer.getLayers().length ? " · previous view retained" : "");
    retry.hidden = false;
  }
}
map.on("movestart", () => { generation += 1; clearTimeout(timer); overpass.cancel(); });
map.on("moveend", () => { clearTimeout(timer); timer = setTimeout(refreshBasemap, 800); });
retry.addEventListener("click", refreshBasemap);
