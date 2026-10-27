"""Student policy small enough for a Factorio mod in Lua: an MLP (optionally after circular 1D
convs over the rays) on the compact `student` observation (docs/rl.md, "Student observation").
With `env.student_map`, the observation ends with a coarse map that the actor reads flat or
through small stride-2 convs; `student_route = false` drops the route features from the actor.

Only the actor ships: normalizer, optional ray convs, `actor` MLP and `logits`. The critic is a
separate, larger MLP used only in training, so it costs nothing in game.
"""

from __future__ import annotations

import torch
from torch import nn

from ..config import ModelConfig
from ..obs import Obs, ObsShapes
from .base import ActorCritic, init_head
from .distributions import CategoricalDist
from .encoder import RunningNorm, mlp
from .reactive import NUM_ACTIONS, ReactivePolicy

# The student observation starts with RAY_CHANNELS blocks of RAY_COUNT ray features (student_obs.rs).
RAY_CHANNELS = 3
RAY_COUNT = 32
RAYS = RAY_CHANNELS * RAY_COUNT
# Then the route features (4 lookaheads x 2 + route length), the shared features and, with the
# student map, MAP_CHANNELS x MAP_CELLS x MAP_CELLS map cells.
ROUTE = 9
STUDENT_LEN = 135
MAP_CHANNELS = 3
MAP_CELLS = 16
MAP_LEN = MAP_CHANNELS * MAP_CELLS * MAP_CELLS


class RayConv(nn.Module):
    """Circular 1D convs over the rays: the first kernel 5 stride 1, the rest kernel 3 stride 2.
    Rays are in circular angle order, so wrap-around padding joins the back rays."""

    def __init__(self, channels: list[int]):
        super().__init__()
        layers: list[nn.Module] = []
        in_ch, length = RAY_CHANNELS, RAY_COUNT
        for i, out_ch in enumerate(channels):
            kernel, stride = (5, 1) if i == 0 else (3, 2)
            layers += [
                nn.Conv1d(in_ch, out_ch, kernel, stride, padding=kernel // 2, padding_mode="circular"),
                nn.ReLU(),
            ]
            in_ch, length = out_ch, (length - 1) // stride + 1
        self.net = nn.Sequential(*layers, nn.Flatten())
        self.out_features = in_ch * length

    def forward(self, rays: torch.Tensor) -> torch.Tensor:
        return self.net(rays)


class MapConv(nn.Module):
    """Kernel 3 stride 2 convs over the student map, then flattened."""

    def __init__(self, channels: list[int]):
        super().__init__()
        layers: list[nn.Module] = []
        in_ch, size = MAP_CHANNELS, MAP_CELLS
        for out_ch in channels:
            layers += [nn.Conv2d(in_ch, out_ch, 3, 2, padding=1), nn.ReLU()]
            in_ch, size = out_ch, (size - 1) // 2 + 1
        self.net = nn.Sequential(*layers, nn.Flatten())
        self.out_features = in_ch * size * size

    def forward(self, cells: torch.Tensor) -> torch.Tensor:
        return self.net(cells)


class StudentPolicy(ActorCritic):
    kind = "reactive"

    def __init__(self, shapes: ObsShapes, cfg: ModelConfig):
        super().__init__()
        if shapes.student is None:
            raise ValueError("a student model needs the student observation")
        self.shapes = shapes
        size = shapes.student
        self.norm = RunningNorm(size) if cfg.normalize_vector else None
        self.rays = RayConv(cfg.ray_conv) if cfg.ray_conv else None
        self.route = cfg.student_route
        self.has_map = size == STUDENT_LEN + MAP_LEN
        self.map = MapConv(cfg.map_conv) if cfg.map_conv else None
        actor_in = (
            (RAYS if self.rays is None else self.rays.out_features)
            + (ROUTE if self.route else 0)
            + STUDENT_LEN - RAYS - ROUTE
            + (0 if not self.has_map else MAP_LEN if self.map is None else self.map.out_features)
        )
        self.actor = mlp([actor_in, *cfg.student_hidden])
        self.logits = init_head(nn.Linear(cfg.student_hidden[-1], NUM_ACTIONS), 0.01)
        self.critic = mlp([size, *cfg.critic_hidden])
        self.value_head = init_head(nn.Linear(cfg.critic_hidden[-1], 1), 1.0)

    def normalized(self, obs: Obs) -> torch.Tensor:
        x = obs["student"].float()
        return self.norm(x) if self.norm is not None else x

    def policy_logits(self, x: torch.Tensor) -> torch.Tensor:
        """Action logits from normalized features: the part that runs in game."""
        rays = x[:, :RAYS]
        if self.rays is not None:
            rays = self.rays(rays.reshape(-1, RAY_CHANNELS, RAY_COUNT))
        parts = [rays, x[:, RAYS:RAYS + ROUTE]] if self.route else [rays]
        parts.append(x[:, RAYS + ROUTE:STUDENT_LEN])
        if self.has_map:
            cells = x[:, STUDENT_LEN:]
            if self.map is not None:
                cells = self.map(cells.reshape(-1, MAP_CHANNELS, MAP_CELLS, MAP_CELLS))
            parts.append(cells)
        return self.logits(self.actor(torch.cat(parts, dim=-1)))

    def forward(self, obs: Obs):
        x = self.normalized(obs)
        return CategoricalDist(self.policy_logits(x)), self.value_head(self.critic(x)).squeeze(-1)

    def value(self, obs: Obs) -> torch.Tensor:
        return self.value_head(self.critic(self.normalized(obs))).squeeze(-1)

    def update_normalizer(self, obs: Obs) -> None:
        if self.norm is not None:
            self.norm.update(obs["student"])

    overlays = ReactivePolicy.overlays


def inference_cost(model: StudentPolicy) -> dict:
    """Shipped size and per-decision compute of the actor (the critic is excluded).

    `macs` counts multiply-adds of convs and dense layers; `elementwise` counts the normalizer
    (subtract, divide, clamp per input) and ReLUs, which are cheaper than a MAC each."""
    macs = 0
    elementwise = model.shapes.student * 3
    params = 0
    if model.rays is not None:
        length = RAY_COUNT
        for layer in model.rays.net:
            if isinstance(layer, nn.Conv1d):
                length = (length - 1) // layer.stride[0] + 1
                macs += length * layer.out_channels * layer.in_channels * layer.kernel_size[0]
                elementwise += length * layer.out_channels
                params += layer.weight.numel() + layer.bias.numel()
    if model.map is not None:
        size = MAP_CELLS
        for layer in model.map.net:
            if isinstance(layer, nn.Conv2d):
                size = (size - 1) // 2 + 1
                macs += size * size * layer.out_channels * layer.in_channels * 9
                elementwise += size * size * layer.out_channels
                params += layer.weight.numel() + layer.bias.numel()
    for layer in [*model.actor, model.logits]:
        if isinstance(layer, nn.Linear):
            macs += layer.in_features * layer.out_features
            params += layer.weight.numel() + layer.bias.numel()
            if layer is not model.logits:
                elementwise += layer.out_features
    normalizer = 2 * model.shapes.student if model.norm is not None else 0
    return {"params": params, "normalizer_values": normalizer, "macs": macs, "elementwise": elementwise}
