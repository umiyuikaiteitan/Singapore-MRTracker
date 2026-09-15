/** Street and corridor matching: the /api/route-snap round trip for one segment. */

import { supportsLocalRail, requestRailMatch } from "./rail-matching.js";
import { apiEnabled, postApi } from "./config.js";
import { G } from "./geom.js";
import { modeRules, lineRadius } from "./modes.js";
import { state, lineById, afterGeometryChange } from "./state.js";
import { toast, setStatus } from "./ui.js";
import { beginSnap, snapIsCurrent } from "./snap-guard.js";

// --------------------------------------------------------- snapping

async function requestSnap(start, end, snapKind, line) {
  if (!apiEnabled && snapKind === "corridor" && supportsLocalRail(line.mode)) return requestRailMatch(start, end, line.mode);
  return postApi("route-snap", {
    start,
    end,
    mode: snapKind === "corridor" ? "rail" : "road",
    transitMode: line.mode,
    minimumRadius: Math.max(10, Math.min(2000, lineRadius(line))),
  });
}

/**
 * Match segment `index` of `line` to a road or rail corridor. The
 * segment's endpoint nodes are never changed — the matched geometry is
 * anchored between them.
 */
export async function snapSegment(line, index, snapKind) {
  if (!apiEnabled && !(snapKind === "corridor" && supportsLocalRail(line.mode))) {
    toast("This matching mode needs the optional matching service.");
    return;
  }
  const start = line.nodes[index];
  const end = line.nodes[index + 1];
  if (!start || !end) return;
  if (!modeRules(line.mode).snaps.includes(snapKind)) return;
  // Track the segment by identity: its index can shift while the
  // request is in flight (inserted or prepended nodes).
  const segmentRef = line.segments[index];
  if (!segmentRef) return;
  // …and by generation: a newer match for the same segment supersedes
  // this one even though the segment itself is unchanged.
  const token = beginSnap(segmentRef, start, end);
  state.pendingSnaps += 1;
  setStatus();
  try {
    const result = await requestSnap(start, end, snapKind, line);
    const target = lineById(line.id);
    const currentIndex = target ? target.segments.indexOf(segmentRef) : -1;
    if (currentIndex < 0) return;
    const from = target.nodes[currentIndex];
    const to = target.nodes[currentIndex + 1];
    if (!from || !to || !snapIsCurrent(token, from, to)) return;
    const guide =
      snapKind === "road" ? result.guideCoordinates : result.coordinates;
    // "road" guides are re-rounded client-side; corridor geometry
    // (profile "rail") is kept exact.
    segmentRef.profile = snapKind === "road" ? "road" : "rail";
    segmentRef.guide = G.anchoredGuide(guide, from, to);
    const notes = [];
    if (result.simplified) {
      notes.push(`rail geometry simplified (${result.originalPointCount} pts)`);
    }
    if (result.corridorCollapsed) notes.push("parallel tracks merged");
    if (result.removedControls) {
      notes.push(`${result.removedControls} tight corner(s) removed`);
    }
    state.notice = notes.join(", ");
    afterGeometryChange(target);
  } catch (error) {
    toast(`${snapKind === "road" ? "Street" : "Corridor"} matching failed: ${error.message}. Segment stays manual.`);
  } finally {
    state.pendingSnaps -= 1;
    setStatus();
  }
}
