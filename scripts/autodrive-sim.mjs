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
  // `drive` cases report a failed start (e.g. an unreachable target) instead of aborting the batch.
  try {
  if(c.request) setup=JSON.parse(sim.start_episode(JSON.stringify({...c.request,distance_field:c.field??c.request.distance_field??model.field})));
  else setup=JSON.parse(sim.sample_episode(JSON.stringify({seed:c.seed??12345,min_distance:15,max_distance:100,
    arrival:{mode:'stop',stop_radius:1,stop_speed:0.01},vehicle:c.vehicle??'car',fuel:c.fuel??'coal',distance_field:c.field??model.field})));
  } catch(error) {
    if(!c.drive) throw error;
    result.push({error:String(error)});sim.free();continue;
  }
  if(c.drive){
    // The mod's escape rule (mod/factorio-autodrive/escape.lua, python/src/fsim_rl/escape.py).
    const e={blocked:0,left:0,action:4,last:4,base:4,attempt:0};
    const maneuvers=a=>{const t=Math.floor(a/3)-1,s=a%3-1,out=s?[[t,0]]:[];
      out.push([-t,0],[-t,s?-s:-1],[-t,s?s:1]);return out.map(([t,s])=>(t+1)*3+s+1);};
    let count=0,previous=null;
    const limited=(c.vehicle??'car')==='car'&&/maps-vulcanus/.test(c.map);
    while(JSON.parse(sim.episode_state()).status==='running' && count++<10000){
      let a=action(model.route_source==='field_smooth'?sim.policy_student_smooth():sim.policy_student());
      if(e.left===0&&(e.blocked>=6||(e.attempt>0&&e.blocked>=4))){
        if(e.attempt===0) e.base=e.last;
        const options=maneuvers(e.base);
        e.action=options[e.attempt%options.length];e.attempt++;e.left=4;e.blocked=0;
      }
      if(e.left>0){a=e.action;e.left--;}
      e.last=a;
      // The mod's Vulcanus car speed limit (driver.lua), after the escape rule: forward above
      // 130 km/h coasts.
      if(limited&&previous&&previous.speed*216>130&&Math.floor(a/3)===2) a=3+a%3;
      sim.decide(a);
      const v=JSON.parse(sim.vehicle_state());
      if(previous){
        const moved=Math.hypot(v.x-previous.x,v.y-previous.y)>=0.02;
        e.blocked=Math.floor(e.last/3)!==1&&!moved?e.blocked+1:0;
        if(moved) e.attempt=0;
      }
      previous=v;
    }
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
