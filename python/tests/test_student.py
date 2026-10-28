"""Student model, distillation loss and the JSON export with its dependency-free forward pass."""

import json

import pytest
import torch

from fsim_rl.config import CONFIG_DIR, load_config
from fsim_rl.distill import distill_loss
from fsim_rl.models import build_model
from fsim_rl.models.student import inference_cost, widen_steer_head
from fsim_rl.obs import ObsShapes
from fsim_rl import student_ref
from fsim_rl.student_export import export_student

SIZE = 135
SHAPES = ObsShapes(student=SIZE)


def student(name: str, size: int = SIZE):
    config = load_config(CONFIG_DIR / f"{name}.toml", {"device": "cpu"})
    torch.manual_seed(0)
    return build_model(config, ObsShapes(student=size)), config


def test_actor_cost_counts_only_the_shipped_layers():
    model, _ = student("student_mlp64")
    cost = inference_cost(model)
    assert cost["macs"] == SIZE * 64 + 64 * 64 + 64 * 9
    assert cost["params"] == cost["macs"] + 64 + 64 + 9
    conv, _ = student("student_conv")
    # 3x32 -k5-> 8x32 -k3 s2-> 8x16, flattened with the 39 non-ray features.
    assert inference_cost(conv)["macs"] == 32 * 8 * 3 * 5 + 16 * 8 * 8 * 3 + (128 + 39) * 64 + 64 * 64 + 64 * 9


def test_widened_steer_head_keeps_classic_actions_and_demotes_partial_strengths():
    classic, config = student("student_mlp32")
    wide = build_model(load_config(CONFIG_DIR / "student_mlp32.toml", {"device": "cpu", "env": {"steer_levels": 4}}),
                       ObsShapes(student=SIZE))
    wide.load_state_dict(widen_steer_head(classic.state_dict(), 4))
    obs = {"student": torch.randn(8, SIZE)}
    classic.update_normalizer(obs)
    wide.norm.load_state_dict(classic.norm.state_dict())
    with torch.no_grad():
        old = classic.policy_logits(classic.normalized(obs)).view(8, 3, 3)
        new = wide.policy_logits(wide.normalized(obs)).view(8, 3, 9)
    for strength, source, offset in [(-4, 0, 0), (0, 1, 0), (4, 2, 0), (-2, 0, 1), (3, 2, 1)]:
        assert torch.allclose(new[:, :, strength + 4], old[:, :, source] - offset, atol=1e-5)


def test_distill_loss_is_zero_at_the_teacher_and_training_reduces_it():
    model, _ = student("student_mlp32")
    obs = {"student": torch.randn(256, SIZE)}
    model.update_normalizer(obs)
    with torch.no_grad():
        dist, value = model(obs)
    loss, kl, value_loss = distill_loss(model, obs, dist.dist.logits, value, 0.5)
    assert kl.item() < 1e-6 and value_loss.item() < 1e-10
    teacher_log_probs = torch.log_softmax(torch.randn(256, 9) * 3, -1)
    teacher_values = torch.randn(256)
    optimizer = torch.optim.Adam(model.parameters(), lr=3e-3)
    first = None
    for _ in range(200):
        loss, kl, _ = distill_loss(model, obs, teacher_log_probs, teacher_values, 0.5)
        first = first if first is not None else loss.item()
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()
    assert loss.item() < 0.5 * first


@pytest.mark.parametrize("name", ["student_mlp32", "student_conv", "student_map", "student_map_path"])
def test_export_round_trip_matches_torch(name):
    size = SIZE + 768 if name.startswith("student_map") else SIZE
    model, config = student(name, size)
    model.update_normalizer({"student": torch.randn(512, size) * 3 + 1})
    spec = {"student": {"features": [f"f{i}" for i in range(SIZE)], "ray_angles_deg": [0.0] * 32,
                        "ray_range": 32.0, "route_lookaheads": [4.0, 8.0, 16.0, 32.0], "size": SIZE,
                        "spec_hash": "abc",
                        "map": {"cells": 16, "cell_tiles": 8.0, "samples": 4, "size": size}},
            "actions": {"count": 9}}
    exported = json.loads(json.dumps(export_student(model, config, {"spec": spec, "spec_hash": "abc"})))
    features = torch.randn(5, size) * 4
    with torch.no_grad():
        expected = model.policy_logits(model.normalized({"student": features}))
    for row, want in zip(features, expected):
        got = student_ref.logits(exported, row.tolist())
        assert len(got) == 9
        assert max(abs(a - b) for a, b in zip(got, want.tolist())) < 1e-3
        assert student_ref.act(exported, row.tolist()) == int(want.argmax())
