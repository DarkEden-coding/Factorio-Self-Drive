"""Dependency-free reference forward pass of an exported student (`fsim-rl export-student`).

Plain loops over nested lists, in the order the Lua mod computes them, so a Lua port can be
checked line by line against this file. Format (docs/rl.md, "Student export"):

1. `x[i] = clamp((features[i] - mean[i]) / std[i], -clip, clip)`.
2. If `ray_conv` is non-empty: the first `channels * rays` normalized features are the ray block,
   channel `c` of ray `r` at index `c * rays + r`. Each conv layer computes, for output channel
   `o` and position `p`, `bias[o] + sum_c sum_k weight[o][c][k] * in[c][(p * stride + k - pad) mod
   length]` with `pad = kernel // 2`, then ReLU. The last conv output, flattened channel-major
   (`o * length + p`), followed by the non-ray features, is the input of the dense layers.
3. Dense layers: `out[o] = bias[o] + sum_i weight[o][i] * in[i]`, ReLU on all but the last.
4. The last layer's 9 outputs are the action logits; the mod takes the argmax (first on ties).

Format v2 (`map` present): the observation is the 135 student features then the map,
`channels x cells x cells` channel-major. The dense input is the ray block (or its conv output),
the 9 route features only if `route`, the other student features, then the map (flattened as is,
or through `map.conv`: zero-padded 2D convs + ReLU, `weight[o][c][ky][kx]`, input row
`p * stride + ky - pad`, flattened channel-major `o * size * size + row * size + col`).
"""

from __future__ import annotations

import json


def load(path: str) -> dict:
    with open(path) as f:
        return json.load(f)


def normalize(model: dict, features: list[float]) -> list[float]:
    norm = model["normalizer"]
    mean, std, clip = norm["mean"], norm["std"], norm["clip"]
    out = []
    for i in range(len(features)):
        v = (features[i] - mean[i]) / std[i]
        if v > clip:
            v = clip
        elif v < -clip:
            v = -clip
        out.append(v)
    return out


def conv1d(layer: dict, x: list[list[float]]) -> list[list[float]]:
    """Circular 1D conv + ReLU over x[channel][position]."""
    length, kernel, stride = layer["length_in"], layer["kernel"], layer["stride"]
    pad = kernel // 2
    out = []
    for o in range(layer["out"]):
        row = []
        w = layer["weight"][o]
        for p in range(layer["length_out"]):
            s = layer["bias"][o]
            for c in range(layer["in"]):
                wc = w[c]
                xc = x[c]
                for k in range(kernel):
                    s += wc[k] * xc[(p * stride + k - pad) % length]
            row.append(s if s > 0.0 else 0.0)
        out.append(row)
    return out


def conv2d(layer: dict, x: list[list[list[float]]]) -> list[list[list[float]]]:
    """Zero-padded 2D conv + ReLU over x[channel][row][col]."""
    size, kernel, stride, pad = layer["size_in"], layer["kernel"], layer["stride"], layer["pad"]
    out = []
    for o in range(layer["out"]):
        w = layer["weight"][o]
        plane = []
        for py in range(layer["size_out"]):
            row = []
            for px in range(layer["size_out"]):
                s = layer["bias"][o]
                for c in range(layer["in"]):
                    for ky in range(kernel):
                        y = py * stride + ky - pad
                        if 0 <= y < size:
                            for kx in range(kernel):
                                xx = px * stride + kx - pad
                                if 0 <= xx < size:
                                    s += w[c][ky][kx] * x[c][y][xx]
                row.append(s if s > 0.0 else 0.0)
            plane.append(row)
        out.append(plane)
    return out


def dense(layer: dict, x: list[float], relu: bool) -> list[float]:
    out = []
    for o in range(layer["out"]):
        s = layer["bias"][o]
        w = layer["weight"][o]
        for i in range(layer["in"]):
            s += w[i] * x[i]
        out.append(s if (s > 0.0 or not relu) else 0.0)
    return out


def logits(model: dict, features: list[float]) -> list[float]:
    x = normalize(model, features)
    channels, rays = model["obs"]["ray_channels_count"], model["obs"]["ray_count"]
    ray_len = channels * rays
    student_len = model["obs"]["size"]
    head = x[:ray_len]
    convs = model["ray_conv"]
    if convs:
        h = [[x[c * rays + r] for r in range(rays)] for c in range(channels)]
        for layer in convs:
            h = conv1d(layer, h)
        head = []
        for row in h:
            head.extend(row)
    student_map = model.get("map")
    if student_map is None:
        x = head + x[ray_len:]
    else:
        route = 2 * len(model["obs"]["route_lookaheads"]) + 1
        rest = x[ray_len:student_len] if model["route"] else x[ray_len + route:student_len]
        cells, map_channels = student_map["cells"], student_map["channels_count"]
        tail = x[student_len:]
        if student_map["conv"]:
            h = [[[x[student_len + (c * cells + r) * cells + q] for q in range(cells)]
                  for r in range(cells)] for c in range(map_channels)]
            for layer in student_map["conv"]:
                h = conv2d(layer, h)
            tail = []
            for plane in h:
                for row in plane:
                    tail.extend(row)
        x = head + rest + tail
    layers = model["dense"]
    for i, layer in enumerate(layers):
        x = dense(layer, x, relu=i + 1 < len(layers))
    return x


def act(model: dict, features: list[float]) -> int:
    out = logits(model, features)
    best = 0
    for i in range(1, len(out)):
        if out[i] > out[best]:
            best = i
    return best
