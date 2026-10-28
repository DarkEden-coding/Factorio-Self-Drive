"""PPO loss and update over a flattened rollout batch."""

from __future__ import annotations

from collections import defaultdict

import numpy as np
import torch
from torch import nn

from .config import PPOConfig
from .models import ActorCritic
from .models.distributions import categorical_kl
from .obs import index_obs


def take(batch: dict, index: torch.Tensor) -> dict:
    out = {key: value[index] for key, value in batch.items() if key != "obs"}
    out["obs"] = index_obs(batch["obs"], index)
    return out


def ppo_loss(model: ActorCritic, mb: dict, cfg: PPOConfig, steer_levels: int = 1) -> tuple[torch.Tensor, dict]:
    log_prob, entropy, value, dist = model.evaluate_actions(mb["obs"], mb["actions"])
    log_ratio = log_prob - mb["log_probs"]
    ratio = log_ratio.exp()

    adv = mb["advantages"]
    if cfg.norm_adv and adv.numel() > 1:
        adv = (adv - adv.mean()) / (adv.std() + 1e-8)
    policy_loss = torch.max(
        -adv * ratio, -adv * ratio.clamp(1.0 - cfg.clip_coef, 1.0 + cfg.clip_coef)
    ).mean()

    if cfg.clip_value > 0:
        clipped = mb["values"] + (value - mb["values"]).clamp(-cfg.clip_value, cfg.clip_value)
        value_loss = 0.5 * torch.max(
            (value - mb["returns"]).square(), (clipped - mb["returns"]).square()
        ).mean()
    else:
        value_loss = 0.5 * (value - mb["returns"]).square().mean()

    entropy_mean = entropy.mean()
    aux_loss = model.aux_loss(dist, mb)
    loss = policy_loss + cfg.vf_coef * value_loss - cfg.ent_coef * entropy_mean + aux_loss
    teacher_kl = torch.zeros((), device=adv.device)
    if "teacher_log_probs" in mb:
        teacher_kl = categorical_kl(mb["teacher_log_probs"], dist.dist.logits).mean()
        loss = loss + cfg.teacher_kl_coef * teacher_kl

    steer_prior = torch.zeros((), device=adv.device)
    if cfg.steer_prior_coef > 0:
        steer_prior = model.steer_prior_loss(dist, mb["obs"], steer_levels)
        loss = loss + cfg.steer_prior_coef * steer_prior

    with torch.no_grad():
        approx_kl = ((ratio - 1.0) - log_ratio).mean()
        clip_frac = ((ratio - 1.0).abs() > cfg.clip_coef).float().mean()
    stats = {
        "loss/total": loss.detach(),
        "loss/policy": policy_loss.detach(),
        "loss/value": value_loss.detach(),
        "loss/entropy": entropy_mean.detach(),
        "loss/aux": aux_loss.detach(),
        "loss/teacher_kl": teacher_kl.detach(),
        "loss/steer_prior": steer_prior.detach(),
        "loss/approx_kl": approx_kl,
        "loss/clip_frac": clip_frac,
    }
    return loss, stats


def ppo_update(
    model: ActorCritic,
    optimizer: torch.optim.Optimizer,
    batch: dict,
    cfg: PPOConfig,
    generator: torch.Generator | None = None,
    steer_levels: int = 1,
) -> dict[str, float]:
    """Run the PPO epochs over a flattened batch. Stops early when approx KL exceeds 1.5 * target_kl."""
    size = batch["log_probs"].shape[0]
    minibatch = max(size // cfg.num_minibatches, 1)
    sums: dict[str, list[torch.Tensor]] = defaultdict(list)
    epochs_run = 0
    stop = False
    for _ in range(cfg.epochs):
        order = torch.randperm(size, generator=generator).to(batch["log_probs"].device)
        for start in range(0, size - minibatch + 1, minibatch):
            mb = take(batch, order[start : start + minibatch])
            loss, stats = ppo_loss(model, mb, cfg, steer_levels)
            optimizer.zero_grad(set_to_none=True)
            loss.backward()
            grad_norm = nn.utils.clip_grad_norm_(model.parameters(), cfg.max_grad_norm)
            optimizer.step()
            stats["loss/grad_norm"] = grad_norm.detach()
            for key, value in stats.items():
                sums[key].append(value)
            if cfg.target_kl > 0 and stats["loss/approx_kl"].item() > 1.5 * cfg.target_kl:
                stop = True
                break
        epochs_run += 1
        if stop:
            break
    out = {key: torch.stack(values).mean().item() for key, values in sums.items()}
    out["loss/epochs"] = float(epochs_run)
    values = batch["values"].cpu().numpy()
    returns = batch["returns"].cpu().numpy()
    var = np.var(returns)
    out["loss/explained_variance"] = float("nan") if var == 0 else float(1 - np.var(returns - values) / var)
    return out
