/** Exercise the shipped importer against the actual LTA starter before deploy. */
import { readFile } from 'node:fs/promises';
import { readGtfsZip } from '../web/fantasy-map/static/modules/gtfs-zip.js';
import { buildGtfsNetwork } from '../web/fantasy-map/static/modules/gtfs-model.js';
const bytes=await readFile(process.argv[2]);
const files=await readGtfsZip(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength));
const result=buildGtfsNetwork(files);
console.log(`Singapore MRT starter: ${result.lines.length} rail patterns, ${result.lines.reduce((n,line)=>n+line.stations.length,0)} stops, ${result.fallback} stop-to-stop fallbacks, ${result.skipped} skipped patterns`);
