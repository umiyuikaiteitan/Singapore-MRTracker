/** The OSM reference overlay: existing railways and stations for the viewport. */

import { currentBasemap, onBasemapData, refreshBasemap } from "./basemap.js";
import { state } from "./state.js";
import { overlayLayer } from "./map-setup.js";
import { tooltipContent } from "./ui.js";

// ------------------------------------------------------- OSM overlay

export async function refreshOverlay() {
  drawOverlay();
  if (state.overlayEnabled && !currentBasemap().stations.length) await refreshBasemap();
}

export function drawOverlay() {
  overlayLayer.clearLayers();
  if (!state.overlayEnabled) return;
  const overlayFeatures = currentBasemap();
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

onBasemapData(drawOverlay);
