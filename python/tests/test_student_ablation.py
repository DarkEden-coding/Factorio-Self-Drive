"""Ablations must change only the intended policy inputs."""

import importlib.util
from pathlib import Path

import torch

script = Path(__file__).resolve().parents[2] / "scripts/student-ablation.py"
spec = importlib.util.spec_from_file_location("student_ablation", script)
ablation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ablation)

FEATURES = [
    *(f"route{s}_{axis}" for s in (4, 8, 16, 32) for axis in ("right", "forward")),
    "route_length", "target_dir_right", "target_dir_forward", "straight_distance", "speed",
]


class CapturePolicy:
    def act(self, obs, deterministic=False):
        self.obs = obs
        self.deterministic = deterministic
        return obs["student"]


def test_straight_route_keeps_map_state_and_original_observation():
    model = CapturePolicy()
    # A target 8 tiles straight ahead: nearer lookaheads reach their full length,
    # farther ones end at the target. The appended map must remain untouched.
    x = torch.full((1, len(FEATURES) + 6), 0.25)
    x[0, 9:12] = torch.tensor([0.0, 1.0, 8 / 64])
    before = x.clone()
    other = torch.ones(1, 2)
    result = ablation.StraightRoute(model, FEATURES).act(
        {"student": x, "vector": other}, deterministic=True,
    )
    assert torch.equal(result[0, :8], torch.tensor([0., 1., 0., 1., 0., 0.5, 0., 0.25]))
    assert result[0, 8] == 8 / 64
    assert torch.equal(result[:, 9:], before[:, 9:])
    assert torch.equal(x, before)
    assert model.obs["vector"] is other
    assert model.deterministic


def test_no_map_keeps_rays_route_and_state():
    model = CapturePolicy()
    x = torch.ones(2, len(FEATURES) + 6)
    result = ablation.AblatedStudent(model, FEATURES, "no_map").act({"student": x})
    assert torch.equal(result[:, :len(FEATURES)], x[:, :len(FEATURES)])
    assert torch.count_nonzero(result[:, len(FEATURES):]) == 0
    assert torch.all(x == 1)
