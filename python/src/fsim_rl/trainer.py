"""PPO training loop: rollouts, GAE, updates, curriculum, logging, checkpoints."""

from __future__ import annotations

import json
import time
from pathlib import Path
from types import ModuleType

import numpy as np
import torch

from .checkpoint import (
    checkpoint_state, load_checkpoint, load_policy, resolve_device, save_checkpoint,
)
from .config import PYTHON_DIR, Config, config_to_dict, with_overrides
from .curriculum import Curriculum
from .env import FsimEnv
from .episodes import EpisodeTracker, training_summary
from .evaluate import scenario_names
from .logger import RunLogger
from .models import build_model
from .models.reactive import NUM_ACTIONS
from .models.direct_plan import plan_consistency_targets
from .ppo import ppo_update
from .rollout import RolloutBuffer


class Trainer:
    def __init__(
        self,
        config: Config,
        run_dir: Path,
        resume: Path | None = None,
        backend: ModuleType | None = None,
        init_from: Path | None = None,
    ):
        if resume is not None and init_from is not None:
            raise SystemExit("use either resume or init_from, not both")
        self.config = config
        self.run_dir = run_dir
        self.device = resolve_device(config.device)
        ppo = config.ppo
        torch.manual_seed(config.seed)
        np.random.seed(config.seed)

        # Optional KL-to-teacher regularizer: the env also returns the teacher's observation.
        self.teacher = None
        env_config = config
        if ppo.teacher_kl_coef > 0:
            if not ppo.teacher_checkpoint:
                raise SystemExit("ppo.teacher_kl_coef needs ppo.teacher_checkpoint")
            teacher = Path(ppo.teacher_checkpoint)
            teacher = teacher if teacher.is_absolute() else PYTHON_DIR / teacher
            self.teacher = load_policy(teacher, self.device)[0]
            env_config = with_overrides(config, {"env": {"observation": "both"}})
        self.env = FsimEnv(env_config, ppo.num_envs, config.seed, self.device, backend=backend)
        self.obs = self.env.reset()
        self.model = build_model(config, self.env.shapes).to(self.device)
        self.optimizer = torch.optim.Adam(self.model.parameters(), lr=ppo.learning_rate, eps=1e-5)
        self.curriculum = Curriculum(config.curriculum, config.env.target)
        self.global_step = 0
        self.iteration = 0
        if resume is not None:
            self._resume(resume)
        if init_from is not None:
            self._init_from(init_from, run_dir)
        if self.curriculum.active:
            self.env.set_curriculum(self.curriculum.stage)

        self.buffer = RolloutBuffer(
            ppo.rollout_steps,
            ppo.num_envs,
            self.env.shapes,
            self.model.action_shape,
            self.model.action_dtype,
            self.device,
        )
        self.teacher_log_probs = (
            torch.zeros(ppo.rollout_steps, ppo.num_envs, NUM_ACTIONS, device=self.device)
            if self.teacher is not None else None
        )
        self.reward_terms = list(config_to_dict(config)["env"]["reward"])
        self.scenario_names = scenario_names(self.env.spec)
        self.tracker = EpisodeTracker(ppo.num_envs, self.reward_terms)
        self.generator = torch.Generator().manual_seed(config.seed)
        run_dir.mkdir(parents=True, exist_ok=True)
        (run_dir / "config.json").write_text(json.dumps(config_to_dict(config), indent=2))
        self.logger = RunLogger(run_dir)

    @property
    def checkpoint_dir(self) -> Path:
        return self.run_dir / "checkpoints"

    @property
    def total_iterations(self) -> int:
        ppo = self.config.ppo
        return max(ppo.total_steps // (ppo.num_envs * ppo.rollout_steps), 1)

    def _compatible_state(self, path: Path) -> dict:
        """Load a checkpoint and refuse it if its model kind or spec_hash differs."""
        state = load_checkpoint(path)
        if state["config"]["model"]["kind"] != self.config.model.kind:
            raise SystemExit(f"checkpoint model kind {state['config']['model']['kind']!r} != config")
        if state["spec_hash"] != self.env.spec_hash:
            raise SystemExit(
                f"checkpoint spec_hash {state['spec_hash']} != env spec_hash {self.env.spec_hash}"
            )
        try:
            self.model.load_state_dict(state["model"])
        except RuntimeError as exc:
            raise SystemExit(f"checkpoint weights do not fit config.model: {exc}") from exc
        return state

    def _init_from(self, path: Path, run_dir: Path) -> None:
        """Start a new run from a checkpoint's weights and vector normalizer, with a fresh
        optimizer, curriculum and step count."""
        state = self._compatible_state(path)
        run_dir.mkdir(parents=True, exist_ok=True)
        source = {
            "checkpoint": str(Path(path).resolve()),
            "global_step": state["global_step"],
            "iteration": state["iteration"],
            "spec_hash": state["spec_hash"],
        }
        (run_dir / "init_from.json").write_text(json.dumps(source, indent=2))
        print(f"initialized from {path} (iteration {state['iteration']}, step {state['global_step']})")

    def _resume(self, path: Path) -> None:
        state = self._compatible_state(path)
        self.optimizer.load_state_dict(state["optimizer"])
        self.curriculum.load_state_dict(state["curriculum"])
        self.global_step = state["global_step"]
        self.iteration = state["iteration"]
        print(f"resumed {path} at iteration {self.iteration}, step {self.global_step}")

    def save(self) -> Path:
        state = checkpoint_state(
            self.model,
            self.optimizer,
            self.config,
            self.env.spec,
            self.curriculum.state_dict(),
            self.global_step,
            self.iteration,
        )
        latest = self.checkpoint_dir / "latest.pt"
        save_checkpoint(latest, state)
        if self.iteration % max(self.config.checkpoint_interval, 1) == 0:
            save_checkpoint(self.checkpoint_dir / f"iter_{self.iteration:06d}.pt", state)
        return latest

    def collect(self) -> list[dict]:
        """Fill the buffer with one rollout. Returns the episodes that finished."""
        model, env, buffer = self.model, self.env, self.buffer
        model.eval()
        episodes: list[dict] = []
        for t in range(buffer.steps):
            result = model.act(self.obs)
            buffer.store_policy(t, self.obs, result.action, result.log_prob, result.value)
            if self.teacher is not None:
                with torch.no_grad():
                    self.teacher_log_probs[t] = self.teacher(self.obs)[0].dist.logits
            step = env.step(model.env_action(result.action))
            reward = step.bootstrapped_reward(model.value)
            buffer.store_step(
                t,
                torch.from_numpy(reward).to(self.device),
                torch.from_numpy(step.discount).to(self.device),
                torch.from_numpy(step.done).to(self.device),
                torch.from_numpy(env.executed(step.discount)).to(self.device),
            )
            finished = self.tracker.step(step.reward, step.info, step.done)
            if finished:
                episodes += finished
                # Only standard episodes follow the curriculum range, so only they count.
                standard = (e.get("success", False) for e in finished if e.get("scenario", 0) == 0)
                if self.curriculum.record(standard):
                    env.set_curriculum(self.curriculum.stage)
                    print(f"curriculum -> stage {self.curriculum.stage_index}: {self.curriculum.stage}")
            self.obs = step.obs
        with torch.no_grad():
            last_value = model.value(self.obs)
        buffer.finish(last_value, self.config.ppo.gae_lambda)
        return episodes

    def update(self) -> dict[str, float]:
        extra = {}
        if self.config.model.kind == "direct_plan":
            targets, mask = plan_consistency_targets(
                self.buffer.actions,
                self.buffer.dones,
                self.buffer.executed,
                self.config.env.direct_plan.execute,
            )
            extra = {"plan_targets": targets, "plan_mask": mask}
        if self.teacher_log_probs is not None:
            extra["teacher_log_probs"] = self.teacher_log_probs
        batch = self.buffer.flatten(extra)
        self.model.train()
        stats = ppo_update(self.model, self.optimizer, batch, self.config.ppo, self.generator)
        # Normalizer stats change only between iterations so each update sees fixed inputs.
        self.model.update_normalizer(batch["obs"])
        return stats

    def set_learning_rate(self) -> float:
        ppo = self.config.ppo
        lr = ppo.learning_rate
        if ppo.lr_schedule == "linear":
            lr *= max(1.0 - self.iteration / self.total_iterations, 0.0)
        for group in self.optimizer.param_groups:
            group["lr"] = lr
        return lr

    def train(self, iterations: int | None = None) -> Path:
        ppo = self.config.ppo
        end = self.total_iterations if iterations is None else self.iteration + iterations
        latest = self.checkpoint_dir / "latest.pt"
        try:
            while self.iteration < end:
                started = time.perf_counter()
                lr = self.set_learning_rate()
                episodes = self.collect()
                collected = time.perf_counter()
                stats = self.update()
                self.iteration += 1
                self.global_step += ppo.num_envs * ppo.rollout_steps
                elapsed = time.perf_counter() - started
                decisions = int(self.buffer.executed.sum().item())
                metrics = {
                    **stats,
                    **training_summary(episodes, self.reward_terms, self.scenario_names),
                    "train/iteration": float(self.iteration),
                    "train/learning_rate": lr,
                    "train/decisions": float(decisions),
                    "perf/steps_per_sec": ppo.num_envs * ppo.rollout_steps / elapsed,
                    "perf/decisions_per_sec": decisions / elapsed,
                    "perf/collect_fraction": (collected - started) / elapsed,
                    "curriculum/stage": float(self.curriculum.stage_index),
                    "curriculum/success_rate": self.curriculum.success_rate(),
                }
                self.logger.log(self.global_step, metrics)
                print(
                    f"iter {self.iteration}/{end} step {self.global_step} "
                    f"sps {metrics['perf/steps_per_sec']:.0f} "
                    f"episodes {len(episodes)} success {metrics['episode/success_rate']:.2f} "
                    f"return {metrics['episode/return']:.2f} loss {stats['loss/total']:.3f} "
                    f"stage {self.curriculum.stage_index}",
                    flush=True,
                )
                if self.iteration % max(self.config.checkpoint_interval, 1) == 0:
                    latest = self.save()
        finally:
            latest = self.save()
            self.logger.close()
        return latest
