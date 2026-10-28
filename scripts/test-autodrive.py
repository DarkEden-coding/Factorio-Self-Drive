#!/usr/bin/env python3
"""Short isolated Factorio driving/parity run. Never opens user saves or mod-list.

Battery guard defaults on for a Mac: below 20% while discharging pauses before
starting an engine. Every child engine is terminated and reaped, including errors.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
from map_format import encode_map
spec=importlib.util.spec_from_file_location('collect_game_data',ROOT/'scripts/collect-game-data.py')
collector=importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)


def battery_guard(allow_low_battery=False):
    if sys.platform!='darwin': return
    result=subprocess.run(['pmset','-g','batt'],capture_output=True,text=True,check=True).stdout
    print(result.strip(),flush=True)
    match=re.search(r'(\d+)%;',result)
    if match and int(match[1])<20 and 'discharging' in result and not allow_low_battery:
        raise RuntimeError('PAUSED: battery below 20% and discharging. Connect power before running engine tests.')


def timings(path):
    groups={}
    if not path.exists(): return groups
    for line in path.read_text().splitlines():
        match=re.match(r'(\w+)\s+(?:Duration:\s*)?([\d.]+)\s*ms',line)
        if match: groups.setdefault(match[1],[]).append(float(match[2]))
    return {k:dict(samples=len(v),mean_ms=sum(v)/len(v),max_ms=max(v),
        p95_ms=sorted(v)[int((len(v)-1)*0.95)]) for k,v in groups.items()}


def run(args,temp):
    mods,write=temp/'mods',temp/'write'
    mods.mkdir();write.mkdir();(write/"saves").mkdir()
    mod_name = json.loads((args.mod_source/'info.json').read_text())['name']
    staged = mods/mod_name
    shutil.copytree(args.mod_source,staged)
    # Install verification hooks only in the disposable copy of the mod.
    fixtures = ROOT/'scripts/fixtures/autodrive'
    shutil.copyfile(fixtures/'test-runner.lua',staged/'test-runner.lua')
    with (staged/'control.lua').open('a') as control:
        control.write('\n'+(fixtures/'control.lua').read_text())
    companion=mods/'factorio-autodrive-verify';companion.mkdir()
    (companion/'info.json').write_text(json.dumps(dict(name='factorio-autodrive-verify',version='0.1.0',
        title='Isolated autodrive verification',author='Fsim',factorio_version='2.1',
        dependencies=['base >= 2.1.23',mod_name])))
    # Prevent biter AI in fixtures while retaining unmodified collision geometry.
    (companion/'data.lua').write_text('for _, nest in pairs(data.raw["unit-spawner"]) do nest.result_units={} end\n')
    (companion/'control.lua').write_text('''script.on_event(defines.events.on_tick,function()
  if not storage.started then
    storage.started=true
    game.speed=8
    remote.call("factorio-autodrive-test","start",{suite="SUITE",seed=SEED,save_reload=RELOAD,case=CASE})
  end
end)
'''.replace('SUITE',args.suite).replace('SEED',str(args.seed)).replace('RELOAD','true' if args.save_reload else 'false').replace('CASE',json.dumps(args.case) if args.case else 'nil'))
    enabled={'base',mod_name,'factorio-autodrive-verify'}
    names=[*sorted(enabled),'space-age','quality','elevated-rails','recycler']
    (mods/'mod-list.json').write_text(json.dumps({'mods':[dict(name=n,enabled=n in enabled) for n in names]}))
    config=temp/'config.ini'
    config.write_text(f'[path]\nread-data={args.factorio.parent.parent / "data"}\nwrite-data={write}\n'
        '[general]\ncheck-updates=false\nautosave-interval=0\n')
    base=[args.factorio,'-c',config,'--mod-directory',mods,'--disable-audio']
    save=temp/'verification.zip'
    battery_guard(args.allow_low_battery)
    collector.run_engine(base+['--create',save,'--map-gen-seed','12345'],temp/'create.log',args.timeout)
    battery_guard(args.allow_low_battery)
    output=write/'script-output/factorio-autodrive'
    server_settings=temp/'server-settings.json'
    server_config=json.loads((args.factorio.parent.parent/'data/server-settings.example.json').read_text())
    server_config.update(name='Isolated autodrive verification',description='Disposable local test',
        visibility=dict(public=False,lan=False),require_user_verification=False,auto_pause=False)
    server_settings.write_text(json.dumps(server_config))
    engine_args=['--start-server',save,'--server-settings',server_settings] if args.save_reload else ['--benchmark',save,'--benchmark-ticks','100000']
    marker=collector.run_engine(base+engine_args,
        temp/'drive.log',args.timeout,output)
    checkpoints=list((write/'saves').glob('autodrive-checkpoint.zip'))
    reload_verified=False
    if checkpoints:
        (output/'done.json').unlink()
        battery_guard(args.allow_low_battery)
        resumed=collector.run_engine(base+['--benchmark',checkpoints[0],'--benchmark-ticks','100000'],
            temp/'reload.log',args.timeout,output)
        if resumed['results']!=marker['results']:
            raise RuntimeError('save/reload changed the driving results')
        reload_verified=True
    if args.save_reload and not reload_verified:
        raise RuntimeError('no checkpoint was produced; save/reload was not verified')
    args.output.mkdir(parents=True,exist_ok=True)
    for pattern in ('parity-*.json','trace-*.json'):
        for path in output.glob(pattern): shutil.copyfile(path,args.output/path.name)
    for path in output.glob('map-*.json'):
        suffix=path.stem.removeprefix('map-')
        encoded,_=encode_map(json.loads(path.read_text()),'autodrive-'+suffix)
        (args.output/('parity-'+suffix+'.fsmap')).write_bytes(encoded)
    for name in ('create.log','drive.log'): shutil.copyfile(temp/name,args.output/name)
    if (output/'timing.txt').exists(): shutil.copyfile(output/'timing.txt',args.output/'timing.txt')
    results=marker['results']
    report=dict(engine='Factorio 2.1.23',suite=args.suite,seed=args.seed,save_reload_verified=reload_verified,results=results,
        success_rate=sum(r['status']=='arrived' for r in results)/len(results),
        timing=timings(output/'timing.txt'),contacts='blocked moving powered tick estimate, not engine collision events')
    (args.output/'driving-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
    if args.engine_only:
        return
    subprocess.run([sys.executable,'scripts/check-autodrive-parity.py','--engine-output',args.output,
        '--output',args.output/'parity-report.json'],cwd=ROOT,check=True)
    # Use identical initial terrain, pose, target and speed for a short simulator
    # comparison. LuaProfiler covers only production obs/forward and route/cache slices.
    requests=[]
    for path in sorted(args.output.glob('parity-*.json')):
        if not path.with_suffix('.fsmap').exists(): continue
        s=json.loads(path.read_text());v=s['state']
        requests.append(dict(map=str(path.with_suffix('.fsmap')),drive=True,
            request=dict(start={k:v[k] for k in ('x','y','orientation')},target=v['target'],
            start_speed=v['speed'],vehicle=s['vehicle'],fuel=s['fuel'],radius=3,
            arrival=dict(mode='stop',stop_radius=1,stop_speed=0.01))))
    result=subprocess.run(['node','scripts/autodrive-sim.mjs'],cwd=ROOT,input=json.dumps(requests),
        capture_output=True,text=True,check=True)
    (args.output/'simulator-comparison.json').write_text(result.stdout+'\n')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--factorio',type=Path,default=collector.DEFAULT_BINARY)
    parser.add_argument('--mod-source',type=Path,default=ROOT/'mod/factorio-autodrive',
        help='production mod directory to copy into the disposable test')
    parser.add_argument('--timeout',type=float,default=90)
    parser.add_argument('--output',type=Path,default=ROOT/'data/autodrive/engine')
    parser.add_argument('--keep-temp',action='store_true')
    parser.add_argument('--engine-only',action='store_true',
        help='run the production mod in Factorio; skip the legacy field-route simulator comparison')
    parser.add_argument('--case',help='run only this exact fixture name')
    parser.add_argument('--seed',type=int,default=12345,help='terrain seed for paired long trips')
    parser.add_argument('--suite',choices=('short','long'),default='short',help='short parity fixtures or long end-to-end trips')
    parser.add_argument('--save-reload',action='store_true',help='also verify a checkpoint saved by a disposable private server')
    parser.add_argument('--allow-low-battery',action='store_true',help='explicitly authorized short-run battery override')
    args=parser.parse_args();args.factorio=args.factorio.expanduser().resolve()
    args.mod_source=args.mod_source.expanduser().resolve()
    try:
        battery_guard(args.allow_low_battery)
        temp=Path(tempfile.mkdtemp(prefix='factorio-autodrive-'))
        print('Isolated directory:',temp,flush=True)
        try: run(args,temp)
        finally:
            if args.keep_temp: print('Kept:',temp)
            else: shutil.rmtree(temp)
    except (RuntimeError,OSError,subprocess.CalledProcessError) as error:
        print('test-autodrive:',error,file=sys.stderr)
        return 2
    return 0


if __name__=='__main__': sys.exit(main())
