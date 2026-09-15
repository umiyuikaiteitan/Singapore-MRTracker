/** Cached Overpass road geometry for line design, not traffic navigation. */
import { roadOverpass } from './overpass.js';
import { matchAlignment } from './alignment-graph.js';
export const supportsLocalRoad = mode => ['Mainline','Metro','Tram','BRT','Monorail','Cycleway'].includes(mode);
export function roadAllowed(road, mode) {
  const tags=road.tags||{};
  if(['no','private'].includes(tags.access))return false;
  if(mode==='Cycleway')return !/^(motorway|trunk)(_link)?$/.test(road.kind)&&tags.bicycle!=='no';
  return /^(motorway|trunk|primary|secondary|tertiary|unclassified|residential|living_street|service|busway)(_link)?$/.test(road.kind);
}
export function matchRoad(start,end,roads,mode){
  if(!supportsLocalRoad(mode))throw new Error('Road following is unavailable for this mode');
  const result=matchAlignment(start,end,roads,{accept:road=>roadAllowed(road,mode),label:'road',nodeIdentity:true});
  return {...result,guideCoordinates:result.coordinates};
}
// Serialize edits so one segment's fetch cannot cancel another's. The client's
// bounded 5-minute cache reuses containing queries across subsequent segments.
export function createRoadMatcher(client=roadOverpass){
  let queue=Promise.resolve();
  return (start,end,mode)=>{
    const run=async()=>{
      const pad=0.01;
      const bounds=[Math.max(-90,Math.min(start[0],end[0])-pad),Math.max(-180,Math.min(start[1],end[1])-pad),Math.min(90,Math.max(start[0],end[0])+pad),Math.min(180,Math.max(start[1],end[1])+pad)];
      const data=await client.get(bounds,13,'roads');
      return matchRoad(start,end,data.roads,mode);
    };
    const pending=queue.then(run);queue=pending.catch(()=>{});return pending;
  };
}
export const requestRoadMatch=createRoadMatcher();
