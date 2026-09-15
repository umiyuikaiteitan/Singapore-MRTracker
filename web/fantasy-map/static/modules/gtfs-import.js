/** GTFS import is additive and commits as one undoable editor operation. */
import { config } from './config.js';
import { MAX_ZIP_BYTES } from './gtfs-zip.js';
import { makeLine, uid } from './model.js';
import { state, pushHistory, invalidate, clearSelection } from './state.js';
import { map } from './map-setup.js';
import { render } from './render.js';
import { updateRadiusControl, updateToolButtons } from './controls.js';

const fileInput=document.getElementById('gtfs-file');
const choose=document.getElementById('import-gtfs');
const singapore=document.getElementById('start-singapore-mrt');
const loadUrl=document.getElementById('load-gtfs-url');
const urlInput=document.getElementById('gtfs-url');
const cancel=document.getElementById('cancel-gtfs');
const status=document.getElementById('gtfs-status');
let active=null;
function busy(value){choose.disabled=singapore.disabled=loadUrl.disabled=urlInput.disabled=value;cancel.hidden=!value;}
async function fetchZip(url,signal){
  const parsed=new URL(url,document.baseURI);
  if((parsed.protocol!=='https:' && !(parsed.protocol==='http:' && parsed.origin===new URL(document.baseURI).origin))||parsed.username||parsed.password)throw new Error('Use a public HTTPS GTFS ZIP URL without embedded credentials');
  const response=await fetch(parsed,{credentials:'omit',signal});
  if(!response.ok){await response.body?.cancel();throw new Error('GTFS download failed ('+response.status+')');}
  if(Number(response.headers.get('Content-Length'))>MAX_ZIP_BYTES){await response.body?.cancel();throw new Error('GTFS ZIP must be 32 MiB or smaller');}
  const reader=response.body.getReader(),chunks=[];let size=0;
  try{while(true){const {done,value}=await reader.read();if(done)break;size+=value.length;if(size>MAX_ZIP_BYTES)throw new Error('GTFS ZIP must be 32 MiB or smaller');chunks.push(value);}}
  finally{await reader.cancel();}
  const bytes=new Uint8Array(size);let at=0;for(const chunk of chunks){bytes.set(chunk,at);at+=chunk.length;}
  return bytes.buffer;
}
function parseInWorker(buffer,signal){
  return new Promise((resolve,reject)=>{
    const worker=new Worker(new URL('./gtfs-worker.js',import.meta.url),{type:'module'});
    const stop=()=>{worker.terminate();signal.removeEventListener('abort',abort);};
    const abort=()=>{stop();reject(signal.reason);};
    worker.onmessage=({data})=>{stop();if(data.error)reject(new Error(data.error));else resolve(data.result);};
    worker.onerror=()=>{stop();reject(new Error('GTFS import worker failed'));};
    signal.addEventListener('abort',abort,{once:true});
    if(signal.aborted){abort();return;}
    worker.postMessage(buffer,[buffer]);
  });
}
async function importGtfs(file, presetUrl=null){
  if(active)return;
  const controller=new AbortController();active=controller;busy(true);
  const timer=setTimeout(()=>controller.abort(new Error('GTFS import timed out; try a smaller feed')),90000);
  try{
    if(!file&&!presetUrl&&!urlInput.value.trim())throw new Error('Enter a public GTFS ZIP URL');
    status.textContent=file?'Reading GTFS ZIP…':'Downloading GTFS ZIP…';
    if(file&&file.size>MAX_ZIP_BYTES)throw new Error('GTFS ZIP must be 32 MiB or smaller');
    const buffer=file?await file.arrayBuffer():await fetchZip(presetUrl || urlInput.value.trim(),controller.signal);
    controller.signal.throwIfAborted();status.textContent='Building rail lines and stations…';
    const result=await parseInWorker(buffer,controller.signal);
    controller.signal.throwIfAborted();
    const lines=result.lines.map((partial,i)=>{
      const values={...partial};if(!values.color)delete values.color;
      return makeLine({...values,stations:values.stations.map(s=>({...s,id:uid('station')}))},state.lines.length+i);
    });
    // Nothing mutates the project until the whole feed has parsed successfully.
    pushHistory();state.lines.push(...lines);state.activeLineId=lines[0].id;state.extendFrom='end';clearSelection();
    invalidate();updateRadiusControl();updateToolButtons();render();
    map.fitBounds(L.latLngBounds(lines.flatMap(line=>line.segments[0].guide)),{padding:[40,40]});
    status.textContent=`Added ${lines.length} rail patterns and ${lines.reduce((n,l)=>n+l.stations.length,0)} stations. Undo restores the previous project.`
      +(result.fallback?` ${result.fallback} pattern(s) had no shape; stops are joined with straight segments.`:'')
      +(result.skipped?` Skipped ${result.skipped} pattern(s) with missing stops.`:'');
  }catch(error){status.textContent=error.message+(file?'':' If the feed host blocks browser downloads, download the ZIP and upload it here.');}
  finally{clearTimeout(timer);active=null;busy(false);fileInput.value='';}
}
choose.addEventListener('click',()=>fileInput.click());
fileInput.addEventListener('change',()=>{if(fileInput.files[0])importGtfs(fileInput.files[0]);});
loadUrl.addEventListener('click',()=>importGtfs(null));
singapore.addEventListener('click',()=>importGtfs(null,config.singaporeGtfsUrl));
cancel.addEventListener('click',()=>active?.abort(new Error('GTFS import cancelled')));
