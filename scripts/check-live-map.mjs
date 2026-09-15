/** Build gate: every scheduled trip must have a valid track and renderable calls. */
import {readFile} from 'node:fs/promises';
import {positionRun} from '../web/live-map/engine.mjs';
const data=JSON.parse(await readFile(process.argv[2],'utf8'));
let checked=0;
for(const run of data.runs){const p=data.patterns[run.pattern],shape=data.shapes[p.shape];for(let i=0;i<p.stops.length-1;i++){const now=run.base+(run.times[2*i+1]+run.times[2*i+2])/2;const position=positionRun(run,p,shape,now,null);if(!position||!position.point.every(Number.isFinite))throw Error('Unplaceable run: '+run.id);checked++;}}
if(!checked||data.stations.length<2)throw Error('The map is empty');
console.log(`Live map validated: ${data.runs.length} runs, ${data.stations.length} stations, ${checked} train legs`);
