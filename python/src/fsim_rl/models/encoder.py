"""Shared observation encoder: CNNs for the two grids, MLP for the vector, MLP torso."""

from __future__ import annotations

import torch
from torch import nn

from ..config import ModelConfig
from ..obs import Obs, ObsShapes


class RunningNorm(nn.Module):
    """Running mean/std of the vector features. Updated by the trainer, saved in the state dict."""

    def __init__(self, size: int, clip: float = 10.0):
        super().__init__()
        self.clip = clip
        self.register_buffer("mean", torch.zeros(size))
        self.register_buffer("var", torch.ones(size))
        self.register_buffer("count", torch.tensor(1e-4))

    @torch.no_grad()
    def update(self, x: torch.Tensor) -> None:
        x = x.reshape(-1, x.shape[-1]).float()
        batch_mean = x.mean(0)
        batch_var = x.var(0, unbiased=False)
        batch_count = x.shape[0]
        delta = batch_mean - self.mean
        total = self.count + batch_count
        self.mean += delta * batch_count / total
        m2 = self.var * self.count + batch_var * batch_count + delta.square() * self.count * batch_count / total
        self.var.copy_(m2 / total)
        self.count.copy_(total)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        return ((x - self.mean) / torch.sqrt(self.var + 1e-8)).clamp(-self.clip, self.clip)


def conv_stack(shape: tuple[int, int, int], channels: list[int], features: int) -> nn.Sequential:
    """Conv layers that each halve the grid (k4 s2 p1), then a linear projection."""
    in_ch, height, width = shape
    layers: list[nn.Module] = []
    for out_ch in channels:
        layers += [nn.Conv2d(in_ch, out_ch, kernel_size=4, stride=2, padding=1), nn.ReLU()]
        in_ch, height, width = out_ch, height // 2, width // 2
    layers += [nn.Flatten(), nn.Linear(in_ch * height * width, features), nn.ReLU()]
    return nn.Sequential(*layers)


def mlp(sizes: list[int]) -> nn.Sequential:
    layers: list[nn.Module] = []
    for a, b in zip(sizes[:-1], sizes[1:]):
        layers += [nn.Linear(a, b), nn.ReLU()]
    return nn.Sequential(*layers)


class Encoder(nn.Module):
    def __init__(self, shapes: ObsShapes, cfg: ModelConfig):
        super().__init__()
        self.local = conv_stack(shapes.local, cfg.local_cnn, cfg.local_features)
        self.coarse = conv_stack(shapes.coarse, cfg.coarse_cnn, cfg.coarse_features)
        self.vector_norm = RunningNorm(shapes.vector) if cfg.normalize_vector else None
        self.vector = mlp([shapes.vector, cfg.vector_hidden, cfg.vector_hidden])
        joined = cfg.local_features + cfg.coarse_features + cfg.vector_hidden
        self.torso = mlp([joined, *cfg.torso_hidden])
        self.out_features = cfg.torso_hidden[-1] if cfg.torso_hidden else joined
        if cfg.precision not in ("bf16", "fp32"):
            raise ValueError(f"model.precision must be bf16 or fp32, got {cfg.precision!r}")
        self.bf16 = cfg.precision == "bf16"

    def forward(self, obs: Obs) -> torch.Tensor:
        local = obs["local"].float() / 255.0
        coarse = obs["coarse"].float() / 255.0
        vector = obs["vector"].float()
        if self.vector_norm is not None:
            vector = self.vector_norm(vector)
        # bf16 roughly doubles CNN throughput on MPS. Features return as fp32 so the
        # action and value heads, log-probs and PPO ratios stay full precision.
        device = local.device.type
        with torch.autocast(device_type=device, dtype=torch.bfloat16, enabled=self.bf16 and device in ("mps", "cuda")):
            joined = torch.cat([self.local(local), self.coarse(coarse), self.vector(vector)], dim=-1)
            features = self.torso(joined)
        return features.float()
