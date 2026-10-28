#!/usr/bin/env python3
"""Compare paired Factorio trips, including startup, waits and all-episode safety."""
import argparse
import json
from pathlib import Path


def compare(baseline, candidate):
    for key in ('engine', 'suite', 'seed'):
        if baseline[key] != candidate[key]:
            raise ValueError(f'different {key}')
    old = {r['name']: r for r in baseline['results']}
    new = {r['name']: r for r in candidate['results']}
    if old.keys() != new.keys() or len(old) != len(baseline['results']) or len(new) != len(candidate['results']):
        raise ValueError('different or duplicate fixtures')
    pairs = []
    common = []
    for name, a in old.items():
        b = new[name]
        if 'initial' not in a or a['initial'] != b.get('initial'):
            raise ValueError(f'{name}: different initial state')
        success = a['status'] == b['status'] == 'arrived'
        if success:
            common.append((a, b))
        pairs.append(dict(name=name, baseline_status=a['status'], candidate_status=b['status'],
            baseline_total_ticks=a['total_ticks'], candidate_total_ticks=b['total_ticks'],
            time_change_pct=100*(b['total_ticks']/a['total_ticks']-1) if success else None,
            baseline_path=a['path_length'], candidate_path=b['path_length'],
            path_change_pct=100*(b['path_length']/a['path_length']-1) if success else None))
    def change(key):
        return 100*(sum(b[key] for a,b in common)/sum(a[key] for a,b in common)-1) if common else None
    def safety(results):
        return dict(arrived=sum(r['status']=='arrived' for r in results), total=len(results),
            damage=sum(r.get('damage',0) for r in results),
            contacts_estimate=sum(r.get('contacts_estimate',0) for r in results))
    return dict(seed=baseline['seed'], common_successes=len(common),
        total_time_change_pct=change('total_ticks'), total_path_change_pct=change('path_length'),
        baseline=safety(baseline['results']), candidate=safety(candidate['results']), pairs=pairs)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('baseline',type=Path)
    parser.add_argument('candidate',type=Path)
    parser.add_argument('--output',type=Path)
    args=parser.parse_args()
    result=compare(json.loads(args.baseline.read_text()),json.loads(args.candidate.read_text()))
    text=json.dumps(result,indent=2)+'\n'
    if args.output:
        args.output.parent.mkdir(parents=True,exist_ok=True)
        args.output.write_text(text)
    print(text,end='')


if __name__=='__main__':
    main()
