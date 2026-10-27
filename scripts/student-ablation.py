"""How much a student relies on the route features: suite runs with them altered.

Usage (from python/): uv run python ../scripts/student-ablation.py <student ckpt> <out.json>
- straight: every route point lies on the straight line to the target (no pathfinder at all):
  route<s> = target_dir * min(1, straight / s), route_length = straight_distance.
- zone_blind: the route comes from a field without nest-zone cost (env.distance_field.
  nest_zone_cost = 0), like Factorio's pathfinder, which doesn't know about nest zones.
Same holdout episodes as the main suite (seed 12345, 16 envs, 200 per kind).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import torch

from fsim_rl.checkpoint import load_policy, resolve_device
from fsim_rl.config import SCENARIO_KINDS, with_overrides
from fsim_rl.env import import_fsim
from fsim_rl.evaluate import evaluate

LOOKAHEADS = (4, 8, 16, 32)


class StraightRoute:
    """Wraps a student so its route features describe the straight line to the target."""

    def __init__(self, model, features: list[str]):
        self.model = model
        index = {name: i for i, name in enumerate(features)}
        self.route = [(s, index[f"route{s}_right"], index[f"route{s}_forward"]) for s in LOOKAHEADS]
        self.length = index["route_length"]
        self.dir = index["target_dir_right"], index["target_dir_forward"]
        self.straight = index["straight_distance"]

    def act(self, obs, deterministic: bool = False):
        x = obs["student"].clone()
        straight = x[:, self.straight] * 64.0
        for s, right, forward in self.route:
            scale = torch.clamp(straight / s, max=1.0)
            x[:, right] = x[:, self.dir[0]] * scale
            x[:, forward] = x[:, self.dir[1]] * scale
        x[:, self.length] = x[:, self.straight]
        return self.model.act({"student": x}, deterministic=deterministic)

    def env_action(self, action):
        return self.model.env_action(action)

    def overlays(self, result):
        return self.model.overlays(result)


def main(checkpoint: str, out: str) -> None:
    device = resolve_device("auto")
    model, config, state = load_policy(checkpoint, device)
    features = import_fsim().spec()["student"]["features"]
    variants = {
        "straight": (StraightRoute(model, features), config),
        "zone_blind": (model, with_overrides(config, {"env": {"distance_field": {"nest_zone_cost": 0.0}}})),
    }
    report = {"checkpoint": checkpoint, "rows": {}}
    for kind in SCENARIO_KINDS:
        report["rows"][kind] = {}
        for name, (policy, cfg) in variants.items():
            metrics = evaluate(cfg, "holdout", 200, 16, 12345, device, model=policy,
                               spec_hash=state["spec_hash"], scenario=kind)
            report["rows"][kind][name] = metrics
            print(f"{kind:>8} {name:>10}: success {metrics['success_rate']:.3f} spl {metrics['spl']:.3f} "
                  f"nest-zone {metrics['nest_zone_fraction']:.3f}", flush=True)
    Path(out).write_text(json.dumps(report, indent=2))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
