import torch
from conftest import SHAPES, random_obs, tiny_config

from fsim_rl.models import build_model
from fsim_rl.ppo import ppo_loss, ppo_update
from fsim_rl.rollout import compute_gae


def test_gae_with_variable_discounts():
    rewards = torch.tensor([[1.0], [2.0], [3.0]])
    discounts = torch.tensor([[0.9], [0.81], [0.9]])
    dones = torch.tensor([[False], [True], [False]])
    values = torch.tensor([[0.5], [1.0], [2.0]])
    adv, ret = compute_gae(rewards, discounts, dones, values, torch.tensor([4.0]), lam=0.5)
    # t2: 3 + 0.9*4 - 2 = 4.6; t1 (done): 2 - 1 = 1; t0: (1 + 0.9*1 - 0.5) + 0.9*0.5*1 = 1.85
    assert torch.allclose(adv[:, 0], torch.tensor([1.85, 1.0, 4.6]))
    assert torch.allclose(ret[:, 0], torch.tensor([2.35, 2.0, 6.6]))


def test_ppo_update_reduces_loss():
    torch.manual_seed(0)
    config = tiny_config("reactive", epochs=4, num_minibatches=1, target_kl=0.0, clip_value=0.0,
                         learning_rate=1e-3, ent_coef=0.0)
    model = build_model(config, SHAPES)
    obs = random_obs(64)
    with torch.no_grad():
        result = model.act(obs)
    adv = torch.where(result.action == 0, 1.0, -1.0)
    batch = {
        "obs": obs, "actions": result.action, "log_probs": result.log_prob,
        "values": result.value, "advantages": adv, "returns": torch.zeros(64),
    }
    before, _ = ppo_loss(model, batch, config.ppo)
    p0_before = model(obs)[0].probs[:, 0].mean()
    optimizer = torch.optim.Adam(model.parameters(), lr=config.ppo.learning_rate)
    stats = ppo_update(model, optimizer, batch, config.ppo)
    after, _ = ppo_loss(model, batch, config.ppo)
    assert after.item() < before.item()
    assert model(obs)[0].probs[:, 0].mean() > p0_before
    assert stats["loss/epochs"] == 4
