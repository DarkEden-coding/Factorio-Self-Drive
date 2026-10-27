"""Wrapper around fsim.VecEnv: config JSON, torch observations, final observations on done."""

from __future__ import annotations

import dataclasses
import importlib
import json
import math
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType

import numpy as np
import torch

from .config import REPO_ROOT, Config, CurriculumStage, TargetConfig
from .obs import OBS_KEYS, Obs, ObsShapes, obs_to_torch


def import_fsim() -> ModuleType:
    try:
        return importlib.import_module("fsim")
    except ImportError as exc:
        raise SystemExit(
            "Cannot import the fsim extension. Build it with `cd python && uv sync` "
            "(needs crates/fsim-py)."
        ) from exc


def policy_spec_hash(spec: dict, config: Config) -> str:
    """The hash of the observation layout the config's model reads: the teacher's `spec_hash`
    for the full observation, `spec["student"]["spec_hash"]` for students (`["map"]["spec_hash"]`
    with the student map, `["map"]["pathfinder_spec_hash"]` with pathfinder route features)."""
    if config.model.arch == "student":
        student = spec.get("student", {})
        if config.env.student_map:
            student = student.get("map", {})
            if config.env.route_source == "pathfinder":
                return str(student.get("pathfinder_spec_hash", ""))
        return str(student.get("spec_hash", ""))
    return str(spec.get("spec_hash", ""))


def env_config_json(config: Config, split: str | None = None) -> str:
    env = dataclasses.asdict(config.env)
    pool = Path(env["map_pool"])
    env["map_pool"] = str(pool if pool.is_absolute() else REPO_ROOT / pool)
    env["mode"] = config.model.kind
    if split is not None:
        env["split"] = split
    return json.dumps(env)


def executed_decisions(discount: np.ndarray, gamma: float, full: int) -> np.ndarray:
    """Decisions run in each macro step, recovered from discount = gamma^k."""
    if not 0.0 < gamma < 1.0:
        return np.full(discount.shape, full, dtype=np.int64)
    k = np.log(np.clip(discount, 1e-30, 1.0)) / math.log(gamma)
    return np.maximum(np.rint(k), 1).astype(np.int64)


@dataclass
class StepResult:
    obs: Obs                 # next observations (new episode's first obs where done)
    reward: np.ndarray       # f32 [N], discounted sum over the macro step
    discount: np.ndarray     # f32 [N], gamma^k
    terminated: np.ndarray   # bool [N]
    truncated: np.ndarray    # bool [N]
    info: dict
    done_index: torch.Tensor  # rows where terminated | truncated
    final_obs: Obs | None    # last obs of the finished episodes, rows aligned with done_index

    @property
    def done(self) -> np.ndarray:
        return self.terminated | self.truncated

    def bootstrapped_reward(self, value_fn) -> np.ndarray:
        """Reward with discount * V(final obs) added on truncated (not terminated) rows.

        The trainer then treats every done row as terminal, so truncation does not look like failure.
        """
        reward = self.reward.copy()
        if self.final_obs is None:
            return reward
        rows = self.done_index.cpu().numpy()
        cut = self.truncated[rows] & ~self.terminated[rows]
        if cut.any():
            with torch.no_grad():
                values = value_fn(self.final_obs).float().cpu().numpy()
            reward[rows[cut]] += self.discount[rows[cut]] * values[cut]
        return reward


class FsimEnv:
    def __init__(
        self,
        config: Config,
        num_envs: int,
        seed: int,
        device: torch.device,
        split: str | None = None,
        backend: ModuleType | None = None,
    ):
        self.backend = backend or import_fsim()
        self.config = config
        self.num_envs = num_envs
        self.device = device
        self.spec = self.backend.spec()
        self.env = self.backend.VecEnv(env_config_json(config, split), num_envs, seed)
        self.shapes: ObsShapes | None = None

    @property
    def spec_hash(self) -> str:
        return policy_spec_hash(self.spec, self.config)

    def reset(self) -> Obs:
        obs = self.env.reset()
        self.shapes = ObsShapes.from_obs(obs)
        return obs_to_torch(obs, self.device)

    def step(self, actions: np.ndarray) -> StepResult:
        obs, reward, discount, terminated, truncated, info = self.env.step(actions)
        terminated = np.asarray(terminated, dtype=bool).copy()
        truncated = np.asarray(truncated, dtype=bool).copy()
        info = {key: np.array(value, copy=True) for key, value in info.items()}
        done_rows = np.flatnonzero(terminated | truncated)
        final_obs = None
        finals = {key: info.pop(f"final_{key}") for key in OBS_KEYS if f"final_{key}" in info}
        if done_rows.size:
            final_obs = obs_to_torch({key: value[done_rows] for key, value in finals.items()}, self.device)
        return StepResult(
            obs=obs_to_torch(obs, self.device),
            reward=np.array(reward, dtype=np.float32, copy=True),
            discount=np.array(discount, dtype=np.float32, copy=True),
            terminated=terminated,
            truncated=truncated,
            info=info,
            done_index=torch.from_numpy(done_rows).to(self.device),
            final_obs=final_obs,
        )

    def executed(self, discount: np.ndarray) -> np.ndarray:
        full = self.config.env.direct_plan.execute if self.config.model.kind == "direct_plan" else 1
        return executed_decisions(discount, self.config.env.gamma, full)

    def set_curriculum(self, stage: CurriculumStage | TargetConfig) -> None:
        self.env.set_curriculum(
            float(stage.min_distance), float(stage.max_distance), float(stage.max_tortuosity)
        )

    def baseline_actions(self) -> np.ndarray:
        return self.env.baseline_actions()

    def set_recording(self, enabled: bool) -> None:
        self.env.set_recording(enabled)

    def episode_records(self) -> list[dict]:
        return [json.loads(r) if isinstance(r, str) else dict(r) for r in self.env.episode_records()]
