/** Follow connected OSM rail geometry locally, without inventing missing links. */
import { matchingOverpass } from "./overpass.js";

const railKinds = {
  Mainline: ['rail', 'narrow_gauge', 'disused', 'abandoned'],
  Metro: ['subway', 'light_rail'],
  Tram: ['tram', 'light_rail'],
  Monorail: ['monorail', 'funicular'],
};
export const supportsLocalRail = mode => Object.hasOwn(railKinds, mode);
const distance = (a,b) => Math.hypot((a[0]-b[0])*111320, (a[1]-b[1])*111320*Math.cos((a[0]+b[0])*Math.PI/360));

export function matchRail(start, end, railways, mode) {
  if (!supportsLocalRail(mode)) throw new Error('This mode needs the matching service');
  const vertices=[], graph=[], keys=new Map(), edges=[];
  function vertex(p) {
    const key=p.join(',');
    if(!keys.has(key)){keys.set(key,vertices.length);vertices.push(p);graph.push([]);}
    return keys.get(key);
  }
  function connect(a,b){const cost=distance(vertices[a],vertices[b]);graph[a].push([b,cost]);graph[b].push([a,cost]);}
  for(const rail of railways) {
    if(!railKinds[mode].includes(rail.kind))continue;
    for(let i=1;i<rail.coordinates.length;i++) {
      const a=vertex(rail.coordinates[i-1]), b=vertex(rail.coordinates[i]);
      if(a===b)continue;
      connect(a,b);edges.push([a,b]);
    }
  }
  function nearest(p) {
    let best=null;
    const cos=Math.cos(p[0]*Math.PI/180);
    for(let i=0;i<edges.length;i++){
      const [a,b]=edges[i],from=vertices[a],to=vertices[b];
      const dx=(to[1]-from[1])*cos,dy=to[0]-from[0];
      const t=Math.max(0,Math.min(1,((p[1]-from[1])*cos*dx+(p[0]-from[0])*dy)/(dx*dx+dy*dy)));
      const point=[from[0]+t*(to[0]-from[0]),from[1]+t*(to[1]-from[1])];
      const gap=distance(p,point);
      if(!best||gap<best.gap)best={edge:i,point,gap};
    }
    if(!best || best.gap>500)throw new Error('Place each endpoint within 500 m of a mapped rail alignment');
    return best;
  }
  const from=nearest(start),to=nearest(end);
  const first=vertex(from.point),last=vertex(to.point);
  for(const [projection,v] of [[from,first],[to,last]])for(const end of edges[projection.edge])connect(v,end);
  if(from.edge===to.edge)connect(first,last);
  // Binary heap keeps long rail networks from turning into quadratic searches.
  const heap=[];
  function push(item){let i=heap.length;heap.push(item);while(i>0){const p=(i-1)>>1;if(heap[p][0]<=item[0])break;heap[i]=heap[p];i=p;}heap[i]=item;}
  function pop(){const value=heap[0],tail=heap.pop();if(heap.length){let i=0;while(i*2+1<heap.length){let c=i*2+1;if(c+1<heap.length&&heap[c+1][0]<heap[c][0])c++;if(heap[c][0]>=tail[0])break;heap[i]=heap[c];i=c;}heap[i]=tail;}return value;}
  const costs=new Map([[first,0]]),previous=new Map();
  push([0,first]);
  while(heap.length){const [cost,v]=pop();if(cost!==costs.get(v))continue;if(v===last)break;
    for(const [next,weight] of graph[v]){const total=cost+weight;if(total<(costs.get(next)??Infinity)){costs.set(next,total);previous.set(next,v);push([total,next]);}}
  }
  if(!costs.has(last))throw new Error('No connected rail alignment found; add closer control points along the same track');
  const path=[];
  for(let v=last;;v=previous.get(v)){path.push(vertices[v]);if(v===first)break;}
  path.reverse();
  return {coordinates:[start,...path,end], source:'OpenStreetMap rail alignment'};
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
