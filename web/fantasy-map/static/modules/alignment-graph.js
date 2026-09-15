/** Shortest connected alignment between projected endpoints; no invented links. */
const distance = (a,b) => Math.hypot((a[0]-b[0])*111320, (a[1]-b[1])*111320*Math.cos((a[0]+b[0])*Math.PI/360));

export function matchAlignment(start, end, features, { accept = () => true, label = "rail", nodeIdentity = false } = {}) {
  const vertices=[], graph=[], keys=new Map(), edges=[];
  function vertex(p, identity) {
    const key=identity ?? p.join(',');
    if(!keys.has(key)){keys.set(key,vertices.length);vertices.push(p);graph.push([]);}
    return keys.get(key);
  }
  function connect(a,b){const cost=distance(vertices[a],vertices[b]);graph[a].push([b,cost]);graph[b].push([a,cost]);}
  for(const rail of features) {
    if(!accept(rail))continue;
    for(let i=1;i<rail.coordinates.length;i++) {
      const identity = index => nodeIdentity ? (rail.nodeIds?.[index] ?? `${rail.id}:${index}`) : undefined;
      const a=vertex(rail.coordinates[i-1], identity(i-1)), b=vertex(rail.coordinates[i], identity(i));
      if(a===b)continue;
      connect(a,b);edges.push([a,b]);
    }
  }
  // Find a compatible pair on the same connected component. The independently
  // nearest edges may be disconnected service roads or parallel tracks.
  const component = Array(vertices.length).fill(-1);
  for (let root = 0; root < vertices.length; root++) {
    if (component[root] !== -1) continue;
    const stack = [root]; component[root] = root;
    while (stack.length) {
      const v = stack.pop();
      for (const [next] of graph[v]) if (component[next] === -1) {
        component[next] = root; stack.push(next);
      }
    }
  }
  function nearest(p) {
    const best=new Map();
    const cos=Math.cos(p[0]*Math.PI/180);
    for(let i=0;i<edges.length;i++){
      const [a,b]=edges[i],from=vertices[a],to=vertices[b];
      const dx=(to[1]-from[1])*cos,dy=to[0]-from[0];
      const t=Math.max(0,Math.min(1,((p[1]-from[1])*cos*dx+(p[0]-from[0])*dy)/(dx*dx+dy*dy)));
      const point=[from[0]+t*(to[0]-from[0]),from[1]+t*(to[1]-from[1])];
      const gap=distance(p,point);
      const group=component[a];
      if(gap<=500 && (!best.has(group)||gap<best.get(group).gap))best.set(group,{edge:i,point,gap});
    }
    if(!best.size)throw new Error(`Place each endpoint within 500 m of a mapped ${label} alignment`);
    return best;
  }
  const starts=nearest(start),ends=nearest(end);
  let pair=null;
  for(const [group,from] of starts){
    const to=ends.get(group);
    if(to && (!pair || from.gap+to.gap<pair.from.gap+pair.to.gap))pair={from,to};
  }
  if(!pair)throw new Error(`No connected ${label} alignment found; add closer control points along the same alignment`);
  const {from,to}=pair;
  const first=vertex(from.point,"@start"),last=vertex(to.point,"@end");
  for(const [projection,v] of [[from,first],[to,last]])for(const end of edges[projection.edge])connect(v,end);
  if(from.edge===to.edge)connect(first,last);
  // Binary heap keeps long rail networks from turning into quadratic searches.
  const heap=[];
  function push(item){let i=heap.length;heap.push(item);while(i>0){const p=(i-1)>>1;if(heap[p][0]<=item[0])break;heap[i]=heap[p];i=p;}heap[i]=item;}
  function pop(){const value=heap[0],tail=heap.pop();if(heap.length){let i=0;while(i*2+1<heap.length){let c=i*2+1;if(c+1<heap.length&&heap[c+1][0]<heap[c][0])c++;if(heap[c][0]>=tail[0])break;heap[i]=heap[c];i=c;}heap[i]=tail;}return value;}
  const costs=new Map([[first,0]]),previous=new Map();
  push([0,first]);
  while(heap.length){const [cost,v]=pop();if(cost!==costs.get(v))continue;if(v===last)break;
    for(const [next,weight] of graph[v]){const total=cost+weight;if(total<(costs.get(next)??Infinity)){costs.set(next,total);previous.set(next,v);push([total,next]);}}
  }
  if(!costs.has(last))throw new Error(`No connected ${label} alignment found; add closer control points along the same alignment`);
  const path=[];
  for(let v=last;;v=previous.get(v)){path.push(vertices[v]);if(v===first)break;}
  path.reverse();
  return {coordinates:[start,...path,end], source:`OpenStreetMap ${label} alignment`};
}

