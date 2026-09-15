/** The eight transit modes and the snapping, curve, and style rules each carries. */

/**
 * Transit modes and their snapping/geometry rules.
 * - `snaps`: which snap tools apply ("road" = street network,
 *   "corridor" = the mode's own OSM corridor; cableways are manual only).
 * - `radius`: default minimum curve radius in metres.
 * - `straight`: straight spans between nodes, no fillets (cableways).
 * - `dash`/`weight`: base rendering style.
 */
export const MODES = {
  Mainline: { snaps: ["corridor", "road"], radius: 400 },
  Metro: { snaps: ["corridor", "road"], radius: 150 },
  Tram: { snaps: ["corridor", "road"], radius: 50 },
  BRT: { snaps: ["road"], radius: 60 },
  Monorail: { snaps: ["corridor", "road"], radius: 100 },
  Cableway: { snaps: [], radius: 1000, straight: true, dash: "2 8" },
  Ferry: { snaps: ["corridor"], radius: 500, dash: "10 8" },
  Cycleway: { snaps: ["road"], radius: 25, weight: 3 },
};
export const MODE_NAMES = Object.keys(MODES);
// v1 projects used a coarser mode list.
export const LEGACY_MODES = { Rail: "Mainline" };

export const modeRules = (mode) => MODES[mode] || MODES.Mainline;
export const lineRadius = (line) =>
  Number.isFinite(line.minRadius) ? line.minRadius : modeRules(line.mode).radius;
