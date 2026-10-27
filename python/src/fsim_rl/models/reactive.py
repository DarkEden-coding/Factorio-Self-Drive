"""Reactive direct control: one of 9 actions per decision."""

from __future__ import annotations

from torch import nn

from ..config import ModelConfig
from ..obs import ObsShapes
from .base import ActResult, EncoderActorCritic, init_head
from .distributions import CategoricalDist

NUM_ACTIONS = 9


class ReactivePolicy(EncoderActorCritic):
    kind = "reactive"

    def __init__(self, shapes: ObsShapes, cfg: ModelConfig):
        super().__init__(shapes, cfg)
        self.logits = init_head(nn.Linear(self.encoder.out_features, NUM_ACTIONS), 0.01)

    def head(self, features):
        return CategoricalDist(self.logits(features))

    def overlays(self, result: ActResult) -> list[dict]:
        probs = result.dist.probs.cpu().tolist()
        values = result.value.cpu().tolist()
        return [{"probs": p, "value": v} for p, v in zip(probs, values)]
