"""Checkpoints: model, optimizer, config, spec, curriculum, step counters."""

from __future__ import annotations

import os
from pathlib import Path

import torch

from .config import Config, config_from_dict, config_to_dict
from .env import policy_spec_hash
from .models import ActorCritic, build_model
from .obs import ObsShapes


def resolve_device(name: str) -> torch.device:
    if name == "auto":
        if torch.cuda.is_available():
            return torch.device("cuda")
        if torch.backends.mps.is_available():
            return torch.device("mps")
        return torch.device("cpu")
    return torch.device(name)


def save_checkpoint(path: Path, state: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    torch.save(state, tmp)
    os.replace(tmp, path)


def checkpoint_state(
    model: ActorCritic,
    optimizer: torch.optim.Optimizer,
    config: Config,
    spec: dict,
    curriculum: dict,
    global_step: int,
    iteration: int,
) -> dict:
    return {
        "model": model.state_dict(),
        "optimizer": optimizer.state_dict(),
        "config": config_to_dict(config),
        "spec": spec,
        "spec_hash": policy_spec_hash(spec, config),
        "obs_shapes": model.shapes.to_dict(),
        "curriculum": curriculum,
        "global_step": global_step,
        "iteration": iteration,
    }


def load_checkpoint(path: str | Path) -> dict:
    return torch.load(path, map_location="cpu", weights_only=False)


def load_policy(path: str | Path, device: torch.device) -> tuple[ActorCritic, Config, dict]:
    """Rebuild the model from a checkpoint. Works without the fsim module."""
    state = load_checkpoint(path)
    config = config_from_dict(state["config"])
    model = build_model(config, ObsShapes.from_dict(state["obs_shapes"]))
    model.load_state_dict(state["model"])
    model.to(device).eval()
    return model, config, state
