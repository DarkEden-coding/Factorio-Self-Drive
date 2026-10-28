// Fixed real-map route geometry comparison, independent of policy actions.
// node scripts/benchmark-route-smoothing.mjs OUT.json
import {readFile,writeFile} from 'node:fs/promises';
import init,{Sim} from '../crates/fsim-wasm/pkg/fsim_wasm.js';
await init({module_or_path:await readFile(new URL('../crates/fsim-wasm/pkg/fsim_wasm_bg.wasm',import.meta.url))});
const maps=['crates/fsim-core/tests/fixtures/nauvis-origin.fsmap',
  'data/game/maps/nauvis-seed-12345.fsmap'];
// Fall back to the portable fixture if this particular local map is absent.
const inputs=[];
for(const path of maps){try{inputs.push([path,await readFile(path)]);}catch(error){if(error.code!=='ENOENT')throw error;}}
function geometry(points){
 let length=0,freeAngleLength=0;
 for(let i=2;i<points.length;i+=2){
  const dx=points[i]-points[i-2],dy=points[i+1]-points[i-1],l=Math.hypot(dx,dy);
  if(l<1e-6)continue;
  const a=Math.atan2(dy,dx),offset=Math.abs(a-Math.round(a/(Math.PI/4))*Math.PI/4);
  length+=l;if(offset>Math.PI/180)freeAngleLength+=l;
 }
 return {vertices:points.length/2,length,free_angle_length:freeAngleLength};
}
const rows=[];
for(const [map,bytes] of inputs)for(const vehicle of ['car','tank']){
 const sim=new Sim(JSON.stringify({vehicle,fuel:'coal'}),bytes);
 for(let seed=1;seed<=100;seed++){
  const setup=JSON.parse(sim.sample_episode(JSON.stringify({seed,min_distance:16,max_distance:100,
   vehicle,fuel:'coal',distance_field:{nest_zone_cost:4},arrival:{mode:'stop',stop_radius:1,stop_speed:0.01}})));
  const grid=geometry(sim.baseline_path()),smooth=geometry(sim.smooth_path());
  if(smooth.length>grid.length+1e-4)throw Error('Smoothing lengthened the raw route');
  rows.push({map,vehicle,seed,grid,smooth});
 }
 sim.free();
}
const sum=(key,field)=>rows.reduce((s,r)=>s+r[key][field],0);
const summary={cases:rows.length,grid_length:sum('grid','length'),smooth_length:sum('smooth','length'),
 grid_vertices:sum('grid','vertices'),smooth_vertices:sum('smooth','vertices'),
 grid_free_angle_fraction:sum('grid','free_angle_length')/sum('grid','length'),
 smooth_free_angle_fraction:sum('smooth','free_angle_length')/sum('smooth','length')};
summary.length_change_percent=100*(summary.smooth_length/summary.grid_length-1);
await writeFile(process.argv[2]??'data/autodrive/smooth-route-geometry.json',JSON.stringify({summary,rows},null,2)+'\n');
process.stdout.write(JSON.stringify(summary,null,2)+'\n');
