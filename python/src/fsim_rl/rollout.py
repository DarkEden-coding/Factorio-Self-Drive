"""Rollout storage (u8 grids on the training device) and semi-MDP GAE."""

from __future__ import annotations

import torch

from .obs import Obs, ObsShapes


def compute_gae(
    rewards: torch.Tensor,
    discounts: torch.Tensor,
    dones: torch.Tensor,
    values: torch.Tensor,
    last_value: torch.Tensor,
    lam: float,
) -> tuple[torch.Tensor, torch.Tensor]:
    """GAE where each step has its own discount (gamma^k for a k-decision macro step).

    All inputs [T, N] except last_value [N]. dones[t] means the episode ended during step t; its
    reward already includes any truncation bootstrap. Returns (advantages, returns).
    """
    steps = rewards.shape[0]
    advantages = torch.zeros_like(rewards)
    next_adv = torch.zeros_like(last_value)
    next_value = last_value
    for t in reversed(range(steps)):
        carry = discounts[t] * (1.0 - dones[t].float())
        delta = rewards[t] + carry * next_value - values[t]
        next_adv = delta + carry * lam * next_adv
        advantages[t] = next_adv
        next_value = values[t]
    return advantages, advantages + values


class RolloutBuffer:
    def __init__(
        self,
        steps: int,
        num_envs: int,
        shapes: ObsShapes,
        action_shape: tuple[int, ...],
        action_dtype: torch.dtype,
        device: torch.device,
    ):
        self.steps = steps
        self.num_envs = num_envs
        tn = (steps, num_envs)
        dims = {"local": shapes.local, "coarse": shapes.coarse, "vector": (shapes.vector,),
                "student": (shapes.student,)}
        dtypes = {"local": torch.uint8, "coarse": torch.uint8}
        # Only the parts the model reads; the env may return more (e.g. for a teacher).
        self.obs = {
            key: torch.zeros(*tn, *dims[key], dtype=dtypes.get(key, torch.float32), device=device)
            for key in shapes.keys()
        }
        self.actions = torch.zeros(*tn, *action_shape, dtype=action_dtype, device=device)
        self.log_probs = torch.zeros(tn, device=device)
        self.values = torch.zeros(tn, device=device)
        self.rewards = torch.zeros(tn, device=device)
        self.discounts = torch.zeros(tn, device=device)
        self.dones = torch.zeros(tn, dtype=torch.bool, device=device)
        self.executed = torch.zeros(tn, dtype=torch.long, device=device)
        self.advantages = torch.zeros(tn, device=device)
        self.returns = torch.zeros(tn, device=device)

    def store_policy(self, t: int, obs: Obs, action, log_prob, value) -> None:
        for key, buffer in self.obs.items():
            buffer[t] = obs[key]
        self.actions[t] = action
        self.log_probs[t] = log_prob
        self.values[t] = value

    def store_step(self, t: int, reward, discount, done, executed) -> None:
        self.rewards[t] = reward
        self.discounts[t] = discount
        self.dones[t] = done
        self.executed[t] = executed

    def finish(self, last_value: torch.Tensor, lam: float) -> None:
        self.advantages, self.returns = compute_gae(
            self.rewards, self.discounts, self.dones, self.values, last_value, lam
        )

    def flatten(self, extra: dict[str, torch.Tensor] | None = None) -> dict:
        """Everything as [T*N, ...] tensors for minibatching."""
        def flat(x: torch.Tensor) -> torch.Tensor:
            return x.reshape(self.steps * self.num_envs, *x.shape[2:])

        batch = {
            "obs": {key: flat(value) for key, value in self.obs.items()},
            "actions": flat(self.actions),
            "log_probs": flat(self.log_probs),
            "values": flat(self.values),
            "advantages": flat(self.advantages),
            "returns": flat(self.returns),
        }
        for key, value in (extra or {}).items():
            batch[key] = flat(value)
        return batch
