/** Read selected GTFS files from a bounded ZIP, entirely in the browser. */
export const MAX_ZIP_BYTES = 32 * 1024 * 1024;
const MAX_EXPANDED_BYTES = 64 * 1024 * 1024;
const wanted = new Set(['routes.txt','trips.txt','stops.txt','stop_times.txt','shapes.txt']);
const decoder = new TextDecoder('utf-8', {fatal:true});
const crcTable=Array.from({length:256},(_,i)=>{for(let j=0;j<8;j++)i=(i>>>1)^((i&1)?0xedb88320:0);return i>>>0;});
function crc32(bytes){let crc=0xffffffff;for(const b of bytes)crc=(crc>>>8)^crcTable[(crc^b)&255];return (crc^0xffffffff)>>>0;}
export async function readGtfsZip(buffer) {
  if(buffer.byteLength>MAX_ZIP_BYTES)throw new Error('GTFS ZIP must be 32 MiB or smaller');
  const view=new DataView(buffer),bytes=new Uint8Array(buffer),files=Object.create(null);
  let end=-1,total=0;
  for(let p=bytes.length-22;p>=Math.max(0,bytes.length-65557);p--){if(view.getUint32(p,true)===0x06054b50 && p+22+view.getUint16(p+20,true)===bytes.length){end=p;break;}}
  if(end<0)throw new Error('Not a valid GTFS ZIP archive');
  const count=view.getUint16(end+10,true),size=view.getUint32(end+12,true),offset=view.getUint32(end+16,true);
  if(view.getUint16(end+4,true)||view.getUint16(end+6,true)||view.getUint16(end+8,true)!==count||count===65535||offset+size>end)throw new Error('Split and ZIP64 archives are not supported');
  let p=offset;
  for(let i=0;i<count;i++){
    if(p+46>offset+size||view.getUint32(p,true)!==0x02014b50)throw new Error('Invalid ZIP directory');
    const flags=view.getUint16(p+8,true),method=view.getUint16(p+10,true),crc=view.getUint32(p+16,true);
    const compressed=view.getUint32(p+20,true),expanded=view.getUint32(p+24,true),nameLength=view.getUint16(p+28,true),extra=view.getUint16(p+30,true),comment=view.getUint16(p+32,true),local=view.getUint32(p+42,true);
    const next=p+46+nameLength+extra+comment;
    if(next>offset+size)throw new Error('Invalid ZIP filename');
    const name=decoder.decode(bytes.subarray(p+46,p+46+nameLength)).split('/').at(-1);
    p=next;
    if(!wanted.has(name))continue;
    if(Object.hasOwn(files,name))throw new Error('Archive contains multiple '+name+' files');
    if((flags&1)||![0,8].includes(method))throw new Error('Encrypted or unsupported ZIP compression');
    total+=expanded;
    if(total>MAX_EXPANDED_BYTES)throw new Error('GTFS tables exceed the 64 MiB expanded limit');
    if(local+30>offset||view.getUint32(local,true)!==0x04034b50)throw new Error('Invalid ZIP entry');
    const start=local+30+view.getUint16(local+26,true)+view.getUint16(local+28,true);
    if(start+compressed>offset)throw new Error('Truncated ZIP entry');
    let content=bytes.subarray(start,start+compressed);
    if(method===8){
      let stream;
      try{stream=new Blob([content]).stream().pipeThrough(new DecompressionStream('deflate-raw'));}
      catch{throw new Error('This browser cannot decompress GTFS ZIPs; use a current browser');}
      const reader=stream.getReader(),chunks=[];let length=0;
      try{while(true){const {done,value}=await reader.read();if(done)break;length+=value.length;if(length>expanded||length>MAX_EXPANDED_BYTES)throw new Error('GTFS entry exceeds its declared size');chunks.push(value);}}
      finally{await reader.cancel();}
      content=new Uint8Array(length);let at=0;for(const chunk of chunks){content.set(chunk,at);at+=chunk.length;}
    }
    if(content.length!==expanded||crc32(content)!==crc)throw new Error('Corrupt GTFS ZIP entry: '+name);
    files[name]=decoder.decode(content);
  }
  for(const name of ['routes.txt','trips.txt','stops.txt','stop_times.txt'])if(!Object.hasOwn(files,name))throw new Error('GTFS ZIP is missing '+name);
  return files;
}
