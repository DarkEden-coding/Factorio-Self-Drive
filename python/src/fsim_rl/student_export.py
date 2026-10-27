"""Export a student checkpoint as JSON for the Factorio mod (`fsim-rl export-student`), with a
cost estimate for running it in Lua. `student_ref.py` evaluates the exported file."""

from __future__ import annotations

import math

import torch
from torch import nn

from .config import Config
from .models.student import (
    MAP_CELLS, MAP_CHANNELS, RAY_CHANNELS, RAY_COUNT, StudentPolicy, inference_cost,
)

FORMAT = "fsim-student-v1"
# v2 adds the student map and optional route features (`map`, `route`).
FORMAT_MAP = "fsim-student-v2"
# Assumed Factorio Lua throughput: simple operations (one arithmetic op with its table reads) per
# second. A multiply-add is 2, a normalizer input or ReLU 1, a ray-march tile step about 8 (two
# compares, an add, a grid read and three flag tests).
LUA_OPS_PER_SEC = 20e6
OPS_PER_RAY_TILE = 8
# The largest hull extent along any ray (tank half length), for the worst-case tile count.
MAX_HULL_EXTENT = 1.3


def _rounded(t: torch.Tensor) -> list:
    """Nested lists of floats with 6 significant digits (the reference pass uses these values)."""
    return [_rounded(row) for row in t] if t.dim() > 1 else [float(f"{v:.6g}") for v in t.tolist()]


def export_student(model: StudentPolicy, config: Config, state: dict) -> dict:
    if model.norm is None:
        raise SystemExit("export needs a student trained with model.normalize_vector = true")
    spec = state["spec"]
    norm = model.norm
    convs = []
    if model.rays is not None:
        length = RAY_COUNT
        for layer in model.rays.net:
            if isinstance(layer, nn.Conv1d):
                out_length = (length - 1) // layer.stride[0] + 1
                convs.append({
                    "in": layer.in_channels, "out": layer.out_channels,
                    "kernel": layer.kernel_size[0], "stride": layer.stride[0],
                    "length_in": length, "length_out": out_length,
                    "weight": _rounded(layer.weight.detach().cpu()),
                    "bias": _rounded(layer.bias.detach().cpu()),
                })
                length = out_length
    student_map = None
    if model.has_map:
        map_convs, size = [], MAP_CELLS
        for layer in model.map.net if model.map is not None else []:
            if isinstance(layer, nn.Conv2d):
                out_size = (size - 1) // layer.stride[0] + 1
                map_convs.append({
                    "in": layer.in_channels, "out": layer.out_channels,
                    "kernel": layer.kernel_size[0], "stride": layer.stride[0], "pad": layer.padding[0],
                    "size_in": size, "size_out": out_size,
                    "weight": _rounded(layer.weight.detach().cpu()),
                    "bias": _rounded(layer.bias.detach().cpu()),
                })
                size = out_size
        student_map = {**spec["student"]["map"], "channels_count": MAP_CHANNELS, "conv": map_convs,
                       "route_source": config.env.route_source}
    dense = [
        {
            "in": layer.in_features, "out": layer.out_features,
            "weight": _rounded(layer.weight.detach().cpu()),
            "bias": _rounded(layer.bias.detach().cpu()),
        }
        for layer in [*model.actor, model.logits]
        if isinstance(layer, nn.Linear)
    ]
    return {
        "format": FORMAT_MAP if student_map else FORMAT,
        "spec_hash": state["spec_hash"],
        "obs": {**spec["student"], "ray_count": RAY_COUNT, "ray_channels_count": RAY_CHANNELS},
        "actions": spec["actions"],
        "decision_ticks": config.env.decision_ticks,
        "normalizer": {
            "mean": _rounded(norm.mean.detach().cpu()),
            "std": _rounded(torch.sqrt(norm.var + 1e-8).detach().cpu()),
            "clip": norm.clip,
        },
        "ray_conv": convs,
        # v2: whether the actor reads the route features, and the map with its convs.
        **({"route": model.route, "map": student_map} if student_map else {}),
        "dense": dense,
        "cost": inference_cost(model),
    }


def worst_case_ray_tiles(obs: dict) -> int:
    """Upper bound on tiles visited by all rays: a ray of length L at angle a crosses at most
    L * (|sin a| + |cos a|) tile boundaries."""
    total = 0
    for angle in obs["ray_angles_deg"]:
        a = math.radians(angle)
        reach = obs["ray_range"] + MAX_HULL_EXTENT
        total += math.ceil(reach * (abs(math.sin(a)) + abs(math.cos(a)))) + 1
    return total


def lua_estimate(cost: dict, ray_tiles: float) -> dict:
    network = 2 * cost["macs"] + cost["elementwise"]
    rays = OPS_PER_RAY_TILE * ray_tiles
    return {
        "assumed_ops_per_sec": LUA_OPS_PER_SEC,
        "network_ops": network,
        "ray_tiles": ray_tiles,
        "ray_ops": rays,
        "network_ms": network / LUA_OPS_PER_SEC * 1e3,
        "total_ms": (network + rays) / LUA_OPS_PER_SEC * 1e3,
    }
