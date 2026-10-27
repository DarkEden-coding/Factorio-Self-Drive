"""Action distributions. Each has sample(), mode(), log_prob(action), entropy()."""

from __future__ import annotations

import math

import torch
import torch.nn.functional as F
from torch.distributions import Categorical, Normal


def categorical_kl(target_log_probs: torch.Tensor, logits: torch.Tensor) -> torch.Tensor:
    """KL(target || softmax(logits)) per row, from the target's log-probs."""
    return (target_log_probs.exp() * (target_log_probs - F.log_softmax(logits, -1))).sum(-1)


class CategoricalDist:
    """One of the 9 discrete actions."""

    def __init__(self, logits: torch.Tensor):
        self.dist = Categorical(logits=logits, validate_args=False)

    @property
    def probs(self) -> torch.Tensor:
        return self.dist.probs

    def sample(self) -> torch.Tensor:
        return self.dist.sample()

    def mode(self) -> torch.Tensor:
        return self.dist.probs.argmax(-1)

    def log_prob(self, action: torch.Tensor) -> torch.Tensor:
        return self.dist.log_prob(action)

    def entropy(self) -> torch.Tensor:
        return self.dist.entropy()


class PlanDist:
    """Factorized plan of H discrete actions. Log-prob and entropy cover the executed prefix only."""

    def __init__(self, logits: torch.Tensor, execute: int):
        self.logits = logits  # [B, H, 9]
        self.execute = execute
        self.dist = Categorical(logits=logits, validate_args=False)

    @property
    def probs(self) -> torch.Tensor:
        return self.dist.probs

    def sample(self) -> torch.Tensor:
        return self.dist.sample()

    def mode(self) -> torch.Tensor:
        return self.logits.argmax(-1)

    def log_prob(self, action: torch.Tensor) -> torch.Tensor:
        return self.dist.log_prob(action)[:, : self.execute].sum(-1)

    def entropy(self) -> torch.Tensor:
        return self.dist.entropy()[:, : self.execute].sum(-1)


class TanhGaussian:
    """Diagonal Gaussian over u, squashed to a = tanh(u) in (-1, 1).

    Actions are stored as the pre-tanh u, so log-probs stay exact without atanh. The log-prob is
    the density of a: log N(u) - sum log(1 - tanh(u)^2). Entropy is that of the Gaussian over u
    (the squashed entropy has no closed form); it is only used as a bonus.
    """

    def __init__(self, mean: torch.Tensor, log_std: torch.Tensor):
        self.normal = Normal(mean, log_std.exp().expand_as(mean), validate_args=False)

    def sample(self) -> torch.Tensor:
        return self.normal.sample()

    def mode(self) -> torch.Tensor:
        return self.normal.mean

    def log_prob(self, u: torch.Tensor) -> torch.Tensor:
        # log(1 - tanh(u)^2) = 2 * (log 2 - u - softplus(-2u)), stable for large |u|
        log_det = 2.0 * (math.log(2.0) - u - F.softplus(-2.0 * u))
        return (self.normal.log_prob(u) - log_det).sum(-1)

    def entropy(self) -> torch.Tensor:
        return self.normal.entropy().sum(-1)
