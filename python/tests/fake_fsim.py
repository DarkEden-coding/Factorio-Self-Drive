"""Stand-in for the `fsim` extension that follows the docs/rl.md interface with random dynamics.

Lets the trainer, evaluation and replay code run without the Rust build. It also checks that the
actions it receives have the shape and dtype the contract requires for the configured mode.
"""

from __future__ import annotations

import json

import numpy as np

C, LOCAL, COARSE = 8, 64, 64 // 2
VECTOR_FEATURES = [f"f{i}" for i in range(12)]
REWARD_TERMS = [
    "progress", "time", "damage", "collision", "nest_zone", "success", "destroyed", "arrival",
    "target_speed", "path_length", "steer_change", "heading_error",
]
SCENARIOS = ["standard", "long", "forest", "reverse", "nest", "arrival", "impact", "recover"]
COLLISION_KINDS = ["tree", "rock", "cliff", "nest", "water"]


def spec() -> dict:
    return {
        "spec_hash": "fake-spec",
        "channels": ["blocked", "cliff", "tree", "rock", "water", "nest_zone", "friction", "target"],
        "vector_features": VECTOR_FEATURES,
        "local_size": LOCAL,
        "coarse_size": COARSE,
        "scenarios": SCENARIOS,
    }


class VecEnv:
    def __init__(self, config_json: str, num_envs: int, seed: int):
        self.config = json.loads(config_json)
        self.mode = self.config["mode"]
        self.map_names = [(0, "fixture")]
        self.gamma = self.config["gamma"]
        self.n = num_envs
        self.rng = np.random.default_rng(seed)
        self.recording = False
        self.records: list[dict] = []
        self.curriculum = None
        self.decisions = np.zeros(num_envs, dtype=np.int64)
        self.actions: list[list] = [[] for _ in range(num_envs)]
        kinds = [SCENARIOS.index(s["kind"]) for s in self.config["scenarios"]]
        weights = np.array([s.get("weight", 1.0) for s in self.config["scenarios"]], dtype=float)
        self.kinds, self.kind_p = kinds, weights / weights.sum()
        self.scenario = self.rng.choice(kinds, size=num_envs, p=self.kind_p)

    def _obs(self) -> dict:
        return {
            "local": self.rng.integers(0, 2, (self.n, C, LOCAL, LOCAL), dtype=np.uint8) * 255,
            "coarse": self.rng.integers(0, 256, (self.n, C, COARSE, COARSE), dtype=np.uint8),
            "vector": self.rng.normal(size=(self.n, len(VECTOR_FEATURES))).astype(np.float32) * 5,
        }

    def reset(self) -> dict:
        return self._obs()

    def _check_actions(self, actions: np.ndarray) -> None:
        if self.mode == "reactive":
            assert actions.shape == (self.n,) and actions.dtype == np.int64
        elif self.mode == "direct_plan":
            assert actions.shape == (self.n, self.config["direct_plan"]["horizon"])
            assert actions.dtype == np.int64
        else:
            assert actions.shape == (self.n, self.config["path_planner"]["waypoints"], 2)
            assert actions.dtype == np.float32
        if self.mode != "path_planner":
            assert actions.min() >= 0 and actions.max() <= 8

    def step(self, actions: np.ndarray):
        actions = np.asarray(actions)
        self._check_actions(actions)
        full = self.config["direct_plan"]["execute"] if self.mode == "direct_plan" else 1
        terminated = self.rng.random(self.n) < 0.08
        truncated = ~terminated & (self.rng.random(self.n) < 0.04)
        done = terminated | truncated
        executed = np.where(done, self.rng.integers(1, full + 1, self.n), full)
        discount = (self.gamma ** executed).astype(np.float32)
        reward = self.rng.normal(size=self.n).astype(np.float32) * 0.1
        success = terminated & (self.rng.random(self.n) < 0.5)
        info = {
            "success": success,
            "destroyed": terminated & ~success,
            "timeout": truncated,
            "stalled": np.zeros(self.n, dtype=bool),
            "path_distance": self.rng.uniform(0, 30, self.n).astype(np.float32),
            "optimal_distance": np.full(self.n, 20.0, dtype=np.float32),
            "ticks": (self.decisions + executed) * self.config["decision_ticks"],
            "damage": np.zeros(self.n, dtype=np.float32),
            "collisions": np.zeros(self.n, dtype=np.int64),
            "map_index": np.zeros(self.n, dtype=np.int64),
            "vehicle_index": np.zeros(self.n, dtype=np.int64),
            "optimal_length": np.full(self.n, 19.0, dtype=np.float32),
            "nest_zone_ticks": self.rng.integers(0, 4, self.n),
            "nest_zone_decisions": np.zeros(self.n, dtype=np.int64),
            "reverse_distance": np.zeros(self.n, dtype=np.float32),
            "reversed": self.rng.random(self.n) < 0.3,
            "scenario": self.scenario.astype(np.int64),
            "speed": self.rng.uniform(0, 0.8, self.n),
            "x": self.rng.uniform(0, 30, self.n),
            "y": self.rng.uniform(0, 30, self.n),
            "orientation": self.rng.random(self.n),
        }
        for kind in COLLISION_KINDS:
            info[f"collisions_{kind}"] = self.rng.integers(0, 2, self.n)
        for term in REWARD_TERMS:
            info[f"reward_{term}"] = reward / len(REWARD_TERMS)
        final = self._obs()
        for key in ("local", "coarse", "vector"):
            info[f"final_{key}"] = final[key]
        self.decisions += executed
        for i in range(self.n):
            self.actions[i].append(actions[i].tolist())
            if done[i]:
                if self.recording:
                    self.records.append({
                        "map": "fake", "vehicle": "car", "fuel": "coal",
                        "start": {"x": 0.0, "y": 0.0, "orientation": 0.25},
                        "mode": self.mode, "actions": self.actions[i],
                    })
                self.actions[i] = []
                self.decisions[i] = 0
                self.scenario[i] = self.rng.choice(self.kinds, p=self.kind_p)
        return self._obs(), reward, discount, terminated, truncated, info

    def set_curriculum(self, min_distance: float, max_distance: float, max_tortuosity: float) -> None:
        self.curriculum = (min_distance, max_distance, max_tortuosity)

    def baseline_actions(self) -> np.ndarray:
        if self.mode == "reactive":
            return np.full(self.n, 7, dtype=np.int64)
        if self.mode == "direct_plan":
            return np.full((self.n, self.config["direct_plan"]["horizon"]), 7, dtype=np.int64)
        k = self.config["path_planner"]["waypoints"]
        points = np.stack([np.zeros(k), np.arange(1, k + 1, dtype=np.float64)], -1)
        return np.broadcast_to(points, (self.n, k, 2)).astype(np.float32)

    def episode_records(self) -> list[dict]:
        records, self.records = self.records, []
        return records

    def set_recording(self, enabled: bool) -> None:
        self.recording = enabled
