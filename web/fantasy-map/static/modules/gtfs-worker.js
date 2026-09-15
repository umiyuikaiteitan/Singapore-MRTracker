import { readGtfsZip } from './gtfs-zip.js';
import { buildGtfsNetwork } from './gtfs-model.js';
self.onmessage = async ({data}) => {
  try { self.postMessage({result:buildGtfsNetwork(await readGtfsZip(data))}); }
  catch(error) { self.postMessage({error:error.message || 'GTFS import failed'}); }
};
