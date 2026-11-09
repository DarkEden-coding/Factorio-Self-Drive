"""Evaluation of a checkpoint or the A* baseline, with optional replay files."""

from __future__ import annotations

import json
import math
from pathlib import Path
from types import ModuleType
from typing import Callable

import numpy as np
import torch

from .checkpoint import load_policy, resolve_device
from .config import (
    SCENARIO_KINDS, Config, TargetConfig, config_from_dict, config_to_dict, with_overrides,
)
from .env import FsimEnv
from .episodes import EpisodeTracker, eval_report
from .escape import Escape
from .models import ActorCritic
from .obs import Obs

# policy(obs) -> (env actions [N, ...], per-row overlays without "decision")
PolicyFn = Callable[[Obs], tuple[np.ndarray, list[dict]]]


def model_policy(model: ActorCritic, deterministic: bool) -> PolicyFn:
    def policy(obs: Obs):
        result = model.act(obs, deterministic=deterministic)
        return model.env_action(result.action), model.overlays(result)

    return policy


def baseline_policy(env: FsimEnv, mode: str) -> PolicyFn:
    def policy(obs: Obs):
        actions = np.asarray(env.baseline_actions())
        if mode == "direct_plan":
            overlays = [{"plan_actions": row.tolist()} for row in actions]
        elif mode == "path_planner":
            overlays = [{"waypoints": row.tolist()} for row in actions]
        else:
            overlays = [{} for _ in actions]
        return actions, overlays

    return policy


def vehicle_to_world(points: list, pose: dict) -> list:
    """Vehicle frame (x right, y forward) -> world (x east, y south). Orientation 0 = north, clockwise."""
    angle = 2.0 * math.pi * pose["orientation"]
    fx, fy = math.sin(angle), -math.cos(angle)  # forward
    rx, ry = math.cos(angle), math.sin(angle)  # right
    return [[pose["x"] + x * rx + y * fx, pose["y"] + x * ry + y * fy] for x, y in points]


def _pose_from_info(info: dict, row: int) -> dict | None:
    if all(key in info for key in ("x", "y", "orientation")):
        pose = {key: float(info[key][row]) for key in ("x", "y", "orientation")}
        if "speed" in info:
            pose["speed"] = float(info["speed"][row])
        return pose
    return None


def _finish_overlays(record: dict, calls: list[dict]) -> dict:
    """Attach Python overlays to an episode record; waypoints go to world coordinates when poses are known."""
    overlays = []
    for i, call in enumerate(calls):
        overlay = dict(call)
        pose = overlay.get("pose")
        if i == 0 and isinstance(record.get("start"), dict):
            pose = {**record["start"], "speed": record.get("start_speed", 0.0)}
        if pose is not None:
            overlay["pose"] = pose
        if "waypoints" in overlay:
            if pose is not None:
                overlay["waypoints"] = vehicle_to_world(overlay["waypoints"], pose)
            else:
                overlay["waypoints_frame"] = "vehicle"
        overlays.append(overlay)
    existing = record.get("overlays")
    if isinstance(existing, list) and len(existing) == len(overlays):
        record["overlays"] = [{**rust, **py} for rust, py in zip(existing, overlays)]
    else:
        record["overlays"] = overlays
    return record


def run_episodes(
    env: FsimEnv,
    policy: PolicyFn,
    episodes: int,
    record: bool = False,
    max_steps: int = 1_000_000,
    escape: bool = False,
) -> tuple[list[dict], list[dict]]:
    """Run until `episodes` episodes finish. Each env contributes a fixed quota so short episodes
    are not over-represented. With `escape`, blocked vehicles follow `escape.Escape` (reactive
    policies only). Returns (episode summaries, replay records)."""
    n = env.num_envs
    quota = np.full(n, episodes // n)
    quota[: episodes % n] += 1
    done_count = np.zeros(n, dtype=np.int64)
    reward_terms = list(config_to_dict(env.config)["env"]["reward"])
    tracker = EpisodeTracker(n, reward_terms)
    calls: list[list[dict]] = [[] for _ in range(n)]
    decision = np.zeros(n, dtype=np.int64)
    pose: list[dict | None] = [None] * n
    results: list[dict] = []
    records: list[dict] = []
    if record:
        env.set_recording(True)
        env.episode_records()  # drop anything recorded before
    rule = Escape(n) if escape else None
    obs = env.reset()
    for _ in range(max_steps):
        if (done_count >= quota).all():
            break
        actions, overlays = policy(obs)
        if rule is not None:
            actions = rule.apply(actions)
            for i in np.flatnonzero(rule.active):
                overlays[i] = {**overlays[i], "escape": int(actions[i])}
        for i, overlay in enumerate(overlays):
            calls[i].append({"decision": int(decision[i]), "pose": pose[i], **overlay})
        step = env.step(actions)
        if rule is not None:
            rule.observe(step.info["x"], step.info["y"], step.done)
        decision += env.executed(step.discount)
        finished = tracker.step(step.reward, step.info, step.done)
        new_records = env.episode_records() if record and finished else []
        for k, episode in enumerate(finished):
            i = episode["env"]
            if done_count[i] < quota[i]:
                results.append(episode)
                if k < len(new_records):
                    records.append(_finish_overlays(new_records[k], calls[i]))
            done_count[i] += 1
            calls[i] = []
            decision[i] = 0
        for i in range(n):
            pose[i] = None if step.done[i] else _pose_from_info(step.info, i)
        obs = step.obs
    if record:
        env.set_recording(False)
    return results, records


def vehicle_names(spec: dict, config: Config) -> list[str]:
    vehicles = spec.get("vehicles")
    if isinstance(vehicles, list) and vehicles:
        return [v["name"] if isinstance(v, dict) else str(v) for v in vehicles]
    return [f"{m.vehicle}/{m.fuel}" for m in config.env.vehicle_mix]


def scenario_names(spec: dict) -> list[str]:
    names = spec.get("scenarios")
    return list(names) if isinstance(names, list) and names else list(SCENARIO_KINDS)


def with_scenario(config: Config, kind: str) -> Config:
    """A copy of `config` whose episodes are all of one scenario kind, with the parameters the
    config gives that kind (Rust defaults otherwise)."""
    if kind not in SCENARIO_KINDS:
        raise SystemExit(f"unknown scenario {kind!r}; choose from {', '.join(SCENARIO_KINDS)}")
    data = config_to_dict(config)
    entry = next((s for s in data["env"]["scenarios"] if s.get("kind") == kind), {"kind": kind})
    data["env"]["scenarios"] = [{**entry, "weight": 1.0}]
    return config_from_dict(data)


def evaluate(
    config: Config,
    split: str,
    episodes: int,
    num_envs: int,
    seed: int,
    device: torch.device,
    model: ActorCritic | None = None,
    deterministic: bool = True,
    target: TargetConfig | None = None,
    replays: Path | None = None,
    spec_hash: str | None = None,
    backend: ModuleType | None = None,
    scenario: str | None = None,
    escape: bool = True,
) -> dict:
    """Evaluate `model`, or the A* baseline when model is None. With `scenario`, every episode is
    of that kind; otherwise the config's scenario mix applies. `escape` applies the mod's
    escape rule to a deterministic reactive model, as the mod does."""
    if scenario is not None:
        config = with_scenario(config, scenario)
    env = FsimEnv(config, min(num_envs, episodes), seed, device, split=split, backend=backend)
    if spec_hash is not None and spec_hash != env.spec_hash:
        raise SystemExit(f"checkpoint spec_hash {spec_hash} != env spec_hash {env.spec_hash}")
    env.set_curriculum(target or config.env.target)
    if model is None:
        policy = baseline_policy(env, config.model.kind)
    else:
        policy = model_policy(model, deterministic)
    escape = (escape and model is not None and deterministic and config.model.kind == "reactive"
              and config.env.steer_levels == 1)
    results, records = run_episodes(env, policy, episodes, record=replays is not None, escape=escape)
    report = eval_report(results, vehicle_names(env.spec, config), scenario_names(env.spec))
    report["split"] = split
    if scenario is not None:
        report["scenario"] = scenario
    report["policy"] = "baseline" if model is None else config.model.kind
    report["escape"] = escape
    if replays is not None:
        replays.parent.mkdir(parents=True, exist_ok=True)
        replays.write_text(json.dumps(records))
        report["replays"] = str(replays)
    return report


def evaluate_checkpoint(
    checkpoint: Path,
    split: str,
    episodes: int,
    num_envs: int = 16,
    seed: int = 12345,
    device: str = "auto",
    deterministic: bool = True,
    target: TargetConfig | None = None,
    replays: Path | None = None,
    backend: ModuleType | None = None,
    scenario: str | None = None,
    overrides: dict | None = None,
    escape: bool = True,
) -> dict:
    """`overrides` change the checkpoint's config for this evaluation (env settings only make
    sense here, e.g. `{"env": {"distance_field": {"nest_zone_cost": 0}}}`)."""
    torch_device = resolve_device(device)
    model, config, state = load_policy(checkpoint, torch_device)
    config = with_overrides(config, overrides)
    report = evaluate(
        config, split, episodes, num_envs, seed, torch_device,
        model=model, deterministic=deterministic, target=target, replays=replays,
        spec_hash=state["spec_hash"], backend=backend, scenario=scenario, escape=escape,
    )
    report["checkpoint"] = str(checkpoint)
    report["global_step"] = state["global_step"]
    return report


def suite(
    config: Config,
    split: str,
    episodes: int,
    num_envs: int,
    seed: int,
    device: torch.device,
    model: ActorCritic | None = None,
    baseline: bool = True,
    spec_hash: str | None = None,
    kinds: list[str] | None = None,
    backend: ModuleType | None = None,
    escape: bool = True,
) -> dict:
    """Evaluate every scenario kind with the model and/or the A* baseline. Both see the same
    episodes: each env's episode sequence depends only on the seed."""
    policies = ([("model", model)] if model is not None else []) + ([("baseline", None)] if baseline else [])
    if not policies:
        raise SystemExit("suite needs a checkpoint, --baseline, or both")
    rows: dict[str, dict] = {}
    for kind in kinds or list(SCENARIO_KINDS):
        rows[kind] = {}
        for name, policy_model in policies:
            report = evaluate(
                config, split, episodes, num_envs, seed, device, model=policy_model,
                spec_hash=spec_hash, backend=backend, scenario=kind, escape=escape,
            )
            rows[kind][name] = report
            print(f"{kind:>8} {name:>8}: success {report['success_rate']:.3f} spl {report['spl']:.3f}",
                  flush=True)
    return {"split": split, "episodes_per_scenario": episodes, "seed": seed, "escape": escape, "rows": rows}


SUITE_COLUMNS = (
    ("success", lambda m: m["success_rate"], "{:.3f}"),
    ("SPL", lambda m: m["spl"], "{:.3f}"),
    *(
        (f"{kind} coll/ep", lambda m, kind=kind: m["collisions_per_episode"][kind], "{:.2f}")
        for kind in ("tree", "rock", "cliff", "nest", "water")
    ),
    ("tree-hit eps", lambda m: m["tree_hit_rate"], "{:.3f}"),
    ("nest-zone time", lambda m: m["nest_zone_fraction"], "{:.3f}"),
    ("reversed eps", lambda m: m["reversed_rate"], "{:.3f}"),
    ("damage", lambda m: m["mean_damage"], "{:.1f}"),
    ("end km/h", lambda m: m.get("mean_end_speed_kmh"), "{:.1f}"),
    ("end dist", lambda m: m.get("mean_end_target_distance"), "{:.2f}"),
    ("time s", lambda m: m["mean_time_s"], "{:.1f}"),
)


def suite_table(report: dict) -> str:
    """Markdown table: one row per scenario and policy."""
    header = ["scenario", "policy", *(name for name, _, _ in SUITE_COLUMNS)]
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * 2 + "---:|" * len(SUITE_COLUMNS)]
    for kind, policies in report["rows"].items():
        for name, metrics in policies.items():
            cells = []
            for _, value, fmt in SUITE_COLUMNS:
                v = value(metrics)
                cells.append("-" if v is None or (isinstance(v, float) and math.isnan(v)) else fmt.format(v))
            lines.append(f"| {kind} | {name} | " + " | ".join(cells) + " |")
    return "\n".join(lines)
