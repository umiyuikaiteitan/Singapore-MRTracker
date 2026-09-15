/** The Leaflet map, its tiles, and the layer groups everything else draws into. */

import { config } from "./config.js";

// ------------------------------------------------------------- map

export const map = L.map("map", { zoomControl: true }).setView(config.initialView.center, config.initialView.zoom);
L.tileLayer("https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png", {
  maxZoom: 19,
  attribution:
    '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors &copy; <a href="https://carto.com/">CARTO</a>',
}).addTo(map);

export const overlayLayer = L.layerGroup().addTo(map);
export const routeLayer = L.layerGroup().addTo(map);
export const issueLayer = L.layerGroup().addTo(map);
export const nodeLayer = L.layerGroup().addTo(map);
export const stationLayer = L.layerGroup().addTo(map);
export const draftLayer = L.layerGroup().addTo(map);
