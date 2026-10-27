"""Paired distance/time and safety comparison of student checkpoints.

From python/: uv run python ../scripts/benchmark-students.py OUT.json old=OLD.pt new=NEW.pt
The first policy defines the evaluation task. All policies see the same scenarios, target
range and field costs. Replay keys include the exact pose, target, fuel and episode seed.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
import torch

from fsim_rl.checkpoint import load_policy
from fsim_rl.config import SCENARIO_KINDS, config_to_dict, with_overrides
from fsim_rl.evaluate import evaluate


def trip_key(record):
    return (
        record['scenario'], record['map'], record['seed'], record['env_index'], record['vehicle'], record['fuel'],
        record['start']['x'], record['start']['y'], record['start']['orientation'],
        record['target']['x'], record['target']['y'], record.get('start_speed', 0.0),
    )


def safety(records):
    results = [r['result'] for r in records]
    n = len(results)
    return {
        'episodes': n,
        'success_rate': sum(r['status'] == 'success' for r in results) / n,
        'collision_free_success_rate': sum(r['status'] == 'success' and r['collisions'] == 0 for r in results) / n,
        'tree_hit_rate': sum(r['collisions_by_kind']['tree'] > 0 for r in results) / n,
        'contact_ticks_per_episode': {kind: sum(r['collisions_by_kind'][kind] for r in results) / n
                                      for kind in ('tree', 'rock', 'cliff', 'nest', 'water')},
        'mean_damage': sum(r['damage'] for r in results) / n,
        'nest_zone_fraction': sum(r['nest_zone_ticks'] for r in results) / max(sum(r['ticks'] for r in results), 1),
    }


def paired(reference, candidate):
    left = {trip_key(r): r['result'] for r in reference}
    right = {trip_key(r): r['result'] for r in candidate}
    if len(left) != len(reference) or len(right) != len(candidate) or left.keys() != right.keys():
        raise ValueError('evaluation tasks did not match exactly')
    pairs = [(a, right[k]) for k, a in left.items()
             if a['status'] == 'success' and right[k]['status'] == 'success']
    result = {'matched_episodes': len(left), 'common_successes': len(pairs)}
    for key, scale, label in (('ticks', 1 / 60, 'time_s'), ('path_length', 1, 'path_tiles')):
        a = np.asarray([p[0][key] * scale for p in pairs])
        b = np.asarray([p[1][key] * scale for p in pairs])
        if len(a) == 0:
            result[label] = None
            continue
        delta = b - a
        rng = np.random.default_rng(20261027)
        samples = delta[rng.integers(0, len(delta), size=(2000, len(delta)))].mean(axis=1)
        result[label] = {
            'reference_mean': float(a.mean()), 'candidate_mean': float(b.mean()),
            'change_percent': float(100 * (b.mean() / a.mean() - 1)),
            'mean_paired_delta': float(delta.mean()),
            'delta_95_percent_bootstrap': np.quantile(samples, [0.025, 0.975]).tolist(),
        }
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('out', type=Path)
    parser.add_argument('policies', nargs='+', help='NAME=CHECKPOINT')
    parser.add_argument('--episodes', type=int, default=200)
    parser.add_argument('--num-envs', type=int, default=16)
    parser.add_argument('--seed', type=int, default=12345)
    parser.add_argument('--split', choices=('train', 'holdout'), default='holdout')
    parser.add_argument('--scenarios', nargs='+', choices=SCENARIO_KINDS, default=list(SCENARIO_KINDS))
    args = parser.parse_args()
    torch.set_num_threads(1)
    device = torch.device('cpu')
    if args.episodes < 1 or args.num_envs < 1:
        parser.error('episodes and num-envs must be positive')
    models = {}
    for entry in args.policies:
        name, sep, path = entry.partition('=')
        if not sep or not name or name in models or not name.replace('-', '').replace('_', '').isalnum():
            parser.error('use unique alphanumeric policy names: NAME=CHECKPOINT')
        model, config, state = load_policy(path, device)
        models[name] = model, config, state, path
    reference = next(iter(models))
    task = config_to_dict(models[reference][1])['env']
    args.out.parent.mkdir(parents=True, exist_ok=True)
    replay_dir = args.out.parent / (args.out.stem + '-replays')
    replay_dir.mkdir(exist_ok=True)
    report = {'split': args.split, 'seed': args.seed, 'episodes_per_scenario': args.episodes,
              'reference': reference, 'policies': {k: {'checkpoint': v[3], 'step': v[2]['global_step']}
                                                  for k, v in models.items()}, 'rows': {}}
    all_records = {name: [] for name in models}
    for kind in args.scenarios:
        row, records = {}, {}
        for name, (model, cfg, state, _) in models.items():
            cfg = with_overrides(cfg, {'env': {k: task[k] for k in (
                'map_pool', 'target', 'time_limit', 'arrival', 'vehicle_mix', 'distance_field', 'scenarios',
            )}})
            replay = replay_dir / f'{name}-{kind}.json'
            metrics = evaluate(cfg, args.split, args.episodes, args.num_envs, args.seed, device,
                               model=model, spec_hash=state['spec_hash'], scenario=kind, replays=replay)
            records[name] = json.loads(replay.read_text())
            if len(records[name]) != args.episodes:
                raise ValueError('evaluation ended before all requested episodes finished')
            all_records[name].extend(records[name])
            row[name] = {'safety': safety(records[name]), 'metrics': metrics}
            row[name]['paired'] = paired(records[reference], records[name])
            p = row[name]['paired']
            time_delta = p['time_s']['change_percent'] if p['time_s'] else float('nan')
            path_delta = p['path_tiles']['change_percent'] if p['path_tiles'] else float('nan')
            print(f"{kind:>8} {name:>16} success={metrics['success_rate']:.3f} "
                  f"time={time_delta:+.2f}% path={path_delta:+.2f}% "
                  f"tree={metrics['tree_hit_rate']:.3f} paired={p['common_successes']}", flush=True)
        report['rows'][kind] = row
        report['aggregate'] = {name: {'safety': safety(rows), 'paired': paired(all_records[reference], rows)}
                               for name, rows in all_records.items()}
        args.out.write_text(json.dumps(report, indent=2, allow_nan=False) + '\n')


if __name__ == '__main__':
    main()
