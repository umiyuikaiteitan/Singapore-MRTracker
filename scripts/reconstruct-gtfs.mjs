/** Build missing GTFS shapes from active OSM rail topology, preserving supplied shapes. */
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { csvRows, railMode, buildGtfsNetwork } from '../web/fantasy-map/static/modules/gtfs-model.js';
import { readGtfsZip } from '../web/fantasy-map/static/modules/gtfs-zip.js';

const distance=(a,b)=>Math.hypot((a[0]-b[0])*111320,(a[1]-b[1])*111320*Math.cos((a[0]+b[0])*Math.PI/360));
const point=p=>p&&Number.isFinite(p.lat)&&Number.isFinite(p.lon)&&Math.abs(p.lat)<=90&&Math.abs(p.lon)<=180?[p.lat,p.lon]:null;
const normalized=s=>String(s||'').toUpperCase().replace(/[^A-Z0-9]/g,'');
const aliases=[['NS','NSL','North South Line'],['EW','EWL','East West Line'],['NE','NEL','North East Line'],['CC','CCL','Circle Line'],['DT','DTL','Downtown Line'],['TE','TEL','Thomson East Coast Line'],['BP','BPL','BPLRT','Bukit Panjang LRT'],['SK','SKL','SKLRT','Sengkang LRT'],['PG','PGL','PGLRT','Punggol LRT']];
const canonical=s=>{const n=normalized(s);return aliases.find(group=>group.some(a=>normalized(a)===n))?.[0]||n;};
function rows(text){const all=[...csvRows(text||'')],head=all.shift();if(!head)return [];return all.map(row=>Object.fromEntries(head.map((k,i)=>[k,row[i]||''])));}
const csv=(head,data)=>[head,...data.map(row=>head.map(k=>row[k]??''))].map(row=>row.map(v=>'"'+String(v).replaceAll('"','""')+'"').join(',')).join('\n')+'\n';

export function railData(osm){
  if(!Array.isArray(osm?.elements)||osm.remark||osm.elements.length>30000)throw new Error('Incomplete or oversized Overpass rail response');
  let count=0;
  const ways=osm.elements.filter(e=>e.type==='way'&&/^(rail|subway|light_rail|monorail|tram)$/.test(e.tags?.railway||'')&&!e.tags?.service&&!['yes','construction'].includes(e.tags?.construction)&&e.tags?.disused!=='yes'&&e.tags?.abandoned!=='yes').map(e=>{
    if(!Array.isArray(e.nodes)||!Array.isArray(e.geometry)||e.nodes.length!==e.geometry.length)throw new Error('OSM rail node references are missing');
    count+=e.nodes.length;if(count>250000)throw new Error('OSM rail geometry is too large');
    const coordinates=e.geometry.map(point);if(coordinates.some(p=>!p)||e.nodes.some(n=>!Number.isSafeInteger(n)))throw new Error('Incomplete OSM rail geometry');
    return {...e,coordinates};
  });
  if(!ways.length)throw new Error('No active OSM rail ways found');
  return {ways,relations:osm.elements.filter(e=>e.type==='relation'&&e.tags?.type==='route'&&/^(subway|light_rail|monorail|train|tram)$/.test(e.tags?.route||''))};
}
function routeWays(data,route){
  const names=new Set([route.route_id,route.route_short_name,route.route_long_name].filter(Boolean).map(canonical));
  const relations=data.relations.filter(r=>[r.tags.ref,r.tags.name,r.tags['name:en']].some(v=>v&&names.has(canonical(v))));
  const ids=new Set(relations.flatMap(r=>(r.members||[]).filter(m=>m.type==='way').map(m=>m.ref)));
  return ids.size?data.ways.filter(w=>ids.has(w.id)):data.ways;
}
// Node IDs, rather than coordinate equality, preserve disconnected parallel
// tracks and grade-separated crossings. Project every call onto one component.
export function shapeThroughStops(ways,stops){
  const vertices=[],adj=[],ids=new Map(),edges=[];
  const vertex=(p,key)=>{if(!ids.has(key)){ids.set(key,vertices.length);vertices.push(p);adj.push([]);}return ids.get(key);};
  const connect=(a,b)=>{const d=distance(vertices[a],vertices[b]);if(d>0){adj[a].push([b,d]);adj[b].push([a,d]);}};
  for(const way of ways)for(let i=1;i<way.coordinates.length;i++){
    const a=vertex(way.coordinates[i-1],way.nodes[i-1]),b=vertex(way.coordinates[i],way.nodes[i]);
    if(a!==b&&distance(vertices[a],vertices[b])>0){connect(a,b);edges.push([a,b]);}
  }
  const components=Array(vertices.length).fill(-1);
  for(let root=0;root<vertices.length;root++)if(components[root]===-1){const stack=[root];components[root]=root;while(stack.length){const v=stack.pop();for(const [n] of adj[v])if(components[n]===-1){components[n]=root;stack.push(n);}}}
  // Keep nearby alternatives: the nearest track at consecutive platforms can
  // switch sides even though its only crossover is kilometres away.
  const candidates=stops.map(p=>{
    const found=[],cos=Math.cos(p[0]*Math.PI/180);
    edges.forEach(([a,b],edge)=>{const x=vertices[a],y=vertices[b],dx=(y[1]-x[1])*cos,dy=y[0]-x[0];
      const t=Math.max(0,Math.min(1,((p[1]-x[1])*cos*dx+(p[0]-x[0])*dy)/(dx*dx+dy*dy)));
      const coordinate=[x[0]+t*(y[0]-x[0]),x[1]+t*(y[1]-x[1])],gap=distance(p,coordinate);
      if(gap<=500)found.push({edge,t,coordinate,gap,group:components[a]});
    });
    // Bound search while retaining alternatives in separate components.
    found.sort((a,b)=>a.gap-b.gap||a.edge-b.edge);
    const perGroup=new Map();return found.filter(p=>{const n=perGroup.get(p.group)||0;perGroup.set(p.group,n+1);return n<16;}).slice(0,64);
  });
  const viable=new Set(candidates[0].map(c=>c.group));
  for(const group of viable)if(candidates.some(list=>!list.some(p=>p.group===group)))viable.delete(group);
  if(!viable.size){
    const error=new Error('No connected active rail component within 500 m of every stop');
    error.legs=[];for(let i=1;i<candidates.length;i++)if(!candidates[i-1].some(a=>candidates[i].some(b=>a.group===b.group)))error.legs.push(i-1);
    error.scope=error.legs.length?'segments':'whole-pattern-component-mismatch';throw error;
  }
  const onEdge=new Map();
  const calls=candidates.map((list,i)=>list.filter(p=>viable.has(p.group)).map((p,j)=>{
    const v=vertex(p.coordinate,`call:${i}:${j}`);if(!onEdge.has(p.edge))onEdge.set(p.edge,[]);onEdge.get(p.edge).push([p.t,v]);return {...p,v};
  }));
  for(const [edge,list] of onEdge){const [a,b]=edges[edge];list.push([0,a],[1,b]);list.sort((x,y)=>x[0]-y[0]);for(let i=1;i<list.length;i++){const x=list[i-1][1],y=list[i][1],d=distance(vertices[x],vertices[y]);adj[x].push([y,d]);adj[y].push([x,d]);}}
  let states=calls[0].map(p=>({v:p.v,cost:4*p.gap,previous:null,leg:[vertices[p.v]]}));
  for(let i=1;i<calls.length;i++){
    const next=new Map(),limit=Math.max(3000,distance(stops[i-1],stops[i])*5);
    // A separate bounded search per predecessor preserves feasible alternatives
    // when the cheapest aggregate path would exceed this leg's detour limit.
    for(const origin of states){
      const costs=new Map([[origin.v,0]]),previous=new Map(),heap=[];
      const push=item=>{let at=heap.length;heap.push(item);while(at>0){const p=(at-1)>>1;if(heap[p][0]<=item[0])break;heap[at]=heap[p];at=p;}heap[at]=item;};
      const pop=()=>{const first=heap[0],last=heap.pop();if(heap.length){let at=0;while(at*2+1<heap.length){let c=at*2+1;if(c+1<heap.length&&heap[c+1][0]<heap[c][0])c++;if(heap[c][0]>=last[0])break;heap[at]=heap[c];at=c;}heap[at]=last;}return first;};
      push([0,origin.v]);const targets=new Set(calls[i].map(p=>p.v));
      while(heap.length&&targets.size){const [cost,v]=pop();if(cost!==costs.get(v))continue;targets.delete(v);
        for(const [n,w] of adj[v]){const total=cost+w;if(total<=limit&&total<(costs.get(n)??Infinity)){costs.set(n,total);previous.set(n,v);push([total,n]);}}
      }
      for(const p of calls[i])if(costs.has(p.v)){
        const cost=origin.cost+costs.get(p.v)+4*p.gap;
        if(next.has(p.v)&&next.get(p.v).cost<=cost)continue;
        const leg=[];for(let v=p.v;;v=previous.get(v)){leg.push(vertices[v]);if(v===origin.v)break;}leg.reverse();
        next.set(p.v,{v:p.v,cost,previous:origin,leg});
      }
    }
    states=[...next.values()];
    if(!states.length){const error=new Error('No connected OSM track match without an excessive detour');error.legs=[i-1];error.scope='segments';throw error;}
  }
  const final=states.reduce((best,s)=>!best||s.cost<best.cost?s:best,null),legs=[];
  for(let state=final;state;state=state.previous)legs.push(state.leg);legs.reverse();
  const path=[],callDistances=[];let traveled=0;
  for(const leg of legs){
    for(const p of leg)if(!path.length||distance(path.at(-1),p)>0.01){if(path.length)traveled+=distance(path.at(-1),p);path.push(p);}
    callDistances.push(traveled);
  }
  if(path.length<2||path.length>10000)throw new Error('OSM shape is empty or exceeds importer limits');
  Object.defineProperty(path,'callDistances',{value:callDistances});
  return path;
}

export function reconstruct(files,osm){
  buildGtfsNetwork(files); // Validate original tables and limits before adding geometry.
  const data=railData(osm),routes=new Map(rows(files['routes.txt']).filter(r=>railMode(r.route_type)).map(r=>[r.route_id,r]));
  const trips=rows(files['trips.txt']),stops=new Map(rows(files['stops.txt']).map(s=>[s.stop_id,s]));
  const existing=new Set(rows(files['shapes.txt']).map(s=>s.shape_id)),calls=new Map(),stopTimes=rows(files['stop_times.txt']);
  for(const row of stopTimes){if(!calls.has(row.trip_id))calls.set(row.trip_id,[]);calls.get(row.trip_id).push(row);}
  const patterns=new Map();
  for(const trip of trips){if(!routes.has(trip.route_id)||existing.has(trip.shape_id))continue;
    const ordered=(calls.get(trip.trip_id)||[]).sort((a,b)=>Number(a.stop_sequence)-Number(b.stop_sequence));
    const key=JSON.stringify([trip.route_id,ordered.map(s=>s.stop_id)]);
    if(!patterns.has(key))patterns.set(key,{route:routes.get(trip.route_id),calls:ordered,trips:[]});patterns.get(key).trips.push(trip);
  }
  const generated=[],report={source:'© OpenStreetMap contributors',license:'ODbL-1.0',osmTimestamp:osm.osm3s?.timestamp_osm_base||null,reconstructed:0,preserved:trips.filter(t=>routes.has(t.route_id)&&existing.has(t.shape_id)).length,unmatched:[]};
  let index=0;
  for(const pattern of patterns.values())try{
    const coordinates=pattern.calls.map(call=>{let s=stops.get(call.stop_id);if(s&&(!s.stop_lat||!s.stop_lon))s=stops.get(s.parent_station);if(!s||!s.stop_lat?.trim()||!s.stop_lon?.trim())throw new Error('Missing stop coordinates');const p=point({lat:Number(s.stop_lat),lon:Number(s.stop_lon)});if(!p)throw new Error('Invalid stop coordinates');return p;});
    if(coordinates.length<2)throw new Error('Pattern has fewer than two stops');
    const path=shapeThroughStops(routeWays(data,pattern.route),coordinates);
    let shapeId;do{shapeId='osm-reconstructed-'+(++index);}while(existing.has(shapeId));existing.add(shapeId);
    let traveled=0;path.forEach((p,i)=>{if(i)traveled+=distance(path[i-1],p);generated.push({shape_id:shapeId,shape_pt_lat:p[0],shape_pt_lon:p[1],shape_pt_sequence:i,shape_dist_traveled:traveled});});
    pattern.trips.forEach(t=>{t.shape_id=shapeId;const ordered=calls.get(t.trip_id).sort((a,b)=>Number(a.stop_sequence)-Number(b.stop_sequence));ordered.forEach((call,i)=>{call.shape_dist_traveled=path.callDistances[i];});});report.reconstructed++;
  }catch(error){report.unmatched.push({route:pattern.route.route_short_name||pattern.route.route_id,stops:pattern.calls.map(s=>s.stop_id),reason:error.message,scope:error.scope||'pattern',segments:(error.legs||[]).map(i=>({from:pattern.calls[i].stop_id,to:pattern.calls[i+1].stop_id}))});}
  const shapeHead=['shape_id','shape_pt_lat','shape_pt_lon','shape_pt_sequence','shape_dist_traveled'];
  // Preserve original columns (including distance) when supplied shapes exist.
  const oldShapes=rows(files['shapes.txt']),head=oldShapes.length?[...new Set([...Object.keys(oldShapes[0]),'shape_dist_traveled'])]:shapeHead;
  const result={...files,'trips.txt':csv([...new Set([...Object.keys(trips[0]||{}),'shape_id'])],trips),'shapes.txt':csv(head,[...oldShapes,...generated]),'stop_times.txt':csv([...new Set([...Object.keys(stopTimes[0]||{}),'shape_dist_traveled'])],stopTimes)};
  const validated=buildGtfsNetwork(result);report.fallback=validated.fallback;report.patterns=validated.lines.length;
  return {files:result,report};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
  const bytes=await readFile(process.argv[2]);const files=await readGtfsZip(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength));
  const result=reconstruct(files,JSON.parse(await readFile(process.argv[3],'utf8')));
  if(!result.report.reconstructed&&!result.report.preserved)throw new Error('No Singapore rail shapes could be reconstructed');
  await writeFile(process.argv[4],JSON.stringify(result));
  console.log(JSON.stringify(result.report));
}
