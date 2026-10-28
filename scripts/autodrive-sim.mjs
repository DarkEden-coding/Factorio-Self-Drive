// Read fixed state requests, return Rust/WASM observations without changing Rust.
import {readFile} from 'node:fs/promises';
import init,{Sim} from '../crates/fsim-wasm/pkg/fsim_wasm.js';
await init({module_or_path:await readFile(new URL('../crates/fsim-wasm/pkg/fsim_wasm_bg.wasm',import.meta.url))});
// `drive` cases run the dense field-route student (field-smooth) shipped in the mod.
const model=JSON.parse(await readFile(process.argv[2]??new URL('../python/runs/students/field-smooth-v1/field-smooth.json',import.meta.url),'utf8'));
function action(features){
 let x=Array.from(features,(v,i)=>Math.max(-model.normalizer.clip,Math.min(model.normalizer.clip,(v-model.normalizer.mean[i])/model.normalizer.std[i])));
 for(let n=0;n<model.dense.length;n++){const l=model.dense[n];x=l.bias.map((b,o)=>{let v=b;for(let i=0;i<l.in;i++)v+=l.weight[o][i]*x[i];return n+1<model.dense.length?Math.max(0,v):v;});}
 let best=0;for(let i=1;i<9;i++)if(x[i]>x[best])best=i;return best;
}
let input=''; for await (const chunk of process.stdin) input+=chunk;
const cases=JSON.parse(input),result=[];
for (const c of cases) {
  const sim=new Sim(JSON.stringify({vehicle:c.vehicle??'car',fuel:c.fuel??'coal'}),await readFile(c.map));
  let setup;
  if(c.request) setup=JSON.parse(sim.start_episode(JSON.stringify({...c.request,distance_field:c.field??c.request.distance_field??model.field})));
  else setup=JSON.parse(sim.sample_episode(JSON.stringify({seed:c.seed??12345,min_distance:15,max_distance:100,
    arrival:{mode:'stop',stop_radius:1,stop_speed:0.01},vehicle:c.vehicle??'car',fuel:c.fuel??'coal',distance_field:c.field??model.field})));
  if(c.drive){
    let count=0;
    while(JSON.parse(sim.episode_state()).status==='running' && count++<10000) sim.decide(action(model.route_source==='field_smooth'?sim.policy_student_smooth():sim.policy_student()));
    result.push({setup,final:JSON.parse(sim.episode_state()),vehicle:JSON.parse(sim.vehicle_state())});
    sim.free();continue;
  }
  const state=JSON.parse(sim.vehicle_state()),episode=JSON.parse(sim.episode_state());
  result.push({state,episode,setup,target:JSON.parse(sim.target_state()),observation:Array.from((c.route_source??model.route_source)==='field_smooth'?sim.policy_student_smooth():sim.policy_student_map()),
    field:JSON.parse(sim.distance_field()),values:Array.from(sim.distance_field_values(),v=>Number.isFinite(v)?v:null),
    route:Array.from((c.route_source??model.route_source)==='field_smooth'?sim.smooth_path():sim.baseline_path())});
  sim.free();
}
process.stdout.write(JSON.stringify(result));
