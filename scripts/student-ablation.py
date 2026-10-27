"""Measure student dependence on route guidance and the coarse map.

From python/: uv run python ../scripts/student-ablation.py CHECKPOINT OUT.json
Each variant uses the same holdout seeds and includes per-vehicle collision metrics.
Ablations change inputs without retraining; they are diagnostics, not deployment policies.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import torch

from fsim_rl.checkpoint import load_policy, resolve_device
from fsim_rl.config import SCENARIO_KINDS, with_overrides
from fsim_rl.env import import_fsim
from fsim_rl.evaluate import evaluate

LOOKAHEADS = (4, 8, 16, 32)


class AblatedStudent:
    """Alter raw student inputs, preserving all other observation features."""

    def __init__(self, model, features: list[str], variant: str):
        self.model = model
        self.variant = variant
        self.features = len(features)
        index = {name: i for i, name in enumerate(features)}
        self.route = [(s, index[f"route{s}_right"], index[f"route{s}_forward"]) for s in LOOKAHEADS]
        self.length = index["route_length"]
        self.dir = index["target_dir_right"], index["target_dir_forward"]
        self.straight = index["straight_distance"]

    def act(self, obs, deterministic: bool = False):
        x = obs["student"].clone()
        if self.variant == "straight":
            straight = x[:, self.straight] * 64.0
            for s, right, forward in self.route:
                scale = torch.clamp(straight / s, max=1.0)
                x[:, right] = x[:, self.dir[0]] * scale
                x[:, forward] = x[:, self.dir[1]] * scale
            x[:, self.length] = x[:, self.straight]
        elif self.variant == "no_map":
            if x.shape[1] <= self.features:
                raise ValueError("no_map needs a student with an appended coarse map")
            # Raw zero means open ground, including no nest zones. Rays stay intact.
            x[:, self.features:] = 0
        else:
            raise ValueError(f"unknown ablation {self.variant!r}")
        return self.model.act({**obs, "student": x}, deterministic=deterministic)

    def env_action(self, action):
        return self.model.env_action(action)

    def overlays(self, result):
        return self.model.overlays(result)


class StraightRoute(AblatedStudent):
    def __init__(self, model, features: list[str]):
        super().__init__(model, features, "straight")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("out", type=Path)
    parser.add_argument("--episodes", type=int, default=200)
    parser.add_argument("--num-envs", type=int, default=16)
    parser.add_argument("--seed", type=int, default=12345)
    parser.add_argument("--device", default="cpu")
    parser.add_argument("--scenarios", nargs="+", choices=SCENARIO_KINDS, default=list(SCENARIO_KINDS))
    args = parser.parse_args()
    if args.episodes < 1 or args.num_envs < 1:
        parser.error("episodes and num-envs must be positive")
    torch.set_num_threads(1)
    device = resolve_device(args.device)
    model, config, state = load_policy(args.checkpoint, device)
    if config.model.arch != "student":
        parser.error("checkpoint must be a student")
    features = import_fsim().spec()["student"]["features"]
    variants = {
        "normal": (model, config),
        "straight": (StraightRoute(model, features), config),
    }
    if config.env.student_map:
        variants["no_map"] = (AblatedStudent(model, features, "no_map"), config)
    if config.env.route_source != "pathfinder":
        variants["zone_blind"] = (model, with_overrides(config, {"env": {"distance_field": {"nest_zone_cost": 0.0}}}))
    report = {
        "checkpoint": str(args.checkpoint), "global_step": state["global_step"],
        "spec_hash": state["spec_hash"], "route_source": config.env.route_source,
        "episodes_per_scenario": args.episodes, "num_envs": args.num_envs,
        "seed": args.seed, "split": "holdout", "rows": {},
        "notes": ["Input ablations are out of training distribution, not proposed driving policies."],
    }
    if config.env.route_source == "pathfinder":
        report["notes"].append("zone_blind omitted: pathfinder routes already ignore zones; changing field cost also changes reward and sampling.")
    args.out.parent.mkdir(parents=True, exist_ok=True)
    for kind in args.scenarios:
        report["rows"][kind] = {}
        for name, (policy, cfg) in variants.items():
            metrics = evaluate(cfg, "holdout", args.episodes, args.num_envs, args.seed, device,
                               model=policy, spec_hash=state["spec_hash"], scenario=kind)
            report["rows"][kind][name] = metrics
            print(f"{kind:>8} {name:>10}: success {metrics['success_rate']:.3f} spl {metrics['spl']:.3f} "
                  f"tree-hit {metrics['tree_hit_rate']:.3f} nest-zone {metrics['nest_zone_fraction']:.3f}", flush=True)
            args.out.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
