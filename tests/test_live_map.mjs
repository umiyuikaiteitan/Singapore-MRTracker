import test from 'node:test';
import assert from 'node:assert/strict';
import {freshness,positionRun} from '../web/live-map/engine.mjs';
import {buildMap} from '../scripts/build-live-map.mjs';
const shape=[[1,103,0],[1.01,103.01,100],[1,103,200],[1.02,103,300]];
const pattern={stops:['A','B','A','C'],distances:[0,100,200,300]};
const run={trip:'T',day:'20260915',base:1000,times:[0,10,110,120,220,230,330,340]};
const live=(trip={},now=1150)=>({live:true,generated:now,trip_updates_timestamp:now,trips:{T:{sd:'20260915',...trip}}});
test('advances through multiple stations and repeated track without freezing at first arrival',()=>{const a=positionRun(run,pattern,shape,1050,null),b=positionRun(run,pattern,shape,1270,null);assert.ok(a.point[1]>103);assert.equal(b.point[1],103);assert.ok(b.point[0]>1);assert.equal(positionRun(run,pattern,shape,1400,null),null);});
test('fresh matched predictions apply, cancellations remove run, wrong date/start ignored',()=>{assert.equal(positionRun(run,pattern,shape,1150,live({d:60})).adjusted,true);assert.equal(positionRun(run,pattern,shape,1150,live({c:1})),null);assert.equal(positionRun(run,pattern,shape,1150,live({sd:'20260914',c:1})).adjusted,false);assert.equal(positionRun(run,pattern,shape,1150,live({st:'00:01:00',c:1})).adjusted,false);});
test('stale or timestamp-less upstream updates fall back to schedule',()=>{assert.equal(freshness({...live(),trip_updates_timestamp:900},1150).predictions,false);assert.equal(positionRun(run,pattern,shape,1300,live({d:60})).adjusted,false);assert.equal(freshness({live:true,generated:1150},1150).predictions,false);});
test('repeated stop updates do not affect both visits; inconsistent times fall back',()=>{assert.equal(positionRun(run,pattern,shape,1150,live({s:{A:90}})).adjusted,false);assert.equal(positionRun(run,pattern,shape,1150,live({s:{B:-200}})).adjusted,false);});
test('headway-free builder binds each schedule instance to the correct shape and platforms',()=>{const files={'routes.txt':'route_id,route_type,route_short_name\nR,1,NS\n','trips.txt':'route_id,trip_id,shape_id\nR,T,S\n','stops.txt':'stop_id,stop_name,stop_lat,stop_lon\nA,Alpha,1,103\nB,Beta,1.01,103.01\n','stop_times.txt':'trip_id,stop_id,stop_sequence,shape_dist_traveled\nT,A,0,0\nT,B,1,100\n','shapes.txt':'shape_id,shape_pt_lat,shape_pt_lon,shape_pt_sequence,shape_dist_traveled\nS,1,103,0,0\nS,1.01,103.01,1,100\n'};const schedule={version:1,runs:[{...run,route:'R',calls:[['A',0,10],['B',110,120]]}],bands:[]};const data=buildMap(files,schedule);assert.equal(data.stations.length,2);assert.equal(data.patterns[0].shape,'S');assert.throws(()=>buildMap(files,{...schedule,runs:[{...schedule.runs[0],calls:[['B',0,10],['A',110,120]]}]}),/mismatch/);});

test('future-only delays do not relabel current schedule position',()=>{const p=positionRun(run,pattern,shape,1050,live({s:{C:60}},1050));assert.equal(p.adjusted,false);assert.equal(p.delay,0);});
test('frequency run rejects a trip update without an instance start time',()=>{assert.ok(positionRun({...run,frequency:true},pattern,shape,1150,live({c:1})));});
