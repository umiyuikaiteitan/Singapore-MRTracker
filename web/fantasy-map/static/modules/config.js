/** Deployment settings; null apiBase explicitly selects the static editor. */
export function resolveConfig(raw = {}, base = "http://localhost/") {
  const value = raw.apiBase === undefined ? "api/" : raw.apiBase;
  let apiBase = null;
  if (value !== null && value !== "") {
    const url = new URL(value, base);
    if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || url.search || url.hash) {
      throw new Error("The API base must be an HTTP(S) URL without credentials, query, or fragment");
    }
    if (!url.pathname.endsWith("/")) url.pathname += "/";
    apiBase = url.href;
  }
  return {
    apiBase,
    initialView: raw.initialView || { center: [51.505, -0.09], zoom: 13 },
    boardHref: raw.boardHref || null,
    singaporeGtfsUrl: raw.singaporeGtfsUrl || "https://umiyui.dev/fantasy-map/singapore-mrt.zip",
  };
}
export const config = resolveConfig(globalThis.OFM_CONFIG, globalThis.document?.baseURI);
export const apiEnabled = config.apiBase !== null;
export async function postApi(endpoint, data) {
  if (!apiEnabled) throw new Error("OSM matching is unavailable in this edition");
  const response = await fetch(new URL(endpoint, config.apiBase), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
    signal: AbortSignal.timeout(65000),
  });
  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    throw new Error(body.detail || `Map service failed (${response.status})`);
  }
  return response.json();
}
