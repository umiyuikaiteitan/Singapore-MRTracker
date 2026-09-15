/** The OSM reference overlay: highlighted railways for the viewport. */

import { currentBasemap, onBasemapData } from "./basemap.js";
import { state } from "./state.js";
import { overlayLayer } from "./map-setup.js";

// ------------------------------------------------------- OSM overlay

export async function refreshOverlay() {
  drawOverlay();
}

export function drawOverlay() {
  overlayLayer.clearLayers();
  if (!state.overlayEnabled) return;
  const overlayFeatures = currentBasemap();
  for (const railway of overlayFeatures.railways) {
    L.polyline(railway.coordinates, {
      pane: "osm-highlight",
      color: "#9cabbc",
      weight: 1.8,
      dashArray: "8 4",
      opacity: 0.75,
      interactive: false,
    }).addTo(overlayLayer);
  }

}

onBasemapData(drawOverlay);
