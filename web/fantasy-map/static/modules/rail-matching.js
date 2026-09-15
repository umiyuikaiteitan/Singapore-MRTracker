/** Follow connected OSM rail geometry locally, without inventing missing links. */
import { matchAlignment } from "./alignment-graph.js";
import { matchingOverpass } from "./overpass.js";

const railKinds = {
  Mainline: ['rail', 'narrow_gauge', 'disused', 'abandoned'],
  Metro: ['subway', 'light_rail'],
  Tram: ['tram', 'light_rail'],
  Monorail: ['monorail', 'funicular'],
};
export const supportsLocalRail = mode => Object.hasOwn(railKinds, mode);
export function matchRail(start, end, railways, mode) {
  if (!supportsLocalRail(mode)) throw new Error('This mode needs the matching service');
  return matchAlignment(start, end, railways, {accept: rail => railKinds[mode].includes(rail.kind)});
}

const client=matchingOverpass;
let queue=Promise.resolve();
export function requestRailMatch(start,end,mode) {
  const run=async()=>{
    const pad=0.01;
    const bounds=[Math.max(-90,Math.min(start[0],end[0])-pad),Math.max(-180,Math.min(start[1],end[1])-pad),Math.min(90,Math.max(start[0],end[0])+pad),Math.min(180,Math.max(start[1],end[1])+pad)];
    const data=await client.get(bounds,13,'rails');
    return matchRail(start,end,data.railways,mode);
  };
  const pending=queue.then(run);
  queue=pending.catch(()=>{});
  return pending;
}
