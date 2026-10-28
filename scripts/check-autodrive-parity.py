#!/usr/bin/env python3
"""Compare production Lua raster/observation (rays, route features, map)/student with Rust/WASM
and student_ref.

Uses fixed episodes on real .fsmap files. No engine or training, a few seconds.
--engine-output also checks initial snapshots from test-autodrive.py.
"""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'python/src'))
from fsim_rl import student_ref

spec = importlib.util.spec_from_file_location('build_weights', ROOT / 'scripts/build-autodrive-weights.py')
build_weights = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build_weights)
GROUPS = {'hard': (0,32), 'tree': (32,64), 'zone': (64,96), 'route': (96,105), 'state': (105,135),
          }
# Map reach: 8 cells of 8 tiles times sqrt(2), plus a block.
MAP_REACH = 112


def decode(path):
    raw = path.read_bytes()
    assert raw[:8] == b'FSMAP001'
    size, _ = struct.unpack_from('<II',raw,8)
    h = json.loads(raw[16:16+size])
    body = zlib.decompress(raw[16+size:])
    (x0,y0),(x1,y1) = h['area']
    w,height = x1-x0,y1-y0
    offset = (x0+w//2,y0+height//2)
    entities = []
    prototypes = json.loads((ROOT/'data/prototypes.json').read_text())
    boxes = {e['name']:e for e in prototypes['entities']}
    for i in range(h['entity_count']):
        name,x,y,orientation,cliff,health = struct.unpack_from('<HdddHf',body,2*w*height+i*32)
        name,kind = h['entity_names'][name]
        p=boxes.get(name) or boxes[{'tree':'tree-02','nest':'biter-spawner','rock':'big-rock'}.get(kind,'big-rock')]
        entities.append(dict(x=x-offset[0],y=y-offset[1],orientation=orientation,kind=kind,
            box=p['collision_box'],health=health if math.isfinite(health) else 1,
            cliff_orientation=h['cliff_names'][cliff] if cliff!=65535 else None))
    tiles = body[:w*height]
    bounds=dict(x0=-w//2,y0=-height//2,x1=w//2,y1=height//2)
    return h,tiles,entities,bounds


def stats(data, vehicle, fuel, tile):
    p=next(v for v in data['vehicles'] if v['name']==vehicle)
    f=next(v for v in data['fuels'] if v['name']==fuel)
    a,t=f['fuel_acceleration_multiplier'],f['fuel_top_speed_multiplier']
    power=p['consumption']*p['effectivity']*a*t*t
    grass=next(v for v in data['tiles'] if v['name']=='grass-1')['vehicle_friction_modifier']
    r=1-p['friction']*(1+(grass-1)*p['terrain_friction_modifier'])
    return dict(tank_driving=p['tank_driving'],weight=p['weight'],max_health=p['max_health'],
        rotation_speed=p['rotation_speed'],braking_power=p['braking_power'],power=power,
        top_speed=math.sqrt(2*power/(60*p['weight'])*r*r/(1-r*r))/60,
        half_width=max(abs(p['collision_box'][0][0]),abs(p['collision_box'][1][0])),
        half_length=max(abs(p['collision_box'][0][1]),abs(p['collision_box'][1][1])),
        fuel_acceleration=a,fuel_top_speed=t,tile_friction=tile)


def fixture(path, result, data):
    h,tiles,entities,bounds=decode(path)
    v,e=result['state'],result['episode']
    target=result['target']
    state=dict(x=v['x'],y=v['y'],orientation=v['orientation'],speed=v['speed'],health=v['health'],
        target=dict(x=target['x'],y=target['y']),elapsed=0,stall=0,time_limit=e['time_limit_ticks'])
    f=result['field']
    w=bounds['x1']-bounds['x0']
    def tile(x,y):
        if not bounds['x0']<=x<bounds['x1'] or not bounds['y0']<=y<bounds['y1']:
            return 'offmap'
        return h['tile_names'][tiles[(y-bounds['y0'])*w+x-bounds['x0']]]
    tile_name=tile(math.floor(v['x']),math.floor(v['y']))
    friction=next(t for t in data['tiles'] if t['name']==tile_name)['vehicle_friction_modifier']
    blocks=[]
    vx,vy=math.floor(v['x']),math.floor(v['y'])
    x0, y0 = min(vx-MAP_REACH,f['x_min']-3), min(vy-MAP_REACH,f['y_min']-3)
    x1, y1 = max(vx+MAP_REACH,f['x_min']+f['width']+3), max(vy+MAP_REACH,f['y_min']+f['height']+3)
    for y in range(y0//16*16,(y1+16)//16*16,16):
        for x in range(x0//16*16,(x1+16)//16*16,16):
            water=[]
            for dy in range(16):
                for dx in range(16):
                    name=tile(x+dx,y+dy)
                    water.append('offmap' if name=='offmap' else name in ('water','deepwater','out-of-map'))
            nearby=[a for a in entities if x-26<=a['x']<x+42 and y-26<=a['y']<y+42]
            blocks.append(dict(x=x,y=y,tiles=water,entities=nearby))
    route=[dict(x=a,y=b) for a,b in zip(result['route'][::2],result['route'][1::2])]
    return dict(state=state,stats=stats(data,v['kind'],v['fuel'],friction),blocks=blocks,bounds=bounds,route=route,field=f)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=ROOT/'data/autodrive/offline-parity.json')
    parser.add_argument('--engine-output',type=Path)
    parser.add_argument('--mod-source',type=Path,default=ROOT/'mod/fsim-autodrive')
    parser.add_argument('--model',type=Path,default=build_weights.DEFAULT,help='the exported student the mod ships')
    args=parser.parse_args()
    data=json.loads((ROOT/'data/prototypes.json').read_text())
    maps=[ROOT/'crates/fsim-core/tests/fixtures/nauvis-origin.fsmap']
    maps += sorted((ROOT/'data/game/maps').glob('*.fsmap'))[:2]
    cases=[]
    for path in maps:
        for vehicle in ('car','tank'):
            for seed in (15,27):
                cases.append(dict(map=str(path),vehicle=vehicle,fuel='coal' if seed==15 else 'nuclear-fuel',seed=seed))
    raw=subprocess.run(['node','scripts/autodrive-sim.mjs',str(args.model)],input=json.dumps(cases),cwd=ROOT,
        capture_output=True,text=True,check=True)
    results=json.loads(raw.stdout)
    fixtures=[fixture(Path(c['map']),r,data) for c,r in zip(cases,results)]
    # Include moving and reversing versions of these fixed real-map poses.
    moving=[]
    for c,r in zip(cases[:4],results[:4]):
        request={k:r['setup'][k] for k in ('start','target','vehicle','fuel','radius')}
        request.update(start_speed=0.12 if c['seed']==15 else -0.08,
            arrival=dict(mode='stop',stop_radius=1,stop_speed=0.01))
        moving.append({**c,'request':request})
    raw=subprocess.run(['node','scripts/autodrive-sim.mjs',str(args.model)],input=json.dumps(moving),cwd=ROOT,
        capture_output=True,text=True,check=True)
    more=json.loads(raw.stdout)
    fixtures.extend(fixture(Path(c['map']),r,data) for c,r in zip(moving,more))
    cases.extend(moving);results.extend(more)
    with tempfile.TemporaryDirectory(prefix='fsim-lua-parity-') as temp:
        source=Path(temp)/'fixtures.lua'
        source.write_text('return '+build_weights.lua(fixtures)+'\n')
        raw=subprocess.run(['lua','scripts/check-autodrive-lua.lua',str(source),str(args.mod_source)],cwd=ROOT,
            capture_output=True,text=True,check=True)
    lines=raw.stdout.splitlines()
    model=student_ref.load(str(args.model))
    maxima={g:0.0 for g in GROUPS};logit_max=0;agreement=0;field_max=0
    for i,r in enumerate(results):
        obs=[float(v) for v in lines[i*4].split(',')]
        logits=[float(v) for v in lines[i*4+1].split(',')]
        action=int(lines[i*4+2])
        for group,(a,b) in GROUPS.items():
            maxima[group]=max(maxima[group],max(abs(obs[j]-r['observation'][j]) for j in range(a,b)))
        values=[float(v) for v in lines[i*4+3].split(',')]
        assert len(values)==len(r['values'])
        for actual, expected_value in zip(values,r['values']):
            if expected_value is None:
                assert actual==math.inf
            else:
                assert math.isfinite(actual)
                field_max=max(field_max,abs(actual-expected_value))
        expected=student_ref.logits(model,obs)
        logit_max=max(logit_max,max(abs(a-b) for a,b in zip(logits,expected)))
        agreement+=action==student_ref.act(model,r['observation'][:model['obs']['size']])
    report=dict(source='offline Lua vs existing Rust/WASM on real map exports',cases=len(results),
        max_abs_by_feature_group=maxima,logits_max_abs=logit_max,action_agreement=agreement,
        field_max_abs=field_max,engine_verified=False)
    if args.engine_output:
        report['engine']=check_engine(args.engine_output,model,args.model)
        report['engine_verified']=True
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
    assert max(maxima.values())<1e-5 and logit_max<1e-8 and agreement==len(results) and field_max==0


def published_route_features(route, state, lookaheads):
    """Independently encode the published corridor; Rust still checks rays/state."""
    def distance(a,b):
        return math.hypot(b['x']-a['x'],b['y']-a['y'])
    def point_along(length):
        for a,b in zip(route,route[1:]):
            span=distance(a,b)
            if span>=length and span>0:
                return dict(x=a['x']+length/span*(b['x']-a['x']),y=a['y']+length/span*(b['y']-a['y']))
            length-=span
        return route[-1]
    h=state['orientation']*2*math.pi
    sine,cosine=math.sin(h),math.cos(h)
    out=[]
    for length in lookaheads:
        point=point_along(length)
        dx,dy=point['x']-state['x'],point['y']-state['y']
        out.extend(((dx*cosine+dy*sine)/length,(dx*sine-dy*cosine)/length))
    out.append(min(8,sum(distance(a,b) for a,b in zip(route,route[1:]))/64))
    return [struct.unpack('<f',struct.pack('<f',v))[0] for v in out]


def check_engine(directory,model,model_path):
    # Engine snapshots each have a matching .fsmap encoded by test-autodrive.py.
    cases=[];snapshots=[]
    for path in sorted(directory.glob('parity-*.json')):
        if not path.with_suffix('.fsmap').exists(): continue
        s=json.loads(path.read_text());v=s['state']
        cases.append(dict(map=str(path.with_suffix('.fsmap')),request=dict(start={k:v[k] for k in ('x','y','orientation')},
            target=v['target'],start_speed=v['speed'],vehicle=s['vehicle'],fuel=s['fuel'],radius=3,
            arrival=dict(mode='stop',stop_radius=1,stop_speed=0.01))))
        snapshots.append(s)
    if not cases: raise ValueError('no engine parity snapshots')
    raw=subprocess.run(['node','scripts/autodrive-sim.mjs',str(model_path)],input=json.dumps(cases),cwd=ROOT,
        capture_output=True,text=True,check=True)
    maxima={g:0.0 for g in GROUPS};agreement=0;logit_max=0
    for s,r in zip(snapshots,json.loads(raw.stdout)):
        obs=s['observation']
        if 'destination' in s:
            # A progressive route intentionally differs from a standalone full
            # field. Compare its feature encoding separately, preserving the
            # independent Rust checks of every ray and vehicle/target feature.
            r['observation'][96:105]=published_route_features(s['route'],s['state'],model['obs']['route_lookaheads'])
        for group,(a,b) in GROUPS.items():
            maxima[group]=max(maxima[group],max(abs(obs[j]-r['observation'][j]) for j in range(a,b)))
        expected=student_ref.logits(model,obs)
        logit_max=max(logit_max,max(abs(a-b) for a,b in zip(s['logits'],expected)))
        agreement+=s['action']==student_ref.act(model,r['observation'][:model['obs']['size']])
    assert max(maxima.values())<1e-5 and logit_max<1e-8 and agreement==len(cases), (maxima,logit_max,agreement)
    return dict(cases=len(cases),max_abs_by_feature_group=maxima,logits_max_abs=logit_max,action_agreement=agreement,
        published_corridor_features=sum('destination' in s for s in snapshots))


if __name__=='__main__':
    main()
