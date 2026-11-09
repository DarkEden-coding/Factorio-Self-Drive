#!/usr/bin/env python3
"""Rebuild a student checkpoint from the mod's generated weights.lua.

weights.lua holds the exported actor (dense layers, normalizer) of the shipped model. Its actor
and normalizer replace those of a template checkpoint with the same layout; the critic and
optimizer stay the template's, so the result is for evaluation (`fsim-rl suite`), or a
starting point whose critic needs to re-fit. Run from python/ (`uv run python ../scripts/...`).
"""
import argparse
from pathlib import Path

import lupa.lua52 as lupa
import torch


def lua_value(value):
    """Lua tables to Python lists/dicts (array tables become lists)."""
    if lupa.lua_type(value) != "table":
        return value
    keys = list(value.keys())
    if keys and all(isinstance(k, int) for k in keys) and sorted(keys) == list(range(1, len(keys) + 1)):
        return [lua_value(value[k]) for k in range(1, len(keys) + 1)]
    return {k: lua_value(v) for k, v in value.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("weights", type=Path, help="mod/factorio-autodrive/weights.lua")
    parser.add_argument("template", type=Path, help="checkpoint with the same student layout")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    runtime = lupa.LuaRuntime()
    model = lua_value(runtime.execute(args.weights.read_text()))
    state = torch.load(args.template, map_location="cpu", weights_only=False)
    if model["spec_hash"] != state["spec_hash"]:
        raise SystemExit(f"spec hash {model['spec_hash']} != template {state['spec_hash']}")
    weights = state["model"]
    names = ["actor.0", "actor.2", "logits"]
    if len(model["dense"]) != len(names):
        raise SystemExit("expected two hidden layers and the logits layer")
    for name, layer in zip(names, model["dense"]):
        weight = torch.tensor(layer["weight"], dtype=torch.float32).reshape(layer["out"], layer["in"])
        if weight.shape != weights[f"{name}.weight"].shape:
            raise SystemExit(f"{name}: shape {tuple(weight.shape)} differs from the template")
        weights[f"{name}.weight"] = weight
        weights[f"{name}.bias"] = torch.tensor(layer["bias"], dtype=torch.float32)
    std = torch.tensor(model["normalizer"]["std"], dtype=torch.float32)
    weights["norm.mean"] = torch.tensor(model["normalizer"]["mean"], dtype=torch.float32)
    # The export stores sqrt(var + 1e-8).
    weights["norm.var"] = std * std - 1e-8
    state["source_weights"] = str(args.weights)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    torch.save(state, args.out)
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
