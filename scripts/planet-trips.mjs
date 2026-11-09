// Reachable trips for `test-autodrive.py --suite planets --trips FILE`.
// usage: node scripts/planet-trips.mjs [seed] [trips per planet and vehicle] > trips.json
// Starts follow the training sampler's start rules (`place_random`), inside the area the engine
// suite generates; targets lie 60-180 tiles away and must be reachable with tortuosity at most 2.5
// on the pool map of the same planet and seed. Coordinates in the output are Factorio's.
import {readFile} from 'node:fs/promises';
import init,{Sim} from '../crates/fsim-wasm/pkg/fsim_wasm.js';
await init({module_or_path:await readFile(new URL('../crates/fsim-wasm/pkg/fsim_wasm_bg.wasm',import.meta.url))});
const seed=Number(process.argv[2]??12345), count=Number(process.argv[3]??8);
const LIMIT=184; // keep starts and targets this far from the origin, leaving the field margin generated
let state=seed>>>0;
function random(){ state=(Math.imul(state,1664525)+1013904223)>>>0; return state/4294967296; }
const trips=[];
for (const planet of ['vulcanus','gleba','fulgora']) {
  const dir=new URL(`../data/game/maps-${planet}/`,import.meta.url);
  const entry=JSON.parse(await readFile(new URL('index.json',dir),'utf8')).find(e=>e.name===`${planet}-seed-${seed}`);
  if(!entry) throw new Error(`no ${planet} map for seed ${seed}`);
  const [ox,oy]=entry.source_offset;
  const bytes=await readFile(new URL(entry.file,dir));
  for (const vehicle of ['car','tank']) {
    const sim=new Sim(JSON.stringify({vehicle,fuel:'coal'}),bytes);
    let found=0, attempts=0;
    while(found<count && attempts++<20000){
      const start=JSON.parse(sim.place_random(Math.floor(random()*4294967296)));
      const sx=start.x+ox, sy=start.y+oy;
      if(Math.abs(sx)>LIMIT || Math.abs(sy)>LIMIT) continue;
      const angle=random()*2*Math.PI, distance=60+random()*120;
      const tx=Math.floor(sx+distance*Math.cos(angle))+0.5, ty=Math.floor(sy+distance*Math.sin(angle))+0.5;
      if(Math.abs(tx)>LIMIT || Math.abs(ty)>LIMIT) continue;
      let episode;
      try {
        episode=JSON.parse(sim.start_episode(JSON.stringify({start,target:{x:tx-ox,y:ty-oy},vehicle,fuel:'coal'})));
      } catch { continue; }
      if(episode.optimal_distance>2.5*Math.hypot(tx-sx,ty-sy)) continue;
      found++;
      trips.push({planet,vehicle,fuel:found%2===0?'rocket-fuel':'coal',start:[sx,sy],orientation:start.orientation,
        target:[tx,ty],name:`${planet}-${vehicle}-${found}`});
    }
    sim.free();
  }
}
process.stdout.write(JSON.stringify(trips,null,1)+'\n');
