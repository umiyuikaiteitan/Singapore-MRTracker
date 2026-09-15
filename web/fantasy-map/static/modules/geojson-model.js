/**
 * The pure half of GeoJSON I/O: features built from lines, and features
 * parsed back into lines. Data in, data out — no DOM, no Leaflet, no
 * editor state, no history or toasts. `geojson.js` wraps these with the
 * file handling and the editor side effects, and the Node tests exercise
 * the round trip through this module alone.
 */

import { G } from "./geom.js";
import { LEGACY_MODES, lineRadius } from "./modes.js";
import {
  PALETTE, uid, makeLine, routeGeometry, normalizeLabel,
} from "./model.js";

const toLngLat = ([lat, lng]) => [lng, lat];
const toLatLng = ([lng, lat]) => [lat, lng];

/** How far a foreign station may sit from a route and still attach. */
export const ATTACH_METERS = 500;

/** The features to import from a parsed document, whatever wraps them. */
export function collectFeatures(data) {
  if (!data || typeof data !== "object") return [];
  if (data.type === "FeatureCollection") return data.features || [];
  if (data.type === "Feature") return [data];
  if (data.type === "LineString" || data.type === "Point") {
    return [{ type: "Feature", geometry: data, properties: {} }];
  }
  return [];
}

/**
 * Every feature for a project: each line as a LineString of its
 * displayed points carrying the editable model in `properties`, then its
 * stations as Points or, where they have corners, Polygons. `geometryOf`
 * lets the editor pass its memoised geometry; on its own the module
 * derives geometry from the line.
 */
export function buildFeatures(lines, geometryOf = routeGeometry) {
  const features = [];
  for (const line of lines || []) {
    const geometry = geometryOf(line);
    features.push({
      type: "Feature",
      geometry: {
        type: "LineString",
        coordinates: geometry.points.map(toLngLat),
      },
      properties: {
        ofm: "line",
        id: line.id,
        name: line.name,
        mode: line.mode,
        minRadius: lineRadius(line),
        color: line.color,
        visible: line.visible,
        nodes: line.nodes,
        segments: line.segments,
        branchOf: line.branchOf,
      },
    });
    for (const station of line.stations || []) {
      const anchor = G.pointAtFraction(geometry.points, station.t);
      if (!anchor) continue;
      if (station.polygon && station.polygon.length >= 3) {
        const ring = station.polygon.map((offset) =>
          toLngLat(G.offsetToCoordinate(anchor, offset)),
        );
        ring.push(ring[0]);
        features.push({
          type: "Feature",
          geometry: { type: "Polygon", coordinates: [ring] },
          properties: {
            ofm: "station-area",
            id: station.id,
            lineId: line.id,
            name: station.name,
            // `code` and `label` are dropped from the JSON when unset or
            // default: an undefined value has no key once stringified.
            code: station.code,
            label: normalizeLabel(station.label),
            t: station.t,
            offsets: station.polygon,
          },
        });
      } else {
        features.push({
          type: "Feature",
          geometry: { type: "Point", coordinates: toLngLat(anchor) },
          properties: {
            ofm: "station",
            id: station.id,
            lineId: line.id,
            name: station.name,
            code: station.code,
            label: normalizeLabel(station.label),
            t: station.t,
          },
        });
      }
    }
  }
  return features;
}

/**
 * Parse features into lines, appending to `options.lines` (the project
 * being imported into, so a partial import lands beside what is already
 * drawn) and returning it with the counts the caller reports.
 *
 * LineStrings become lines: an OpenFantasyMap export restores its own
 * nodes and segments, a foreign one keeps its rendered coordinates as
 * manual nodes. Points and Polygons become stations on the line their
 * `lineId` names, or on the nearest route within `attachMeters`.
 * Anything else is counted in `skipped`.
 *
 * Parsing is atomic: features are validated into a staging area and the
 * project is only appended to once the whole document has parsed, so a
 * feature that cannot be read at all leaves nothing half-imported.
 */
export function parseFeatures(features, options = {}) {
  const lines = options.lines || [];
  const geometryOf = options.geometryOf || routeGeometry;
  const attachMeters = Number.isFinite(options.attachMeters)
    ? options.attachMeters
    : ATTACH_METERS;
  const created = [];
  const pending = new Map(); // line -> stations parsed but not yet attached
  const stationFeatures = [];
  const idMap = new Map(); // exported line id -> new line id
  let imported = 0;
  let skipped = 0;

  const pendingStations = (line) => {
    if (!pending.has(line)) pending.set(line, []);
    return pending.get(line);
  };

  for (const feature of features || []) {
    const geometry = (feature && feature.geometry) || {};
    const properties = (feature && feature.properties) || {};
    if (geometry.type === "LineString") {
      const positions = finitePairs(geometry.coordinates);
      if (!positions || positions.length < 2) {
        skipped += 1;
        continue;
      }
      const rendered = positions.map(toLatLng);
      // A feature that is structurally one of ours but corrupt inside —
      // unusable nodes, malformed segments — is skipped with the rest,
      // never imported as far as it parses.
      let nodes = rendered;
      if (Array.isArray(properties.nodes) && properties.nodes.length >= 2) {
        nodes = finitePairs(properties.nodes);
        if (!nodes) {
          skipped += 1;
          continue;
        }
      }
      const segments = parseSegments(properties.segments);
      if (!segments) {
        skipped += 1;
        continue;
      }
      const ordinal = lines.length + created.length;
      const line = makeLine(
        {
          name: properties.name || `Imported ${ordinal + 1}`,
          mode: LEGACY_MODES[properties.mode] || properties.mode,
          minRadius: Number.isFinite(properties.minRadius)
            ? properties.minRadius
            : undefined,
          color: properties.color || PALETTE[ordinal % PALETTE.length],
          visible: properties.visible !== false,
          nodes,
          segments,
          // Copied, not aliased: the branch parent is remapped below,
          // and the features being read must not change under the caller.
          branchOf: properties.branchOf ? { ...properties.branchOf } : null,
        },
        ordinal,
      );
      created.push(line);
      if (properties.id) idMap.set(properties.id, line.id);
      imported += 1;
    } else if (geometry.type === "Point" || geometry.type === "Polygon") {
      stationFeatures.push(feature);
    } else {
      skipped += 1;
    }
  }

  // Remap branch parents to the new line ids.
  for (const line of created) {
    if (line.branchOf && idMap.has(line.branchOf.lineId)) {
      line.branchOf.lineId = idMap.get(line.branchOf.lineId);
    }
  }

  const targets = [...lines, ...created];
  for (const feature of stationFeatures) {
    const geometry = feature.geometry;
    const properties = feature.properties || {};
    const ring =
      geometry.type === "Polygon"
        ? finitePairs(geometry.coordinates && geometry.coordinates[0])
        : null;
    const anchor =
      geometry.type === "Point"
        ? pointAnchor(geometry.coordinates)
        : ringCentroid(ring);
    if (!anchor) {
      skipped += 1;
      continue;
    }
    const offsets = Array.isArray(properties.offsets)
      ? metreOffsets(properties.offsets)
      : null;
    if (Array.isArray(properties.offsets) && !offsets) {
      skipped += 1;
      continue;
    }
    const line =
      (properties.lineId && lineById(targets, idMap.get(properties.lineId))) ||
      nearestLineTo(anchor, targets, attachMeters, geometryOf);
    if (!line) {
      skipped += 1;
      continue;
    }
    const projection = G.projectToPolyline(anchor, geometryOf(line).points);
    if (!projection) {
      skipped += 1;
      continue;
    }
    const stations = pendingStations(line);
    stations.push({
      id: uid("station"),
      name:
        properties.name ||
        `Station ${line.stations.length + stations.length + 1}`,
      code: properties.code || undefined,
      label: normalizeLabel(properties.label),
      t: Number.isFinite(properties.t) ? properties.t : projection.t,
      coordinate: projection.coordinate,
      polygon:
        offsets ||
        (ring
          ? ring
              .slice(0, -1)
              .map((position) =>
                G.coordinateToOffset(projection.coordinate, toLatLng(position)),
              )
          : undefined),
    });
    imported += 1;
  }

  // Commit: everything parsed, nothing left to throw.
  for (const line of created) lines.push(line);
  for (const [line, stations] of pending) line.stations.push(...stations);

  return { lines, created, imported, skipped };
}

/**
 * `list` as pairs of finite numbers, or null when any entry is not one.
 * Every coordinate an import carries goes through here: a null or NaN
 * pair reaching the geometry library is either a throw or a route that
 * cannot be drawn.
 */
function finitePairs(list) {
  if (!Array.isArray(list)) return null;
  const pairs = [];
  for (const entry of list) {
    if (!Array.isArray(entry) || entry.length < 2) return null;
    if (!Number.isFinite(entry[0]) || !Number.isFinite(entry[1])) return null;
    pairs.push([entry[0], entry[1]]);
  }
  return pairs;
}

/** Station corner offsets, or null when any of them is unusable. */
function metreOffsets(list) {
  const offsets = [];
  for (const offset of list) {
    if (!offset || !Number.isFinite(offset.north) || !Number.isFinite(offset.east)) {
      return null;
    }
    offsets.push({ north: offset.north, east: offset.east });
  }
  return offsets;
}

/**
 * The segments of an exported line, or null when the property is an
 * array the model could not use. A non-array is treated as absent, as a
 * foreign file may carry the name for something else entirely.
 */
function parseSegments(value) {
  if (!Array.isArray(value)) return [];
  const segments = [];
  for (const segment of value) {
    if (!segment || typeof segment !== "object") return null;
    const guide =
      segment.guide === undefined || segment.guide === null
        ? []
        : finitePairs(segment.guide);
    if (!guide) return null;
    segments.push({
      profile: segment.profile || "manual",
      guide,
      interlineId: segment.interlineId,
    });
  }
  return segments;
}

const lineById = (lines, id) => lines.find((line) => line.id === id) || null;

/** A Point's anchor, or null when its coordinates are unusable. */
function pointAnchor(coordinates) {
  if (!Array.isArray(coordinates) || coordinates.length < 2) return null;
  const anchor = toLatLng(coordinates);
  return Number.isFinite(anchor[0]) && Number.isFinite(anchor[1]) ? anchor : null;
}

function ringCentroid(ring) {
  if (!Array.isArray(ring) || !ring.length) return null;
  const positions = ring.map(toLatLng);
  return [
    positions.reduce((sum, point) => sum + point[0], 0) / positions.length,
    positions.reduce((sum, point) => sum + point[1], 0) / positions.length,
  ];
}

function nearestLineTo(coordinate, lines, maximumMeters, geometryOf) {
  let best = null;
  for (const line of lines) {
    const projection = G.projectToPolyline(coordinate, geometryOf(line).points);
    if (!projection || projection.distanceMeters > maximumMeters) continue;
    if (!best || projection.distanceMeters < best.distance) {
      best = { line, distance: projection.distanceMeters };
    }
  }
  return best ? best.line : null;
}
