/** Rendering the current display, viewport-sized, to a standalone SVG file. */

import { getViewportFeatures, getBasemapMode } from "./basemap.js";
import { G } from "./geom.js";
import { labelSide, labelHidden, stationLabelText } from "./model.js";
import { state, lineGeometry } from "./state.js";
import { map } from "./map-setup.js";
import { toast } from "./ui.js";
import { lineVisible, lineStrokeStyle } from "./render.js";
import { interlineSlot } from "./topology.js";
import { downloadFile } from "./geojson.js";

// -------------------------------------------------------- SVG render

function escapeXml(value) {
  return String(value).replace(/[<>&'"]/g, (character) => ({
    "<": "&lt;",
    ">": "&gt;",
    "&": "&amp;",
    "'": "&apos;",
    '"': "&quot;",
  })[character]);
}

const svgPath = (coordinates) =>
  coordinates
    .map((coordinate, index) => {
      const point = map.latLngToContainerPoint(coordinate);
      return `${index === 0 ? "M" : "L"}${point.x.toFixed(2)} ${point.y.toFixed(2)}`;
    })
    .join(" ");

// Station labels. Placement is the station's own side; the numbers are
// the disc radius (5.4) plus clearance, and the dark stroke under the
// text is the SVG equivalent of `.station-label`'s shadow.
const LABEL_GAP = 10;
const LABEL_SIZE = 11;
const LABEL_FILL = "#e8eef5"; // mirrors `--text` in static/style.css
const LABEL_HALO = "#050a13"; // the casing colour, used as a text halo

/** `<text>` for one station name, placed on the side it asks for. */
function stationLabelMarkup(point, station) {
  if (labelHidden(station)) return "";
  const side = labelSide(station);
  const place =
    side === "left"
      ? { x: point.x - LABEL_GAP, y: point.y + 4, anchor: "end" }
      : side === "right"
        ? { x: point.x + LABEL_GAP, y: point.y + 4, anchor: "start" }
        : side === "bottom"
          ? { x: point.x, y: point.y + LABEL_GAP + LABEL_SIZE * 0.8, anchor: "middle" }
          : { x: point.x, y: point.y - LABEL_GAP, anchor: "middle" };
  return (
    `<text x="${place.x.toFixed(2)}" y="${place.y.toFixed(2)}" ` +
    `text-anchor="${place.anchor}" fill="${LABEL_FILL}" ` +
    `stroke="${LABEL_HALO}" stroke-width="3" paint-order="stroke" ` +
    `font-family="sans-serif" font-size="${LABEL_SIZE}">` +
    `${escapeXml(stationLabelText(station))}</text>`
  );
}

/** Render the current display (viewport-sized) to a standalone SVG. */
export async function exportSvg() {
  const button = document.getElementById("export-svg");
  button.disabled = true;
  try {
    const exportMode = getBasemapMode();
    let featureData = { railways: [], borders: [], terrain: [] };
    let hasOsm = false;
    try {
      featureData = await getViewportFeatures();
      hasOsm = true;
    } catch (_) {
      // An unavailable public service must not prevent exporting the project.
    }
    const size = map.getSize();
    const terrainMarkup = featureData.terrain.map(terrain =>
      `<path d="${svgPath(terrain.coordinates)}" stroke="#53877a" stroke-width="1"><title>${escapeXml(terrain.name)}</title></path>`
    ).join("");
    const borderMarkup = featureData.borders.map(border =>
      `<path d="${svgPath(border.coordinates)}" stroke="#a18db3" stroke-width="1.3" stroke-dasharray="6 4"><title>${escapeXml(border.name)}</title></path>`
    ).join("");
    const railMarkup = featureData.railways
      .map((railway) => {
        const path = svgPath(railway.coordinates);
        return (
          `<path d="${path}" stroke="#101823" stroke-width="4.4" />` +
          `<path d="${path}" stroke="#9cabbc" stroke-width="1.8" stroke-dasharray="10 3" opacity=".9"><title>${escapeXml(railway.name)}</title></path>`
        );
      })
      .join("");
    const transitMarkup = state.lines
      .filter((line) => lineVisible(line))
      .flatMap((line) => {
        const geometry = lineGeometry(line);
        const baseStyle = lineStrokeStyle(line);
        return geometry.segmentPoints.flatMap((points, index) => {
          if (points.length < 2) return [];
          const path = svgPath(points);
          const slot = interlineSlot(line, line.segments[index]);
          const dash = slot
            ? ` stroke-dasharray="12 ${12 * (slot.count - 1)}" stroke-dashoffset="${12 * slot.index}"`
            : baseStyle.dashArray
              ? ` stroke-dasharray="${baseStyle.dashArray}"`
              : "";
          return [
            `<path d="${path}" stroke="#050a13" stroke-width="${baseStyle.weight * 2}" />`,
            `<path d="${path}" stroke="${escapeXml(line.color)}" stroke-width="${baseStyle.weight}"${dash}><title>${escapeXml(line.name)}</title></path>`,
          ];
        });
      })
      .join("");
    const userStations = state.lines
      .filter((line) => lineVisible(line))
      .flatMap((line) => {
        const geometry = lineGeometry(line);
        return line.stations.map((station) => {
          const anchor = G.pointAtFraction(geometry.points, station.t);
          if (!anchor) return "";
          let polygonMarkup = "";
          if (station.polygon && station.polygon.length >= 3) {
            const ring = station.polygon
              .map((offset) => {
                const point = map.latLngToContainerPoint(
                  G.offsetToCoordinate(anchor, offset),
                );
                return `${point.x.toFixed(2)},${point.y.toFixed(2)}`;
              })
              .join(" ");
            polygonMarkup = `<polygon points="${ring}" fill="${escapeXml(line.color)}" fill-opacity=".22" stroke="${escapeXml(line.color)}" stroke-width="2" />`;
          }
          const point = map.latLngToContainerPoint(anchor);
          return (
            polygonMarkup +
            `<circle cx="${point.x.toFixed(2)}" cy="${point.y.toFixed(2)}" r="5.4" fill="#09101b" stroke="${escapeXml(line.color)}" stroke-width="2.4"><title>${escapeXml(station.name)}</title></circle>` +
            stationLabelMarkup(point, station)
          );
        });
      })
      .join("");
    const svg = `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="${size.x}" height="${size.y}" viewBox="0 0 ${size.x} ${size.y}">
<title>OpenFantasyMap export</title>
<desc>${hasOsm ? "Display-sized map of OpenStreetMap features and authored transit services." : "Authored transit lines and stations; OSM base features are not included."}</desc>
<rect width="100%" height="100%" fill="#0b121d"/>
<g fill="none" stroke-linecap="round" stroke-linejoin="round">${terrainMarkup}${borderMarkup}</g>
<g fill="none" stroke-linecap="round" stroke-linejoin="round">${railMarkup}</g>
<g fill="none" stroke-linecap="round" stroke-linejoin="round">${transitMarkup}</g>
<g>${userStations}</g>
<text x="${size.x - 12}" y="${size.y - 12}" text-anchor="end" fill="#8b9aab" font-family="sans-serif" font-size="10">© OpenStreetMap contributors · OpenFantasyMap</text>
</svg>`;
    downloadFile(svg, `transit-map-${size.x}x${size.y}.svg`, "image/svg+xml");
    toast(`SVG rendered at ${size.x} × ${size.y}${hasOsm ? (exportMode === "tiles" ? " with OSM rail lines; tile background omitted" : " with OSM context") : exportMode === "tiles" ? " — project only; tile background omitted" : " — project only; OSM unavailable"}.`);
  } catch (error) {
    toast(`SVG render failed: ${error.message}`);
  } finally {
    button.disabled = false;
  }
}
