import math

import numpy as np
import pytest
import torch
import torch.nn.functional as F
from conftest import SHAPES, random_obs, tiny_config

from fsim_rl.models import build_model
from fsim_rl.models.direct_plan import plan_consistency_targets
from fsim_rl.models.distributions import TanhGaussian
from fsim_rl.models.path_planner import segments_to_waypoints


@pytest.mark.parametrize(
    "kind, action_shape, env_shape, env_dtype",
    [
        ("reactive", (5,), (5,), np.int64),
        ("direct_plan", (5, 24), (5, 24), np.int64),
        ("path_planner", (5, 16), (5, 8, 2), np.float32),
    ],
)
def test_output_shapes(kind, action_shape, env_shape, env_dtype):
    model = build_model(tiny_config(kind), SHAPES)
    obs = random_obs(5)
    result = model.act(obs)
    assert result.action.shape == action_shape
    assert result.log_prob.shape == (5,) and result.value.shape == (5,)
    env_action = model.env_action(result.action)
    assert env_action.shape == env_shape and env_action.dtype == env_dtype
    log_prob, entropy, value, _ = model.evaluate_actions(obs, result.action)
    assert torch.allclose(log_prob, result.log_prob, atol=1e-5)
    assert entropy.shape == (5,) and value.shape == (5,)
    assert len(model.overlays(result)) == 5


def test_reactive_and_plan_log_probs():
    obs = random_obs(3)
    model = build_model(tiny_config("reactive"), SHAPES)
    dist, _ = model(obs)
    action = torch.tensor([0, 4, 8])
    expected = F.log_softmax(model.logits(model.encoder(obs)), -1)[torch.arange(3), action]
    assert torch.allclose(dist.log_prob(action), expected, atol=1e-6)

    model = build_model(tiny_config("direct_plan"), SHAPES)
    dist, _ = model(obs)
    plan = torch.randint(0, 9, (3, 24))
    log_softmax = F.log_softmax(dist.logits, -1).gather(-1, plan.unsqueeze(-1)).squeeze(-1)
    # only the 4 executed steps count
    assert torch.allclose(dist.log_prob(plan), log_softmax[:, :4].sum(-1), atol=1e-5)


def test_squashed_gaussian_density_matches_numerical():
    mean, log_std = torch.tensor([0.3], dtype=torch.float64), torch.tensor([math.log(0.8)], dtype=torch.float64)
    dist = TanhGaussian(mean, log_std)
    normal = torch.distributions.Normal(mean, log_std.exp())
    da = 1e-5
    for a in [-0.95, -0.4, 0.0, 0.5, 0.97]:
        lo, hi = torch.tensor([a - da / 2], dtype=torch.float64), torch.tensor([a + da / 2], dtype=torch.float64)
        numeric = (normal.cdf(torch.atanh(hi)) - normal.cdf(torch.atanh(lo))) / da
        analytic = dist.log_prob(torch.atanh(torch.tensor([a], dtype=torch.float64))).exp()
        assert analytic.item() == pytest.approx(numeric.item(), rel=1e-4)
    # stable far into the tails
    assert torch.isfinite(dist.log_prob(torch.tensor([30.0], dtype=torch.float64))).all()


def test_waypoints_respect_reach_and_heading():
    squashed = torch.ones(1, 8, 2) * torch.tensor([1.0, 0.0])  # max length, straight
    points = segments_to_waypoints(squashed, 1.0, 12.0, 0.6)
    assert torch.allclose(points[0, -1], torch.tensor([0.0, 96.0]))
    turn_right = segments_to_waypoints(torch.tensor([[[1.0, 1.0]]]), 1.0, 12.0, 0.6)
    assert turn_right[0, 0, 0] > 0  # positive heading change goes right
    extreme = segments_to_waypoints(torch.rand(64, 8, 2) * 2 - 1, 1.0, 12.0, 0.6)
    assert (extreme.norm(dim=-1) <= 96.0 + 1e-4).all()


def test_plan_consistency_targets():
    # T=3 steps, N=1 env, H=4, execute=2. An episode ends at step 1.
    actions = torch.tensor([[[1, 2, 3, 4]], [[5, 6, 7, 8]], [[0, 1, 2, 3]]])
    dones = torch.tensor([[False], [True], [False]])
    executed = torch.tensor([[2], [1], [2]])
    targets, mask = plan_consistency_targets(actions, dones, executed, execute=2)
    # step 0, plan steps 2,3 -> step 1 decisions 0,1; decision 1 never ran (executed 1)
    assert mask[0, 0].tolist() == [False, False, True, False]
    assert targets[0, 0, 2] == 5
    # step 1 ended its episode; step 2 is out of the rollout for step 2
    assert not mask[1].any() and not mask[2].any()
