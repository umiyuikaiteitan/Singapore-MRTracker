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

export function buildQuery(bounds, zoom = 13, profile = "boundaries") {
  const [south, west, north, east] = bounds;
  if (bounds.length !== 4 || !bounds.every(Number.isFinite)
      || south < -90 || north > 90 || west < -180 || east > 180
      || south >= north || west >= east) throw new Error("Move the map inside the world bounds to load OSM data");
  if (north - south > 0.45 || east - west > 0.65 || zoom < 12) {
    throw new Error("Zoom in to load OSM data (city scale or closer)");
  }
  if (!["rails", "boundaries", "roads"].includes(profile)) throw new Error("Unknown OSM query profile");
  const detail = profile;
  const bbox = bounds.join(",");
  return { key: detail + ":" + bbox, detail, query: `[out:json][timeout:20][maxsize:67108864];
(
  ${profile === "roads" ? `way["highway"~"^(motorway|trunk|primary|secondary|tertiary|unclassified|residential|living_street|service|busway|cycleway|path|footway|pedestrian|track)(_link)?$"](${bbox});` : `way["railway"~"^(rail|narrow_gauge|subway|light_rail|tram|monorail|funicular|disused|abandoned)$"](${bbox});`}
  ${profile === "boundaries" ? `wr["boundary"="administrative"](${bbox});
  wr["natural"~"^(coastline|water|wood|scrub|heath|grassland|wetland|bare_rock|sand|beach|glacier)$"](${bbox});
  wr["landuse"~"^(forest|grass|meadow|farmland|orchard|vineyard|reservoir)$"](${bbox});
  wr["leisure"="park"](${bbox});` : ""}
);
out ${profile === "roads" ? "body" : "tags"} geom(${bbox});` };
}

export function emptyFeatures() { return { railways: [], borders: [], terrain: [] }; }

export function parseFeatures(body, profile = "boundaries") {
  if (!body || typeof body !== "object" || !Array.isArray(body.elements)) throw new Error("Overpass returned an invalid map response");
  if (body.remark) throw new Error("Overpass could not finish this viewport; zoom in or retry later");
  if (body.elements.length > 20000) throw new Error("Too many OSM features; zoom in");
  const result = profile === "roads" ? { roads: [] } : emptyFeatures();
  const seen = new Set();
  let pointCount = 0, featureCount = 0;
  function append(raw, id, name, category, kind) {
    if (!Array.isArray(raw)) return;
    pointCount += raw.length;
    if (pointCount > 150000) throw new Error("OSM geometry is too detailed; zoom in");
    // Clipped relation members contain gaps. Draw each valid run separately;
    // never close a partial ring or connect across missing geometry.
    let run = [];
    function flush() {
      if (run.length >= 2) {
        const key = category + ":" + id + ":" + JSON.stringify(run);
        if (!seen.has(key)) {
          seen.add(key);
          if (++featureCount > 20000) throw new Error("Too many OSM features; zoom in");
          result[category].push({ id, name, kind, coordinates: run });
        }
      }
      run = [];
    }
    for (const point of raw) {
      const value = coordinate(point);
      if (value) run.push(value); else flush();
    }
    flush();
  }
  if (profile === "roads") {
    for (const element of body.elements) {
      if (element?.type !== "way" || !element.tags?.highway || !Array.isArray(element.geometry) || !Array.isArray(element.nodes)) continue;
      const raw = element.geometry;
      pointCount += raw.length;
      if (pointCount > 150000) throw new Error("OSM geometry is too detailed; zoom in");
      if (raw.length !== element.nodes.length) throw new Error("OSM road geometry has inconsistent node references");
      let coordinates = [], nodeIds = [];
      const flush = () => {
        if (coordinates.length >= 2) {
          if (++featureCount > 20000) throw new Error("Too many OSM features; zoom in");
          result.roads.push({id: "way/" + element.id, kind: element.tags.highway, tags: element.tags, coordinates, nodeIds});
        }
        coordinates = []; nodeIds = [];
      };
      raw.forEach((point, i) => {
        const value = coordinate(point), id = element.nodes[i];
        if (value && Number.isSafeInteger(id)) { coordinates.push(value); nodeIds.push(id); } else flush();
      });
      flush();
    }
    return result;
  }
  for (const element of body.elements) {
    if (!element || !["way", "relation"].includes(element.type)) continue;
    const tags = element.tags || {};
    let category;
    if (tags.boundary === "administrative") category = "borders";
    else if (/^(coastline|water|wood|scrub|heath|grassland|wetland|bare_rock|sand|beach|glacier)$/.test(tags.natural)
      || /^(forest|grass|meadow|farmland|orchard|vineyard|reservoir)$/.test(tags.landuse)
      || tags.leisure === "park") category = "terrain";
    else if (element.type === "way" && /^(rail|narrow_gauge|subway|light_rail|tram|monorail|funicular|disused|abandoned)$/.test(tags.railway)) category = "railways";
    else continue;
    const name = String(tags.name || tags["name:en"] || tags.ref || "");
    if (element.type === "way") append(element.geometry, "way/" + element.id, name, category, tags.railway);
    else if (Array.isArray(element.members)) {
      for (const member of element.members) {
        if (member?.type === "way" && ["", "outer", "inner"].includes(member.role || "")) {
          append(member.geometry, "way/" + member.ref, name, category, tags.railway);
        }
      }
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

export function createOverpassClient({ fetchImpl = globalThis.fetch, now = Date.now, minInterval = 2000, gate = { lastStarted: -Infinity, retryAt: 0 } } = {}) {
  const cache = new Map();
  let active = null;
  function cancel() { active?.controller.abort(); active = null; }
  function get(bounds, zoom, profile = "boundaries") {
    const request = buildQuery(bounds, zoom, profile);
    if (active?.key === request.key) return active.promise;
    cancel();
    for (const [key, entry] of cache) {
      if (now() - entry.at >= CACHE_TTL) { cache.delete(key); continue; }
      if (entry.detail === request.detail && entry.bounds[0] <= bounds[0] && entry.bounds[1] <= bounds[1]
          && entry.bounds[2] >= bounds[2] && entry.bounds[3] >= bounds[3]) return Promise.resolve(entry.features);
    }
    if (now() < gate.retryAt) return Promise.reject(new Error("Overpass is busy; retry in " + Math.ceil((gate.retryAt - now()) / 1000) + " seconds"));
    const controller = new AbortController();
    const current = { key: request.key, controller, promise: null };
    current.promise = (async () => {
      do {
        await wait(Math.max(0, gate.lastStarted + minInterval - now()), controller.signal);
        controller.signal.throwIfAborted();
      } while (now() < gate.lastStarted + minInterval);
      if (now() < gate.retryAt) throw new Error("Overpass is busy; retry later");
      gate.lastStarted = now();
      const timer = setTimeout(() => controller.abort(new Error("Overpass timed out; zoom in or retry later")), 30000);
      try {
        const response = await fetchImpl(OVERPASS_ENDPOINT, {
          method: "POST", credentials: "omit", signal: controller.signal,
          body: new URLSearchParams({ data: request.query }),
        });
        if (!response.ok) {
          if (response.status === 429 || response.status === 504 || response.status === 503) {
            const seconds = Number(response.headers.get("Retry-After")) || 60;
            gate.retryAt = now() + Math.max(30, Math.min(300, seconds)) * 1000;
          }
          await response.body?.cancel();
          throw new Error("Overpass is unavailable (" + response.status + "); retry later");
        }
        const features = parseFeatures(await readJson(response, controller.signal), profile);
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
const serviceGate = { lastStarted: -Infinity, retryAt: 0 };
export const overpass = createOverpassClient({ gate: serviceGate });
export const matchingOverpass = createOverpassClient({ gate: serviceGate });

export const roadOverpass = createOverpassClient({ gate: serviceGate });
