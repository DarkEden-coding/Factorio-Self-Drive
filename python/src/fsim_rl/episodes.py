"""Per-episode bookkeeping from env info, and summaries for training logs and evaluation."""

from __future__ import annotations

import math

import numpy as np

from .config import SCENARIO_KINDS

OUTCOMES = ("success", "destroyed", "timeout", "stalled")
COLLISION_KINDS = ("tree", "rock", "cliff", "nest", "water")
END_FIELDS = (
    "path_distance", "optimal_distance", "optimal_length", "ticks", "damage", "collisions",
    *(f"collisions_{kind}" for kind in COLLISION_KINDS),
    "nest_zone_ticks", "nest_zone_decisions", "reverse_distance",
    "speed", "target_distance",  # at the episode's end; speed in tiles/tick
)
KMH_PER_TILE_TICK = 216.0


class EpisodeTracker:
    """Accumulates returns and reward terms per env; emits a dict per finished episode.

    `reward_<term>` info values are treated as per-step amounts and summed here. Info values on a
    done row are taken to describe the finished episode.
    """

    def __init__(self, num_envs: int, reward_terms: list[str]):
        self.reward_terms = reward_terms
        self.returns = np.zeros(num_envs, dtype=np.float64)
        self.lengths = np.zeros(num_envs, dtype=np.int64)
        self.terms = {term: np.zeros(num_envs, dtype=np.float64) for term in reward_terms}

    def step(self, reward: np.ndarray, info: dict, done: np.ndarray) -> list[dict]:
        self.returns += reward
        self.lengths += 1
        for term, total in self.terms.items():
            key = f"reward_{term}"
            if key in info:
                total += info[key]
        finished = []
        for i in np.flatnonzero(done):
            episode = {"env": int(i), "return": float(self.returns[i]), "length": int(self.lengths[i])}
            for key in (*OUTCOMES, *END_FIELDS, "map_index", "vehicle_index", "path_length",
                        "scenario", "reversed"):
                if key in info:
                    episode[key] = info[key][i].item()
            for term, total in self.terms.items():
                episode[f"reward_{term}"] = float(total[i])
                total[i] = 0.0
            finished.append(episode)
            self.returns[i] = 0.0
            self.lengths[i] = 0
        return finished


def _mean(values) -> float:
    values = [float(v) for v in values]
    return sum(values) / len(values) if values else math.nan


def scenario_name(episode: dict, names: list[str] | tuple[str, ...] = SCENARIO_KINDS) -> str:
    index = int(episode.get("scenario", 0))
    return names[index] if index < len(names) else f"scenario_{index}"


def nest_zone_fraction(episode: dict) -> float:
    ticks = episode.get("ticks")
    if not ticks or "nest_zone_ticks" not in episode:
        return math.nan
    return episode["nest_zone_ticks"] / ticks


def training_summary(
    episodes: list[dict],
    reward_terms: list[str],
    scenario_names: list[str] | tuple[str, ...] = SCENARIO_KINDS,
) -> dict[str, float]:
    """Fixed set of keys (NaN when no episode finished) so CSV columns stay stable."""
    out = {"episode/count": float(len(episodes))}
    out["episode/return"] = _mean(e["return"] for e in episodes)
    out["episode/length"] = _mean(e["length"] for e in episodes)
    for key in OUTCOMES:
        out[f"episode/{key}_rate"] = _mean(bool(e.get(key, False)) for e in episodes)
    for key in END_FIELDS:
        out[f"episode/{key}"] = _mean(e[key] for e in episodes if key in e)
    out["episode/spl"] = _mean(spl(e) for e in episodes)
    out["episode/nest_zone_fraction"] = _mean(nest_zone_fraction(e) for e in episodes)
    out["episode/reversed_rate"] = _mean(bool(e["reversed"]) for e in episodes if "reversed" in e)
    for term in reward_terms:
        out[f"reward/{term}"] = _mean(e[f"reward_{term}"] for e in episodes)
    for name in scenario_names:
        group = [e for e in episodes if scenario_name(e, scenario_names) == name]
        out[f"scenario/{name}/count"] = float(len(group))
        out[f"scenario/{name}/success_rate"] = _mean(bool(e.get("success")) for e in group)
        out[f"scenario/{name}/length"] = _mean(e["length"] for e in group)
    return out


def spl(episode: dict) -> float:
    """Success weighted by path length: success * optimal / max(actual length, optimal).

    The optimal length is the field route's geometric length (`optimal_length`); the field value
    `optimal_distance` includes tree and nest-zone costs, so it is only a fallback.
    """
    if not episode.get("success"):
        return 0.0
    length = episode.get("path_length")
    optimal = episode.get("optimal_length", episode.get("optimal_distance"))
    if length is None or optimal is None:
        return math.nan
    return optimal / max(length, optimal, 1e-6)


def eval_metrics(episodes: list[dict]) -> dict:
    successes = [e for e in episodes if e.get("success")]
    return {
        "episodes": len(episodes),
        "success_rate": _mean(bool(e.get("success")) for e in episodes),
        "spl": _mean(spl(e) for e in episodes),
        "mean_time_s": _mean(e["ticks"] / 60.0 for e in successes if "ticks" in e),
        "mean_damage": _mean(e["damage"] for e in episodes if "damage" in e),
        "destroyed_rate": _mean(bool(e.get("destroyed")) for e in episodes),
        "timeout_rate": _mean(bool(e.get("timeout")) for e in episodes),
        "stalled_rate": _mean(bool(e.get("stalled")) for e in episodes),
        "mean_return": _mean(e["return"] for e in episodes),
        # Collision ticks per episode by kind, and the share of episodes touching a tree.
        "collisions_per_episode": {
            kind: _mean(e[f"collisions_{kind}"] for e in episodes if f"collisions_{kind}" in e)
            for kind in COLLISION_KINDS
        },
        "tree_hit_rate": _mean(
            e["collisions_tree"] > 0 for e in episodes if "collisions_tree" in e
        ),
        "nest_zone_fraction": _mean(nest_zone_fraction(e) for e in episodes),
        "reversed_rate": _mean(bool(e["reversed"]) for e in episodes if "reversed" in e),
        # Where and how fast episodes end, against the target point.
        "mean_end_speed_kmh": _mean(KMH_PER_TILE_TICK * e["speed"] for e in episodes if "speed" in e),
        "mean_end_target_distance": _mean(
            e["target_distance"] for e in episodes if "target_distance" in e
        ),
    }


def _grouped(episodes: list[dict], key) -> dict[str, dict]:
    groups: dict[str, list[dict]] = {}
    for e in episodes:
        groups.setdefault(key(e), []).append(e)
    return {name: eval_metrics(group) for name, group in sorted(groups.items())}


def eval_report(
    episodes: list[dict],
    vehicle_names: list[str],
    scenario_names: list[str] | tuple[str, ...] = SCENARIO_KINDS,
) -> dict:
    report = eval_metrics(episodes)

    def vehicle(e: dict) -> str:
        index = int(e.get("vehicle_index", 0))
        return vehicle_names[index] if index < len(vehicle_names) else f"vehicle_{index}"

    report["per_vehicle"] = _grouped(episodes, vehicle)
    report["per_scenario"] = _grouped(episodes, lambda e: scenario_name(e, scenario_names))
    return report
