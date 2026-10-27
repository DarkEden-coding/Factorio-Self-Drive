"""The interface every policy implements: forward(obs) -> (distribution, value), act, evaluate_actions.

`EncoderActorCritic` is the full-observation base (CNN encoder shared by the heads and the value)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

import numpy as np
import torch
from torch import nn

from ..config import ModelConfig
from ..obs import Obs, ObsShapes
from .encoder import Encoder


@dataclass
class ActResult:
    action: torch.Tensor    # what the buffer stores (for path_planner: the pre-tanh sample)
    log_prob: torch.Tensor  # [B]
    value: torch.Tensor     # [B]
    dist: Any


def init_head(layer: nn.Linear, gain: float) -> nn.Linear:
    nn.init.orthogonal_(layer.weight, gain)
    nn.init.zeros_(layer.bias)
    return layer


class ActorCritic(nn.Module):
    """Subclasses set `shapes` (the observation parts they read) and implement forward and value."""

    kind: str
    action_shape: tuple[int, ...] = ()
    action_dtype: torch.dtype = torch.int64
    shapes: ObsShapes

    def forward(self, obs: Obs):
        """Returns (action distribution, value [B])."""
        raise NotImplementedError

    def value(self, obs: Obs) -> torch.Tensor:
        raise NotImplementedError

    @torch.no_grad()
    def act(self, obs: Obs, deterministic: bool = False) -> ActResult:
        dist, value = self(obs)
        action = dist.mode() if deterministic else dist.sample()
        return ActResult(action, dist.log_prob(action), value, dist)

    def evaluate_actions(self, obs: Obs, action: torch.Tensor):
        """Returns (log_prob, entropy, value, dist) for stored actions."""
        dist, value = self(obs)
        return dist.log_prob(action), dist.entropy(), value, dist

    def aux_loss(self, dist, batch: dict) -> torch.Tensor:
        """Extra loss term (already weighted). Zero unless a model needs one."""
        return torch.zeros((), device=batch["advantages"].device)

    def env_action(self, action: torch.Tensor) -> np.ndarray:
        """Convert a stored action batch to what fsim.VecEnv.step expects."""
        return action.cpu().numpy().astype(np.int64)

    def overlays(self, result: ActResult) -> list[dict]:
        """Per-row model outputs for replays and the viewer (waypoints in the vehicle frame)."""
        return [{"value": float(v)} for v in result.value.cpu()]

    def update_normalizer(self, obs: Obs) -> None:
        """Update running input statistics from a batch (between updates)."""


class EncoderActorCritic(ActorCritic):
    def __init__(self, shapes: ObsShapes, cfg: ModelConfig):
        super().__init__()
        self.shapes = shapes
        self.encoder = Encoder(shapes, cfg)
        self.value_head = init_head(nn.Linear(self.encoder.out_features, 1), 1.0)

    def head(self, features: torch.Tensor):
        raise NotImplementedError

    def forward(self, obs: Obs):
        features = self.encoder(obs)
        return self.head(features), self.value_head(features).squeeze(-1)

    def value(self, obs: Obs) -> torch.Tensor:
        return self.value_head(self.encoder(obs)).squeeze(-1)

    def update_normalizer(self, obs: Obs) -> None:
        if self.encoder.vector_norm is not None:
            self.encoder.vector_norm.update(obs["vector"])
