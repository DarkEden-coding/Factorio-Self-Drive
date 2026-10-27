"""Path planner: K waypoints as per-segment (length, heading change), tanh-squashed Gaussian.

Waypoints are in the vehicle frame: x = right, y = forward, tiles. Segment i has length in
[segment_min_length, segment_max_length] and turns by up to max_turn radians (positive = right)
relative to segment i-1; the first segment turns relative to the vehicle heading. Total reach is
at most K * segment_max_length, which config validation keeps within max_reach.
"""

from __future__ import annotations

import numpy as np
import torch
from torch import nn

from ..config import ModelConfig, PathPlannerConfig
from ..obs import ObsShapes
from .base import ActResult, EncoderActorCritic, init_head
from .distributions import TanhGaussian


def segments_to_waypoints(
    squashed: torch.Tensor, min_length: float, max_length: float, max_turn: float
) -> torch.Tensor:
    """squashed [B, K, 2] in (-1, 1) -> waypoints [B, K, 2] (x right, y forward)."""
    length = min_length + (squashed[..., 0] + 1.0) * 0.5 * (max_length - min_length)
    heading = torch.cumsum(squashed[..., 1] * max_turn, dim=-1)
    x = torch.cumsum(length * torch.sin(heading), dim=-1)
    y = torch.cumsum(length * torch.cos(heading), dim=-1)
    return torch.stack([x, y], dim=-1)


class PathPlannerPolicy(EncoderActorCritic):
    kind = "path_planner"
    action_dtype = torch.float32

    def __init__(self, shapes: ObsShapes, cfg: ModelConfig, planner: PathPlannerConfig):
        super().__init__(shapes, cfg)
        self.num_waypoints = planner.waypoints
        self.min_length = cfg.segment_min_length
        self.max_length = cfg.segment_max_length
        self.max_turn = cfg.max_turn
        self.action_shape = (planner.waypoints * 2,)
        self.mean = init_head(nn.Linear(self.encoder.out_features, planner.waypoints * 2), 0.01)
        self.log_std = nn.Parameter(torch.full((planner.waypoints * 2,), cfg.init_log_std))

    def head(self, features):
        return TanhGaussian(self.mean(features), self.log_std)

    def waypoints(self, action: torch.Tensor) -> torch.Tensor:
        squashed = torch.tanh(action).view(-1, self.num_waypoints, 2)
        return segments_to_waypoints(squashed, self.min_length, self.max_length, self.max_turn)

    def env_action(self, action: torch.Tensor) -> np.ndarray:
        return self.waypoints(action).cpu().numpy().astype(np.float32)

    def overlays(self, result: ActResult) -> list[dict]:
        points = self.waypoints(result.action).cpu().tolist()
        values = result.value.cpu().tolist()
        return [{"value": v, "waypoints": p} for v, p in zip(values, points)]
