/** The OSM reference overlay: existing railways and stations for the viewport. */

import { apiEnabled, postApi } from "./config.js";
import { state } from "./state.js";
import { map, overlayLayer } from "./map-setup.js";
import { toast, setStatus, tooltipContent } from "./ui.js";

// ------------------------------------------------------- OSM overlay

let overlayFeatures = { roads: [], railways: [], stations: [] };
let overlayTimer = null;
export async function refreshOverlay() {
  if (!apiEnabled || !state.overlayEnabled) return;
  const bounds = map.getBounds();
  if (
    bounds.getNorth() - bounds.getSouth() > 0.45 ||
    bounds.getEast() - bounds.getWest() > 0.65
  ) {
    state.notice = "Zoom in to load the OSM overlay";
    setStatus();
    return;
  }
  try {
    overlayFeatures = await postApi("map-features", {
      south: bounds.getSouth(), west: bounds.getWest(),
      north: bounds.getNorth(), east: bounds.getEast(),
    });
    drawOverlay();
    state.notice = "";
    setStatus();
  } catch (error) {
    toast(`OSM overlay: ${error.message}`);
  }
}

export function drawOverlay() {
  overlayLayer.clearLayers();
  if (!apiEnabled || !state.overlayEnabled) return;
  for (const railway of overlayFeatures.railways) {
    L.polyline(railway.coordinates, {
      color: "#9cabbc",
      weight: 1.8,
      dashArray: "8 4",
      opacity: 0.75,
      interactive: false,
    }).addTo(overlayLayer);
  }
  for (const station of overlayFeatures.stations) {
    L.circleMarker(station.coordinate, {
      radius: 4,
      color: "#101722",
      weight: 1.5,
      fillColor: "#d8e4ef",
      fillOpacity: 0.95,
    })
      .bindTooltip(tooltipContent(station.name), {
        direction: "top",
        className: "station-label",
      })
      .addTo(overlayLayer);
  }
}

map.on("moveend", () => {
  clearTimeout(overlayTimer);
  overlayTimer = setTimeout(refreshOverlay, 500);
});
