/** Join calendar-expanded runs to their actual OSM GTFS shape and stop distances. */
import {readFile,writeFile,mkdir,copyFile,cp} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
import path from 'node:path';
import {readGtfsZip} from '../web/fantasy-map/static/modules/gtfs-zip.js';
import {csvRows,buildGtfsNetwork} from '../web/fantasy-map/static/modules/gtfs-model.js';
const rows=text=>{const all=[...csvRows(text)],head=all.shift();return all.map(r=>Object.fromEntries(head.map((k,i)=>[k,r[i]])));};
export function buildMap(files,schedule){
  if(schedule.version!==1||!Array.isArray(schedule.runs)||!schedule.runs.length||schedule.runs.length>100000)throw Error('Invalid or empty live-map schedule');
  const network=buildGtfsNetwork(files);
  if(network.fallback||network.skipped)throw Error('Live map requires complete OSM shapes');
  const trips=new Map(rows(files['trips.txt']).map(t=>[t.trip_id,t]));
  const routes=Object.fromEntries(rows(files['routes.txt']).map(r=>[r.route_id,{name:r.route_short_name||r.route_long_name||r.route_id,color:/^[0-9a-f]{6}$/i.test(r.route_color||'')?'#'+r.route_color:'#999999'}]));
  const shapes={},calls=new Map();
  for(const row of rows(files['shapes.txt'])){const p=[Number(row.shape_pt_lat),Number(row.shape_pt_lon),Number(row.shape_dist_traveled),Number(row.shape_pt_sequence)];if(!row.shape_dist_traveled?.trim()||!p.every(Number.isFinite))throw Error('Missing shape distance');(shapes[row.shape_id]??=[]).push(p);}
  for(const shape of Object.values(shapes)){shape.sort((a,b)=>a[3]-b[3]);shape.forEach((p,i)=>{if(i&&p[2]<shape[i-1][2])throw Error('Nonmonotonic shape');p.pop();});}
  for(const row of rows(files['stop_times.txt'])){if(!calls.has(row.trip_id))calls.set(row.trip_id,[]);calls.get(row.trip_id).push(row);}
  for(const list of calls.values())list.sort((a,b)=>Number(a.stop_sequence)-Number(b.stop_sequence));
  const patterns={},runs=[];
  for(const run of schedule.runs){
    const trip=trips.get(run.trip),shape=shapes[trip?.shape_id],stopCalls=calls.get(run.trip);
    if(!shape||!stopCalls||!routes[run.route]||trip.route_id!==run.route||run.calls.length!==stopCalls.length)throw Error('Scheduled trip lacks matching OSM shape: '+run.trip);
    const distances=stopCalls.map((s,i)=>{const d=Number(s.shape_dist_traveled);if(s.stop_id!==run.calls[i][0]||!s.shape_dist_traveled?.trim()||!Number.isFinite(d)||d<shape[0][2]||d>shape.at(-1)[2])throw Error('Stop/shape mismatch: '+run.trip);return d;});
    if(distances.some((d,i)=>i&&d<distances[i-1]))throw Error('Nonmonotonic stop distance');
    const key=trip.shape_id+':'+JSON.stringify(stopCalls.map(s=>s.stop_id));
    if(!patterns[key])patterns[key]={shape:trip.shape_id,stops:stopCalls.map(s=>s.stop_id),distances};
    const times=run.calls.flatMap((c,i)=>{if(!Number.isFinite(c[1])||!Number.isFinite(c[2])||c[1]>c[2]||(i&&c[1]<run.calls[i-1][2]))throw Error('Invalid run times');return [c[1],c[2]];});
    runs.push({id:run.id,trip:run.trip,route:run.route,day:run.day,base:run.base,headsign:run.headsign||routes[run.route].name,computed:!!run.computed,frequency:!!run.frequency,pattern:key,times});
  }
  const patternList=Object.values(patterns),indexes=new Map(Object.keys(patterns).map((key,i)=>[key,i]));runs.forEach(r=>{r.pattern=indexes.get(r.pattern);});
  const stationRows=rows(files['stops.txt']),byId=new Map(stationRows.map(s=>[s.stop_id,s])),stations=new Map();
  for(const p of patternList)for(const id of p.stops){const stop=byId.get(id),s=byId.get(stop?.parent_station)||stop;if(!s)throw Error('Missing station');const lat=Number(s.stop_lat),lon=Number(s.stop_lon);if(!Number.isFinite(lat)||!Number.isFinite(lon))throw Error('Invalid station');if(!stations.has(s.stop_id))stations.set(s.stop_id,{id:s.stop_id,name:s.stop_name,codes:new Set(s.stop_code?[s.stop_code]:[]),point:[lat,lon]});if(stop.stop_code)stations.get(s.stop_id).codes.add(stop.stop_code);}
  return {...schedule,runs,patterns:patternList,shapes,routes,stations:[...stations.values()].map(s=>({...s,codes:[...s.codes].sort(),code:[...s.codes].sort().join(' / ')||s.id})),lines:network.lines.map(l=>({name:l.name,color:l.color,points:l.segments[0].guide})),duplicates:network.duplicates};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
  const bytes=await readFile(process.argv[2]),files=await readGtfsZip(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength));
  const data=buildMap(files,JSON.parse(await readFile(process.argv[3],'utf8'))),out=process.argv[4];
  await mkdir(out,{recursive:false});await cp(new URL('../web/live-map/',import.meta.url),out,{recursive:true});
  await mkdir(path.join(out,'vendor'));for(const name of ['leaflet.js','leaflet.css'])await copyFile(new URL('../web/fantasy-map/static/vendor/'+name,import.meta.url),path.join(out,'vendor',name));
  const body=JSON.stringify(data);if(Buffer.byteLength(body)>32*1024*1024)throw Error('Map data exceeds 32 MiB');
  await writeFile(path.join(out,'network.json'),body);await writeFile(path.join(out,'.nojekyll'),'');
  console.log(`Live map: ${data.runs.length} scheduled runs, ${data.stations.length} stations, ${data.lines.length} ribbons, ${Object.keys(data.shapes).length} track shapes`);
}
