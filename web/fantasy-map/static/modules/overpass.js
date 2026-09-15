/** Bounded, browser-side Overpass queries. No tiles, keys, or application proxy. */
export const OVERPASS_ENDPOINT = "https://overpass-api.de/api/interpreter";
const MAX_BYTES = 8 * 1024 * 1024;
const CACHE_TTL = 5 * 60 * 1000;
const MAX_ENTRIES = 4;
const coordinate = (point) => point && Number.isFinite(point.lat) && Number.isFinite(point.lon)
  && Math.abs(point.lat) <= 90 && Math.abs(point.lon) <= 180 ? [point.lat, point.lon] : null;

export function viewportBounds(bounds) {
  return [bounds.getSouth(), bounds.getWest(), bounds.getNorth(), bounds.getEast()];
}

export function buildQuery(bounds, zoom = 13) {
  const [south, west, north, east] = bounds;
  if (bounds.length !== 4 || !bounds.every(Number.isFinite)
      || south < -90 || north > 90 || west < -180 || east > 180
      || south >= north || west >= east) throw new Error("Move the map inside the world bounds to load OSM data");
  if (north - south > 0.45 || east - west > 0.65 || zoom < 12) {
    throw new Error("Zoom in to load OSM data (city scale or closer)");
  }
  const detail = zoom >= 16 ? "street" : zoom >= 14 ? "district" : "city";
  const roads = "motorway|trunk|primary|secondary" + (zoom >= 14 ? "|tertiary" : "")
    + (zoom >= 16 ? "|residential|unclassified|service|living_street|pedestrian|cycleway" : "");
  const bbox = bounds.join(",");
  return { key: detail + ":" + bbox, detail, query: `[out:json][timeout:20][maxsize:67108864];
(
  way["highway"~"^(${roads})(_link)?$"](${bbox});
  way["railway"~"^(rail|subway|light_rail|tram|monorail)$"](${bbox});
  way["waterway"~"^(river|canal|stream)$"](${bbox});
  way["natural"~"^(water|wood)$"](${bbox});
  way["landuse"~"^(forest|grass|recreation_ground|meadow)$"](${bbox});
  way["leisure"="park"](${bbox});
  nwr["railway"~"^(station|halt|tram_stop)$"](${bbox});
  nwr["public_transport"="station"](${bbox});
);
out tags geom;` };
}

export function parseFeatures(body) {
  if (!body || typeof body !== "object") throw new Error("Overpass returned an invalid map response");
  if (body.remark) throw new Error("Overpass could not finish this viewport; zoom in or retry later");
  if (!Array.isArray(body.elements)) throw new Error("Overpass returned an invalid map response");
  if (body.elements.length > 20000) throw new Error("Too many OSM features; zoom in");
  const result = { roads: [], railways: [], stations: [], waterways: [], areas: [] };
  const seen = new Set();
  let pointCount = 0;
  for (const element of body.elements) {
    if (!element || !["node", "way", "relation"].includes(element.type)) continue;
    const id = element.type + "/" + element.id;
    if (seen.has(id)) continue;
    seen.add(id);
    const tags = element.tags || {};
    const raw = Array.isArray(element.geometry) ? element.geometry : [];
    pointCount += raw.length;
    if (pointCount > 150000) throw new Error("OSM geometry is too detailed; zoom in");
    // Never bridge across missing/malformed geometry points.
    const coordinates = raw.map(coordinate);
    const validWay = element.type === "way" && coordinates.length >= 2 && coordinates.every(Boolean);
    const name = String(tags.name || tags["name:en"] || tags.ref || "");
    if (validWay) {
      const feature = { id, name, coordinates };
      if (tags.highway) result.roads.push({ ...feature, kind: tags.highway });
      if (/^(rail|subway|light_rail|tram|monorail)$/.test(tags.railway)) result.railways.push(feature);
      if (tags.waterway) result.waterways.push(feature);
      const closed = coordinates.length >= 4 && coordinates[0][0] === coordinates.at(-1)[0]
        && coordinates[0][1] === coordinates.at(-1)[1];
      if (closed && (tags.natural === "water" || tags.natural === "wood" || tags.landuse || tags.leisure === "park")) {
        result.areas.push({ ...feature, kind: tags.natural === "water" ? "water" : "green" });
      }
    }
    if (/^(station|halt|tram_stop)$/.test(tags.railway) || tags.public_transport === "station") {
      let point = coordinate(element) || coordinate(element.center);
      if (!point && validWay) {
        point = coordinates.reduce((sum, p) => [sum[0] + p[0] / coordinates.length, sum[1] + p[1] / coordinates.length], [0, 0]);
      }
      if (!point && element.bounds) {
        const b = element.bounds;
        point = coordinate({ lat: (b.minlat + b.maxlat) / 2, lon: (b.minlon + b.maxlon) / 2 });
      }
      if (point) result.stations.push({ id, name: name || "Mapped station", coordinate: point });
    }
  }
  return result;
}

async function readJson(response, signal) {
  if (Number(response.headers.get("Content-Length")) > MAX_BYTES) {
    await response.body?.cancel();
    throw new Error("OSM response is too large; zoom in");
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let size = 0, text = "";
  try {
    while (true) {
      signal.throwIfAborted();
      const { value, done } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > MAX_BYTES) throw new Error("OSM response is too large; zoom in");
      text += decoder.decode(value, { stream: true });
    }
    return JSON.parse(text + decoder.decode());
  } finally { await reader.cancel(); }
}

function wait(ms, signal) {
  if (ms <= 0) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const abort = () => { clearTimeout(timer); reject(signal.reason); };
    const timer = setTimeout(() => { signal.removeEventListener("abort", abort); resolve(); }, ms);
    signal.addEventListener("abort", abort, { once: true });
    if (signal.aborted) abort();
  });
}

export function createOverpassClient({ fetchImpl = globalThis.fetch, now = Date.now, minInterval = 2000 } = {}) {
  const cache = new Map();
  let active = null, lastStarted = -Infinity, retryAt = 0;
  function cancel() { active?.controller.abort(); active = null; }
  function get(bounds, zoom) {
    const request = buildQuery(bounds, zoom);
    if (active?.key === request.key) return active.promise;
    cancel();
    for (const [key, entry] of cache) {
      if (now() - entry.at >= CACHE_TTL) { cache.delete(key); continue; }
      if (entry.detail === request.detail && entry.bounds[0] <= bounds[0] && entry.bounds[1] <= bounds[1]
          && entry.bounds[2] >= bounds[2] && entry.bounds[3] >= bounds[3]) return Promise.resolve(entry.features);
    }
    if (now() < retryAt) return Promise.reject(new Error("Overpass is busy; retry in " + Math.ceil((retryAt - now()) / 1000) + " seconds"));
    const controller = new AbortController();
    const current = { key: request.key, controller, promise: null };
    current.promise = (async () => {
      await wait(Math.max(0, lastStarted + minInterval - now()), controller.signal);
      controller.signal.throwIfAborted();
      lastStarted = now();
      const timer = setTimeout(() => controller.abort(new Error("Overpass timed out; zoom in or retry later")), 30000);
      try {
        const response = await fetchImpl(OVERPASS_ENDPOINT, {
          method: "POST", credentials: "omit", signal: controller.signal,
          body: new URLSearchParams({ data: request.query }),
        });
        if (!response.ok) {
          if (response.status === 429 || response.status === 504 || response.status === 503) {
            const seconds = Number(response.headers.get("Retry-After")) || 60;
            retryAt = now() + Math.max(30, Math.min(300, seconds)) * 1000;
          }
          await response.body?.cancel();
          throw new Error("Overpass is unavailable (" + response.status + "); retry later");
        }
        const features = parseFeatures(await readJson(response, controller.signal));
        controller.signal.throwIfAborted();
        cache.set(request.key, { at: now(), bounds: [...bounds], detail: request.detail, features });
        while (cache.size > MAX_ENTRIES) cache.delete(cache.keys().next().value);
        return features;
      } finally { clearTimeout(timer); }
    })().finally(() => { if (active === current) active = null; });
    active = current;
    return current.promise;
  }
  return { get, cancel };
}
export const overpass = createOverpassClient();
