/** Pure schedule playback. Positions are estimates, never vehicle observations. */
export function freshness(live,now){
  const report=Number.isFinite(live?.generated)&&live.generated<=now+30&&now-live.generated<=120&&live.live===true;
  const stamp=live?.trip_updates_timestamp;
  const predictions=report&&Number.isFinite(stamp)&&stamp<=now+30&&now-stamp<=120;
  return {report,predictions,age:report?Math.max(0,now-live.generated):null};
}
export function preferReport(current,candidate,now){
  if(!candidate)return current;
  if(!current)return candidate;
  const a=freshness(current,now).predictions,b=freshness(candidate,now).predictions;
  return (b&&!a)||(a===b&&candidate.generated>current.generated)?candidate:current;
}
export function pointAt(shape,distance){
  let low=1,high=shape.length-1;
  if(distance<=shape[0][2])return shape[0].slice(0,2);
  if(distance>=shape[high][2])return shape[high].slice(0,2);
  while(low<high){const mid=(low+high)>>1;if(shape[mid][2]<distance)low=mid+1;else high=mid;}
  const a=shape[low-1],b=shape[low],span=b[2]-a[2],f=span>0?(distance-a[2])/span:0;
  return [a[0]+f*(b[0]-a[0]),a[1]+f*(b[1]-a[1])];
}
export function positionRun(run,pattern,shape,now,live){
  const original=run.times,relative=now-run.base;
  if(relative<original[0]-7200||relative>original.at(-1)+7200)return null;
  let update=freshness(live,now).predictions?live.trips?.[run.trip]:null;
  // Date/start identifiers prevent another day's/frequency instance's update
  // from moving this run. Older report schemas still support service notices.
  if(update&&(!update.sd||update.sd!==run.day))update=null;
  const start=original[1];
  if(update&&run.frequency&&!update.st)update=null;
  if(update?.st){const parts=update.st.split(':').map(Number);if(parts.length!==3||parts[0]*3600+parts[1]*60+parts[2]!==start)update=null;}
  if(update?.ts!==undefined&&(!Number.isFinite(update.ts)||now-update.ts>120||update.ts>now+30))update=null;
  if(update?.c===1)return null;
  const validDelay=n=>Number.isFinite(n)&&Math.abs(n)<=7200;
  const counts=new Map();pattern.stops.forEach(s=>counts.set(s,(counts.get(s)||0)+1));
  let calls=pattern.stops.map((stop,i)=>{
    const perStop=counts.get(stop)===1?update?.s?.[stop]:undefined;
    const d=validDelay(perStop)?perStop:validDelay(update?.d)?update.d:0;
    const adjusted=!!update&&(validDelay(perStop)||validDelay(update.d));
    return {arrival:original[i*2]+d,departure:original[i*2+1]+d,distance:pattern.distances[i],skip:perStop==='skip',adjusted,delay:d,index:i};
  });
  // Contradictory predictions cannot reverse a train or its clock.
  if(calls.some((c,i)=>c.arrival>c.departure||(i&&c.arrival<calls[i-1].departure))){calls=pattern.stops.map((_,i)=>({arrival:original[i*2],departure:original[i*2+1],distance:pattern.distances[i],adjusted:false,delay:0,index:i}));}
  calls=calls.filter(c=>!c.skip);
  if(calls.length<2||relative<calls[0].arrival||relative>calls.at(-1).departure)return null;
  for(let i=0;i<calls.length;i++){
    const call=calls[i];
    if(relative>=call.arrival&&relative<=call.departure)return {point:pointAt(shape,call.distance),adjusted:call.adjusted,delay:call.delay,dwell:true};
    const next=calls[i+1];
    if(next&&relative>call.departure&&relative<next.arrival){const f=(relative-call.departure)/(next.arrival-call.departure);return {point:pointAt(shape,call.distance+f*(next.distance-call.distance)),adjusted:call.adjusted||next.adjusted||next.index-call.index>1,delay:call.delay+f*(next.delay-call.delay),dwell:false};}
  }
  return null;
}
