/** The Leaflet map, its browser-rendered basemap, and the layer groups everything else draws into. */

import { config } from "./config.js";

// ------------------------------------------------------------- map

export const map = L.map("map", { zoomControl: true, minZoom: 3, maxZoom: 20 }).setView(config.initialView.center, config.initialView.zoom);
map.attributionControl.setPosition("topright");
map.attributionControl.addAttribution('&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors');
map.createPane("osm-base").style.zIndex = 200;
export const baseRenderer = L.canvas({ pane: "osm-base" });
export const baseLayer = L.layerGroup().addTo(map);

map.createPane("osm-highlight").style.zIndex = 300;
export const overlayLayer = L.layerGroup().addTo(map);
export const routeLayer = L.layerGroup().addTo(map);
export const issueLayer = L.layerGroup().addTo(map);
export const nodeLayer = L.layerGroup().addTo(map);
export const stationLayer = L.layerGroup().addTo(map);
export const draftLayer = L.layerGroup().addTo(map);
