"""DAgger distillation of a student policy from a teacher checkpoint (`fsim-rl distill`).

Each iteration rolls out the env with the teacher's or the student's action per step (the
teacher's with probability beta, falling to 0), labels every visited state with the teacher's
action log-probs and value, adds the samples to an aggregated buffer, and fits the student on
random minibatches from it: KL(teacher || student) plus value regression to the teacher's value.
Before training on a rollout, the KL on it ("fresh KL") measures how well the student matches the
teacher on the states it now visits; the run stops when that stops improving.
"""

from __future__ import annotations

import json
import math
import time
from pathlib import Path
from types import ModuleType

import torch
from torch import nn

from .checkpoint import checkpoint_state, load_policy, resolve_device, save_checkpoint
from .config import Config, config_to_dict, with_overrides
from .curriculum import Curriculum
from .env import FsimEnv
from .episodes import EpisodeTracker, training_summary
from .evaluate import scenario_names
from .logger import RunLogger
from .models import build_model
from .models.distributions import categorical_kl
from .models.reactive import NUM_ACTIONS


class DistillBuffer:
    """Ring buffer of (student obs, teacher log-probs, teacher value) on the training device."""

    def __init__(self, capacity: int, obs_size: int, device: torch.device):
        self.obs = torch.zeros(capacity, obs_size, device=device)
        self.log_probs = torch.zeros(capacity, NUM_ACTIONS, device=device)
        self.values = torch.zeros(capacity, device=device)
        self.capacity = capacity
        self.size = 0
        self.pos = 0

    def add(self, obs: torch.Tensor, log_probs: torch.Tensor, values: torch.Tensor) -> None:
        n = obs.shape[0]
        index = (torch.arange(n, device=obs.device) + self.pos) % self.capacity
        self.obs[index] = obs
        self.log_probs[index] = log_probs
        self.values[index] = values
        self.pos = (self.pos + n) % self.capacity
        self.size = min(self.size + n, self.capacity)

    def sample(self, n: int, generator: torch.Generator) -> tuple[dict, torch.Tensor, torch.Tensor]:
        index = torch.randint(0, self.size, (n,), generator=generator).to(self.obs.device)
        return {"student": self.obs[index]}, self.log_probs[index], self.values[index]


def distill_loss(model, obs: dict, teacher_log_probs, teacher_values, value_coef: float):
    """KL(teacher || student) + value_coef * squared error to the teacher's value."""
    dist, value = model(obs)
    kl = categorical_kl(teacher_log_probs, dist.dist.logits).mean()
    value_loss = (value - teacher_values).square().mean()
    return kl + value_coef * value_loss, kl, value_loss


class Distiller:
    def __init__(
        self,
        config: Config,
        teacher_path: Path,
        run_dir: Path,
        backend: ModuleType | None = None,
    ):
        if config.model.arch != "student":
            raise SystemExit("distill needs a student config (model.arch = student)")
        self.config = config
        self.run_dir = run_dir
        self.device = resolve_device(config.device)
        d = config.distill
        torch.manual_seed(config.seed)
        self.teacher, _, teacher_state = load_policy(teacher_path, self.device)
        self.teacher.eval()
        env_config = with_overrides(config, {"env": {"observation": "both"}})
        self.env = FsimEnv(env_config, d.num_envs, config.seed, self.device, backend=backend)
        if teacher_state["spec_hash"] != self.env.spec["spec_hash"]:
            raise SystemExit(
                f"teacher spec_hash {teacher_state['spec_hash']} != env {self.env.spec['spec_hash']}"
            )
        self.obs = self.env.reset()
        self.model = build_model(config, self.env.shapes).to(self.device)
        self.model.update_normalizer(self.obs)
        self.optimizer = torch.optim.Adam(self.model.parameters(), lr=d.learning_rate)
        # Standard episodes use the stage the teacher's curriculum reached; it doesn't advance.
        self.curriculum = Curriculum(config.curriculum, config.env.target)
        if self.curriculum.active and "curriculum" in teacher_state:
            self.curriculum.load_state_dict(teacher_state["curriculum"])
        self.env.set_curriculum(self.curriculum.stage)
        self.buffer = DistillBuffer(d.buffer_size, self.model.shapes.student, self.device)
        self.generator = torch.Generator().manual_seed(config.seed)
        self.reward_terms = list(config_to_dict(config)["env"]["reward"])
        self.scenario_names = scenario_names(self.env.spec)
        self.tracker = EpisodeTracker(d.num_envs, self.reward_terms)
        self.iteration = 0
        self.global_step = 0
        run_dir.mkdir(parents=True, exist_ok=True)
        (run_dir / "config.json").write_text(json.dumps(config_to_dict(config), indent=2))
        source = {"teacher": str(Path(teacher_path).resolve()), "teacher_spec_hash": teacher_state["spec_hash"]}
        (run_dir / "teacher.json").write_text(json.dumps(source, indent=2))
        self.logger = RunLogger(run_dir)

    def beta(self) -> float:
        d = self.config.distill
        if d.beta_iterations <= 0:
            return 0.0
        return max(d.beta_start * (1.0 - self.iteration / d.beta_iterations), 0.0)

    def learning_rate(self) -> float:
        d = self.config.distill
        progress = min(self.iteration / max(d.iterations, 1), 1.0)
        return d.final_learning_rate + 0.5 * (d.learning_rate - d.final_learning_rate) * (
            1.0 + math.cos(math.pi * progress)
        )

    @torch.no_grad()
    def collect(self, beta: float) -> tuple[list[dict], dict[str, float]]:
        """One rollout of mixed actions; every state goes to the buffer with teacher labels."""
        d = self.config.distill
        self.model.eval()
        episodes: list[dict] = []
        fresh_kl = agree = 0.0
        new_obs = []
        for _ in range(d.rollout_steps):
            teacher_dist, teacher_value = self.teacher(self.obs)
            student_dist, _ = self.model(self.obs)
            teacher_action = teacher_dist.mode()
            student_mode = student_dist.mode()
            student_action = student_dist.sample() if d.student_sample else student_mode
            use_teacher = torch.rand(d.num_envs, device=self.device) < beta
            action = torch.where(use_teacher, teacher_action, student_action)
            teacher_log_probs = teacher_dist.dist.logits
            fresh_kl += categorical_kl(teacher_log_probs, student_dist.dist.logits).sum().item()
            agree += (student_mode == teacher_action).float().sum().item()
            self.buffer.add(self.obs["student"], teacher_log_probs, teacher_value.float())
            new_obs.append(self.obs["student"])
            step = self.env.step(action.cpu().numpy())
            episodes += self.tracker.step(step.reward, step.info, step.done)
            self.obs = step.obs
        samples = d.rollout_steps * d.num_envs
        self.model.update_normalizer({"student": torch.cat(new_obs)})
        return episodes, {"distill/fresh_kl": fresh_kl / samples, "distill/agreement": agree / samples}

    def train_step(self) -> dict[str, float]:
        d = self.config.distill
        self.model.train()
        sums = {"distill/loss": 0.0, "distill/kl": 0.0, "distill/value_loss": 0.0}
        for _ in range(d.updates_per_iteration):
            obs, log_probs, values = self.buffer.sample(d.minibatch, self.generator)
            loss, kl, value_loss = distill_loss(self.model, obs, log_probs, values, d.value_coef)
            self.optimizer.zero_grad(set_to_none=True)
            loss.backward()
            nn.utils.clip_grad_norm_(self.model.parameters(), 1.0)
            self.optimizer.step()
            for key, value in zip(sums, (loss, kl, value_loss)):
                sums[key] += value.item() / d.updates_per_iteration
        return sums

    def save(self, path: Path) -> None:
        state = checkpoint_state(
            self.model, self.optimizer, self.config, self.env.spec,
            self.curriculum.state_dict(), self.global_step, self.iteration,
        )
        save_checkpoint(path, state)

    def run(self, iterations: int | None = None) -> Path:
        """Distill until `iterations` (default `distill.iterations`) or a plateau of the fresh KL
        once beta is 0. Saves checkpoints/latest.pt and best/model.pt (lowest smoothed fresh KL)."""
        d = self.config.distill
        end = d.iterations if iterations is None else iterations
        best_kl, plateau_kl, smoothed, since_best = math.inf, math.inf, None, 0
        best_path = self.run_dir / "best" / "model.pt"
        latest = self.run_dir / "checkpoints" / "latest.pt"
        try:
            while self.iteration < end:
                started = time.perf_counter()
                beta = self.beta()
                lr = self.learning_rate()
                for group in self.optimizer.param_groups:
                    group["lr"] = lr
                episodes, fresh = self.collect(beta)
                collected = time.perf_counter()
                stats = self.train_step()
                self.iteration += 1
                self.global_step += d.rollout_steps * d.num_envs
                elapsed = time.perf_counter() - started
                metrics = {
                    **stats, **fresh,
                    **training_summary(episodes, self.reward_terms, self.scenario_names),
                    "distill/beta": beta,
                    "distill/learning_rate": lr,
                    "distill/buffer": float(self.buffer.size),
                    "perf/steps_per_sec": d.rollout_steps * d.num_envs / elapsed,
                    "perf/collect_fraction": (collected - started) / elapsed,
                }
                self.logger.log(self.global_step, metrics)
                print(
                    f"iter {self.iteration}/{end} beta {beta:.2f} fresh_kl {fresh['distill/fresh_kl']:.4f} "
                    f"agree {fresh['distill/agreement']:.3f} kl {stats['distill/kl']:.4f} "
                    f"vloss {stats['distill/value_loss']:.3f} success {metrics['episode/success_rate']:.3f} "
                    f"sps {metrics['perf/steps_per_sec']:.0f}",
                    flush=True,
                )
                if self.iteration % max(self.config.checkpoint_interval, 1) == 0:
                    self.save(latest)
                if beta > 0:
                    continue
                kl = fresh["distill/fresh_kl"]
                smoothed = kl if smoothed is None else 0.8 * smoothed + 0.2 * kl
                if smoothed < best_kl:
                    best_kl = smoothed
                    self.save(best_path)
                # Only a 2% improvement resets the patience.
                if smoothed < 0.98 * plateau_kl:
                    plateau_kl, since_best = smoothed, 0
                else:
                    since_best += 1
                if since_best >= d.patience:
                    print(f"fresh KL plateaued at {best_kl:.4f}; stopping", flush=True)
                    break
        finally:
            self.save(latest)
            if not best_path.exists():
                self.save(best_path)
            self.logger.close()
        return best_path
