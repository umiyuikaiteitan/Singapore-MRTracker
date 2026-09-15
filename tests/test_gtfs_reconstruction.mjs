import test from 'node:test';
import assert from 'node:assert/strict';
import { reconstruct, railData, shapeThroughStops } from '../scripts/reconstruct-gtfs.mjs';
const way=(id,nodes,coordinates,tags={})=>({type:'way',id,nodes,geometry:coordinates.map(([lat,lon])=>({lat,lon})),tags:{railway:'subway',...tags}});
const a=[1.3,103.8],b=[1.31,103.81],c=[1.3,103.82];
const osm={elements:[way(1,[1,2,3],[a,b,c])]};
const files={
 'routes.txt':'route_id,route_type,route_short_name\nR,1,NSL\n',
 'trips.txt':'route_id,trip_id\nR,T\n',
 'stops.txt':'stop_id,stop_name,stop_lat,stop_lon\nA,A,1.3,103.8\nC,C,1.3,103.82\n',
 'stop_times.txt':'trip_id,stop_id,stop_sequence\nT,A,1\nT,C,2\n'
};
test('reconstructs curved shape on real edges and validates shipped importer',()=>{const r=reconstruct(files,osm);assert.equal(r.report.reconstructed,1);assert.equal(r.report.fallback,0);assert.match(r.files['shapes.txt'],/1.31/);assert.deepEqual(r,reconstruct(files,osm));});
test('same-edge projections follow only the interval and reverse correctly',()=>{const ways=railData({elements:[way(1,[1,2],[a,c])]}).ways;const s=[1.3,103.805],e=[1.3,103.815];assert.deepEqual(shapeThroughStops(ways,[s,e]),[s,e]);assert.deepEqual(shapeThroughStops(ways,[e,s]),[e,s]);});
test('different node IDs at coincident crossings do not join',()=>{const data=railData({elements:[way(1,[1,2],[a,b]),way(2,[3,4],[b,c])]}).ways;assert.throws(()=>shapeThroughStops(data,[a,c]),/connected/);});
test('unmatched entire pattern stays shapeless and reports route and calls',()=>{const r=reconstruct(files,{elements:[way(1,[1,2],[a,[1.301,103.801]])]});assert.equal(r.report.reconstructed,0);assert.equal(r.report.fallback,1);assert.deepEqual(r.report.unmatched[0].stops,['A','C']);assert.equal(r.report.unmatched[0].route,'NSL');assert.deepEqual(r.report.unmatched[0].segments,[{from:'A',to:'C'}]);});
test('route relations prevent a shortcut on another corridor',()=>{const r=reconstruct(files,{elements:[...osm.elements,way(2,[1,3],[a,c]),{type:'relation',tags:{type:'route',route:'subway',ref:'NS'},members:[{type:'way',ref:1}]}]});assert.match(r.files['shapes.txt'],/1.31/);});
test('supplied shapes and trip IDs are preserved',()=>{const input={...files,'trips.txt':'route_id,trip_id,shape_id\nR,T,original\n','shapes.txt':'shape_id,shape_pt_lat,shape_pt_lon,shape_pt_sequence,shape_dist_traveled\noriginal,1.3,103.8,1,0\noriginal,1.3,103.82,2,2\n'};const r=reconstruct(input,osm);assert.equal(r.report.reconstructed,0);assert.equal(r.report.preserved,1);assert.match(r.files['trips.txt'],/"original"/);assert.match(r.files['shapes.txt'],/"shape_dist_traveled"/);assert.match(r.files['shapes.txt'],/"original","1.3","103.82","2","2"/);});
test('loop returns along mapped edges and branches remain separate',()=>{const data=railData({elements:[way(1,[1,2,3,1],[a,b,c,a])]}).ways;const result=shapeThroughStops(data,[a,b,c,a]);assert.deepEqual(result,[a,b,c,a]);});
test('malformed and incomplete Overpass responses fail',()=>{assert.throws(()=>railData({elements:[],remark:'timeout'}));assert.throws(()=>railData({elements:[way(1,[1],[a,c])]}));assert.throws(()=>railData({elements:[way(1,[1,2],[a,c],{service:'yard'})]}));});

test('parent-station coordinates and shape ID collisions are handled',()=>{const input={...files,'stops.txt':files['stops.txt'].replace('stop_lon\n','stop_lon,parent_station\n').replace('103.8\n','103.8,\n').replace('103.82\n','103.82,\n')+'P,Platform,,,A\n','stop_times.txt':files['stop_times.txt'].replace('T,A,1','T,P,1'),'shapes.txt':'shape_id,shape_pt_lat,shape_pt_lon,shape_pt_sequence\nosm-reconstructed-1,1.3,103.8,0\nosm-reconstructed-1,1.3,103.82,1\n'};const r=reconstruct(input,osm);assert.equal(r.report.reconstructed,1);assert.match(r.files['trips.txt'],/osm-reconstructed-2/);});
