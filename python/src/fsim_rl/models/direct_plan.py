"""Direct control with a plan: H actions per call, the env executes the first `execute`.

PPO trains the executed prefix. The rest of the plan gets a consistency loss toward the actions the
policy actually took at those future decisions (targets built by `plan_consistency_targets`).
"""

from __future__ import annotations

import torch
import torch.nn.functional as F
from torch import nn

from ..config import DirectPlanConfig, ModelConfig
from ..obs import ObsShapes
from .base import ActResult, EncoderActorCritic, init_head
from .distributions import PlanDist
from .reactive import NUM_ACTIONS


class DirectPlanPolicy(EncoderActorCritic):
    kind = "direct_plan"

    def __init__(self, shapes: ObsShapes, cfg: ModelConfig, plan: DirectPlanConfig):
        super().__init__(shapes, cfg)
        self.horizon = plan.horizon
        self.execute = plan.execute
        self.consistency_coef = cfg.plan_consistency_coef
        self.action_shape = (plan.horizon,)
        self.logits = init_head(nn.Linear(self.encoder.out_features, plan.horizon * NUM_ACTIONS), 0.01)

    def head(self, features):
        logits = self.logits(features).view(-1, self.horizon, NUM_ACTIONS)
        return PlanDist(logits, self.execute)

    def aux_loss(self, dist: PlanDist, batch: dict) -> torch.Tensor:
        mask = batch["plan_mask"]
        if self.consistency_coef == 0 or not mask.any():
            return torch.zeros((), device=mask.device)
        ce = F.cross_entropy(dist.logits[mask], batch["plan_targets"][mask])
        return self.consistency_coef * ce

    def overlays(self, result: ActResult) -> list[dict]:
        first_probs = result.dist.probs[:, 0].cpu().tolist()
        plans = result.action.cpu().tolist()
        values = result.value.cpu().tolist()
        return [
            {"probs": p, "value": v, "plan_actions": plan}
            for p, v, plan in zip(first_probs, values, plans)
        ]


def plan_consistency_targets(
    actions: torch.Tensor, dones: torch.Tensor, executed: torch.Tensor, execute: int
) -> tuple[torch.Tensor, torch.Tensor]:
    """Targets for plan steps j >= execute from the same rollout.

    actions [T, N, H] long, dones [T, N] bool (episode ended during step t), executed [T, N]
    decisions actually run at step t. Plan step j of the call at step t lines up with decision
    j % execute of the call at step t + j // execute. It is valid when that step is inside the
    rollout, no episode ended in steps t .. t + j//execute - 1, and that decision actually ran.
    Returns (targets [T, N, H] long, mask [T, N, H] bool).
    """
    steps, envs, horizon = actions.shape
    device = actions.device
    targets = torch.zeros_like(actions)
    mask = torch.zeros(actions.shape, dtype=torch.bool, device=device)
    # ended_before[k] = episodes ended in steps 0..k-1
    ended_before = torch.zeros(steps + 1, envs, dtype=torch.long, device=device)
    ended_before[1:] = dones.long().cumsum(0)
    for j in range(execute, horizon):
        ahead, sub = divmod(j, execute)
        if ahead >= steps:
            break
        source = slice(ahead, steps)
        count = steps - ahead
        same_episode = ended_before[ahead:steps] - ended_before[0:count] == 0
        ran = executed[source] > sub
        targets[:count, :, j] = actions[source][:, :, sub]
        mask[:count, :, j] = same_episode & ran
    return targets, mask
