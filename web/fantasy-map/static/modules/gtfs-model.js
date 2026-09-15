/** GTFS Schedule rail routes -> editable lines, without importing departures. */
export function* csvRows(text) {
  let row=[],field='',quoted=false,closed=false,rows=0;
  text=text.replace(/^\uFEFF/,'');
  for(let i=0;i<=text.length;i++){
    const c=i===text.length?'\n':text[i];
    if(quoted){if(c==='"'){if(text[i+1]==='"'){field+='"';i++;}else{quoted=false;closed=true;}}else{if(i===text.length)throw new Error('Unclosed CSV quote');field+=c;}continue;}
    if(c==='"'&&!field&&!closed){quoted=true;continue;}
    if(c===','||c==='\n'||c==='\r'){
      row.push(field);field='';closed=false;
      if(c!==','){if(c==='\r'&&text[i+1]==='\n')i++;if(row.some(x=>x!=='')){if(++rows>1000000)throw new Error('GTFS table exceeds one million rows');yield row;}row=[];}
    }else{if(closed||c==='"')throw new Error('Malformed CSV field');field+=c;}
  }
}
function* table(files,name,required) {
  if(!files[name])throw new Error('Missing or empty '+name);
  const rows=csvRows(files[name]),head=rows.next().value;
  if(!head||new Set(head).size!==head.length||required.some(k=>!head.includes(k)))throw new Error('Missing or duplicate columns in '+name);
  for(const values of rows){if(values.length!==head.length)throw new Error('Wrong column count in '+name);yield Object.fromEntries(head.map((k,i)=>[k,values[i]]));}
}
const number=value=>typeof value==='string'&&value.trim()!==''&&Number.isFinite(Number(value))?Number(value):null;
function coordinate(lat,lon){const a=number(lat),b=number(lon);return a!==null&&b!==null&&Math.abs(a)<=90&&Math.abs(b)<=180?[a,b]:null;}
export function railMode(type) {
  const n=number(type);
  if(n===0||n===5||(n>=900&&n<=906))return 'Tram';
  if(n===12||n===405||n===7||n===1400)return 'Monorail';
  if(n===1||(n>=400&&n<=404))return 'Metro';
  if(n===2||(n>=100&&n<=117))return 'Mainline';
  return null;
}
const distance=(a,b)=>Math.hypot((a[0]-b[0])*111320,(a[1]-b[1])*111320*Math.cos((a[0]+b[0])*Math.PI/360));
function stationFraction(point,path,cumulative,minT){
  let best=null;const total=cumulative.at(-1),cos=Math.cos(point[0]*Math.PI/180);
  for(let i=1;i<path.length;i++){
    const a=path[i-1],b=path[i],dx=(b[1]-a[1])*cos,dy=b[0]-a[0],den=dx*dx+dy*dy;
    if(!den)continue;
    const f=Math.max(0,Math.min(1,((point[1]-a[1])*cos*dx+(point[0]-a[0])*dy)/den));
    const t=(cumulative[i-1]+f*(cumulative[i]-cumulative[i-1]))/total;
    if(t+1e-8<minT)continue;
    const projected=[a[0]+f*(b[0]-a[0]),a[1]+f*(b[1]-a[1])],gap=distance(point,projected);
    if(!best||gap<best.gap)best={t,gap};
  }
  return best&&best.gap<=1000?best.t:null;
}
// Collapse directional and short-turn copies within a route, preserving branches.
// Parent stations unify platform IDs. Closed loops allow exact cyclic rotation
// and reversal, while repeated-station patterns are never removed by containment.
function uniquePatterns(patterns, stops, shapes) {
  const physical = trip => trip.stops.map(call => stops.get(call.id)?.parent_station || call.id);
  const candidates = [...patterns.values()].map(trip => ({trip, ids: physical(trip)}));
  candidates.sort((a,b) => b.ids.length-a.ids.length
    || Number(shapes.has(b.trip.shape_id))-Number(shapes.has(a.trip.shape_id))
    || String(a.trip.trip_id).localeCompare(String(b.trip.trip_id)));
  const retained=[];
  for(const candidate of candidates){
    const duplicate=retained.some(other=>{
      if(candidate.trip.route_id!==other.trip.route_id)return false;
      const a=candidate.ids,b=other.ids;
      if(a.length===b.length&&a.length>2&&a[0]===a.at(-1)&&b[0]===b.at(-1)){
        const loop=a.slice(0,-1),otherLoop=b.slice(0,-1);
        if([loop,[...loop].reverse()].some(ids=>otherLoop.some((_,start)=>ids.every((id,i)=>id===otherLoop[(start+i)%otherLoop.length]))))return true;
      }
      const repeated=ids=>new Set(ids).size!==ids.length;
      if(a.length!==b.length&&(repeated(a)||repeated(b)))return false;
      return [a,[...a].reverse()].some(ids=>{
        for(let start=0;start<=b.length-ids.length;start++)if(ids.every((id,i)=>id===b[start+i]))return true;
        return false;
      });
    });
    if(!duplicate)retained.push(candidate);
  }
  return retained.map(x=>x.trip);
}

export function buildGtfsNetwork(files) {
  const routes=new Map(),trips=new Map(),stops=new Map(),shapes=new Map();
  for(const row of table(files,'routes.txt',['route_id','route_type'])){const mode=railMode(row.route_type);if(mode)routes.set(row.route_id,{...row,mode});}
  if(!routes.size)throw new Error('This feed has no supported rail routes');
  for(const row of table(files,'trips.txt',['route_id','trip_id']))if(routes.has(row.route_id)){if(trips.has(row.trip_id))throw new Error('Duplicate GTFS trip_id');trips.set(row.trip_id,{...row,stops:[]});}
  for(const row of table(files,'stops.txt',['stop_id','stop_name','stop_lat','stop_lon']))stops.set(row.stop_id,{...row,coordinate:coordinate(row.stop_lat,row.stop_lon)});
  for(const row of table(files,'stop_times.txt',['trip_id','stop_id','stop_sequence'])){
    const trip=trips.get(row.trip_id);if(!trip)continue;
    const sequence=number(row.stop_sequence);if(sequence===null||!Number.isInteger(sequence)||sequence<0)throw new Error('Invalid stop_sequence');
    trip.stops.push({id:row.stop_id,sequence});
  }
  const patterns=new Map();
  for(const trip of trips.values()){
    trip.stops.sort((a,b)=>a.sequence-b.sequence);
    const key=JSON.stringify([trip.route_id,trip.shape_id||'',trip.stops.map(s=>s.id)]);
    if(!patterns.has(key))patterns.set(key,trip);
  }
  if(patterns.size>2500)throw new Error('Feed has more than 2,500 raw rail patterns; use a smaller regional feed');
  const neededShapes=new Set([...patterns.values()].map(x=>x.shape_id).filter(Boolean));
  if(files['shapes.txt'])for(const row of table(files,'shapes.txt',['shape_id','shape_pt_lat','shape_pt_lon','shape_pt_sequence'])){
    if(!neededShapes.has(row.shape_id))continue;
    const point=coordinate(row.shape_pt_lat,row.shape_pt_lon),sequence=number(row.shape_pt_sequence);
    if(!point||sequence===null||!Number.isInteger(sequence)||sequence<0)throw new Error('Invalid GTFS shape coordinate or sequence');
    if(!shapes.has(row.shape_id))shapes.set(row.shape_id,[]);
    shapes.get(row.shape_id).push({point,sequence});
  }
  const selected=uniquePatterns(patterns,stops,shapes);
  if(selected.length>250)throw new Error('Feed has more than 250 distinct rail patterns; use a smaller regional feed');
  const lines=[];let fallback=0,skipped=0,pointCount=0,stationCount=0;
  for(const trip of selected){
    const route=routes.get(trip.route_id);
    const calls=trip.stops.map(call=>{const stop=stops.get(call.id);const point=stop?.coordinate||stops.get(stop?.parent_station)?.coordinate;return point?{...stop,coordinate:point}:null;});
    if(calls.some(x=>!x)||calls.length<2){skipped++;continue;}
    const shape=shapes.get(trip.shape_id);
    let path=shape?[...shape].sort((a,b)=>a.sequence-b.sequence).map(x=>x.point):calls.map(x=>x.coordinate);
    path=path.filter((p,i)=>i===0||p[0]!==path[i-1][0]||p[1]!==path[i-1][1]);
    if(path.length<2){skipped++;continue;}
    if(path.length>10000||(pointCount+=path.length)>100000)throw new Error('GTFS rail geometry is too large; use a smaller feed');
    if((stationCount+=calls.length)>10000)throw new Error('GTFS has more than 10,000 rail stops across patterns');
    if(!shape)fallback++;
    const cumulative=[0];for(let i=1;i<path.length;i++)cumulative.push(cumulative.at(-1)+distance(path[i-1],path[i]));
    const stations=[];let lastT=0;
    for(const stop of calls){const t=stationFraction(stop.coordinate,path,cumulative,lastT);if(t===null)throw new Error('Stops do not follow the shape for route '+trip.route_id);stations.push({name:stop.stop_name,code:stop.stop_code||undefined,t});lastT=t;}
    const suffix=trip.trip_headsign?' → '+trip.trip_headsign:'';
    lines.push({name:(route.route_short_name||route.route_long_name||route.route_id)+suffix,mode:route.mode,
      color:/^[0-9a-f]{6}$/i.test(route.route_color||'')?'#'+route.route_color:undefined,
      nodes:[path[0],path.at(-1)],segments:[{profile:'rail',guide:path}],stations});
  }
  if(!lines.length)throw new Error('No rail patterns with usable stops and geometry were found');
  return {lines,fallback,skipped,duplicates:patterns.size-selected.length};
}
