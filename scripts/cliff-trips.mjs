// Reachable trips through cliff mazes, for `test-autodrive.py --suite planets --trips FILE`.
// usage: node scripts/cliff-trips.mjs SEED [trips per vehicle] [planet] > trips.json
// Scores 96x96-tile windows (step 32) of the pool map of that planet and seed inside the engine
// suite's generated area by cliff pieces, keeps windows with at least MIN_CLIFFS pieces and under
// 3% lava, and draws car and tank trips whose start and target both lie in such windows,
// 40-160 tiles apart, with no tortuosity limit. Coordinates in the output are Factorio's; each
// trip also records its straight and route distance and the cliff pieces around it.
// stderr reports the number of windows and trips.
import {readFile} from 'node:fs/promises';
import init,{Sim} from '../crates/fsim-wasm/pkg/fsim_wasm.js';
await init({module_or_path:await readFile(new URL('../crates/fsim-wasm/pkg/fsim_wasm_bg.wasm',import.meta.url))});
const seed=Number(process.argv[2]??3724854990), per=Number(process.argv[3]??8), planet=process.argv[4]??'vulcanus';
const LIMIT=184, W=96, STEP=32, MIN_CLIFFS=170, MAX_LAVA=0.03;
const dir=new URL(`../data/game/maps-${planet}/`,import.meta.url);
const entry=JSON.parse(await readFile(new URL('index.json',dir),'utf8')).find(e=>e.name===`${planet}-seed-${seed}`);
if(!entry) throw new Error(`no ${planet} map for seed ${seed}`);
const [ox,oy]=entry.source_offset, bytes=await readFile(new URL(entry.file,dir));

function cliffWindows(){
  const sim=new Sim(JSON.stringify({vehicle:'car',fuel:'coal'}),bytes);
  const info=JSON.parse(sim.map_info()), w=info.width, h=info.height;
  const lava=JSON.parse(sim.tile_palette()).map(p=>/lava/.test(p.name)?1:0), tiles=sim.tiles();
  const L=new Int32Array((w+1)*(h+1)), C=new Int32Array((w+1)*(h+1));
  for(const e of JSON.parse(sim.entities_in_rect(info.x_min,info.y_min,info.x_min+w,info.y_min+h))){
    const x=Math.floor(e.x-info.x_min), y=Math.floor(e.y-info.y_min);
    if(/cliff/.test(e.name)&&x>=0&&y>=0&&x<w&&y<h) C[(y+1)*(w+1)+x+1]++;
  }
  for(let y=0;y<h;y++) for(let x=0;x<w;x++){  // prefix sums
    const i=(y+1)*(w+1)+x+1;
    L[i]=lava[tiles[y*w+x]]+L[i-1]+L[i-w-1]-L[i-w-2];
    C[i]+=C[i-1]+C[i-w-1]-C[i-w-2];
  }
  const sum=(P,x,y)=>P[(y+W)*(w+1)+x+W]-P[y*(w+1)+x+W]-P[(y+W)*(w+1)+x]+P[y*(w+1)+x];
  const out=[];
  for(let y=0;y+W<=h;y+=STEP) for(let x=0;x+W<=w;x+=STEP){
    const gx=info.x_min+x+ox, gy=info.y_min+y+oy;
    if(gx<-LIMIT||gy<-LIMIT||gx+W>LIMIT||gy+W>LIMIT) continue;
    if(sum(C,x,y)>=MIN_CLIFFS && sum(L,x,y)<MAX_LAVA*W*W) out.push({x0:gx,y0:gy});
  }
  sim.free();
  return out;
}

const windows=cliffWindows();
const inRegion=(x,y)=>windows.some(w=>x>=w.x0+8&&y>=w.y0+8&&x<w.x0+W-8&&y<w.y0+W-8);
let state=seed>>>0;
function random(){ state=(Math.imul(state,1664525)+1013904223)>>>0; return state/4294967296; }
const trips=[];
for(const vehicle of ['car','tank']){
  const sim=new Sim(JSON.stringify({vehicle,fuel:'coal'}),bytes);
  let found=0;
  for(let attempt=0;windows.length&&found<per&&attempt<400000;attempt++){
    const start=JSON.parse(sim.place_random(Math.floor(random()*4294967296)));
    const sx=start.x+ox, sy=start.y+oy;
    if(!inRegion(sx,sy)) continue;
    const angle=random()*2*Math.PI, distance=40+random()*120;
    const tx=Math.floor(sx+distance*Math.cos(angle))+0.5, ty=Math.floor(sy+distance*Math.sin(angle))+0.5;
    if(!inRegion(tx,ty)) continue;
    let episode;
    try { episode=JSON.parse(sim.start_episode(JSON.stringify({start,target:{x:tx-ox,y:ty-oy},vehicle,fuel:'coal'}))); }
    catch { continue; }
    found++;
    const cliffs=JSON.parse(sim.entities_in_rect(Math.min(sx,tx)-ox-16,Math.min(sy,ty)-oy-16,
      Math.max(sx,tx)-ox+16,Math.max(sy,ty)-oy+16)).filter(e=>/cliff/.test(e.name)).length;
    trips.push({planet,vehicle,fuel:found%2===0?'rocket-fuel':'coal',start:[sx,sy],orientation:start.orientation,
      target:[tx,ty],name:`${planet}-${vehicle}-${found}`,straight:Math.hypot(tx-sx,ty-sy),
      route:episode.optimal_distance,cliffs_nearby:cliffs});
  }
  sim.free();
}
console.error(`${entry.name}: ${windows.length} cliff windows, ${trips.length} trips`);
console.log(JSON.stringify(trips));
