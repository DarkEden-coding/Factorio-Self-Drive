"""Checks against the real `fsim` extension. Skipped when it is not built."""

import numpy as np
import pytest
import torch
from conftest import tiny_config

fsim = pytest.importorskip("fsim")

from fsim_rl.env import FsimEnv  # noqa: E402
from fsim_rl.trainer import Trainer  # noqa: E402


def test_spec_has_contract_fields():
    spec = fsim.spec()
    assert spec["spec_hash"]
    assert len(spec["vector_features"]) > 0
    assert spec["scenarios"] == ["standard", "long", "forest", "reverse", "nest", "arrival"]


def test_spec_hash_still_matches_reactive_checkpoints():
    # reactive-1 was trained before the scenario sampler; fine-tuning depends on this hash.
    assert fsim.spec()["spec_hash"] == "16a3d4353d1658c1"


@pytest.mark.parametrize("kind", ["reactive", "direct_plan", "path_planner"])
def test_vec_env_step_shapes(kind):
    config = tiny_config(kind)
    env = FsimEnv(config, num_envs=2, seed=0, device=torch.device("cpu"))
    obs = env.reset()
    assert obs["local"].shape == (2, 8, 64, 64) and obs["local"].dtype == torch.uint8
    assert obs["coarse"].shape == (2, 8, 32, 32)
    assert obs["vector"].shape == (2, len(env.spec["vector_features"]))
    actions = np.asarray(env.baseline_actions())
    step = env.step(actions)
    assert step.reward.shape == (2,) and step.discount.shape == (2,)
    assert ((step.discount > 0) & (step.discount <= 1)).all()
    for key in ("success", "destroyed", "path_distance", "optimal_distance", "reward_progress",
                "optimal_length", "scenario", "collisions_tree", "collisions_water",
                "nest_zone_ticks", "nest_zone_decisions", "reverse_distance", "reversed"):
        assert step.info[key].shape == (2,)


def test_scenario_mix_runs_on_the_real_pool():
    config = tiny_config("reactive")
    config.env.scenarios = [{"kind": kind, "weight": 1.0} for kind in fsim.spec()["scenarios"]]
    env = FsimEnv(config, num_envs=8, seed=3, device=torch.device("cpu"))
    env.reset()
    seen = set()
    for _ in range(40):
        step = env.step(np.asarray(env.baseline_actions()))
        seen.update(step.info["scenario"].tolist())
    assert seen <= set(range(6)) and len(seen) >= 2


def test_one_training_iteration(tmp_path):
    trainer = Trainer(tiny_config("reactive"), tmp_path / "run")
    checkpoint = trainer.train(iterations=1)
    assert checkpoint.exists()


def test_student_observation_shapes_and_determinism():
    spec = fsim.spec()
    assert spec["spec_hash"] == "16a3d4353d1658c1"
    size = spec["student"]["size"]
    assert len(spec["student"]["features"]) == size
    config = tiny_config("reactive")

    def rollout(observation):
        config.env.observation = observation
        env = FsimEnv(config, num_envs=3, seed=4, device=torch.device("cpu"))
        trace = [env.reset()]
        for _ in range(30):
            step = env.step(np.asarray(env.baseline_actions()))
            trace.append(step.obs)
        return trace

    student = rollout("student")
    both = rollout("both")
    assert set(student[0]) == {"student"}
    assert set(both[0]) == {"local", "coarse", "vector", "student"}
    assert student[0]["student"].shape == (3, size)
    for a, b in zip(student, both):
        assert torch.equal(a["student"], b["student"])
    assert torch.equal(rollout("student")[-1]["student"], student[-1]["student"])


def test_student_trains_and_distills_on_the_real_env(tmp_path):
    from fsim_rl.config import CONFIG_DIR, load_config, with_overrides
    from fsim_rl.distill import Distiller

    teacher = Trainer(tiny_config("reactive"), tmp_path / "teacher").train(iterations=1)
    config = load_config(CONFIG_DIR / "student_mlp32.toml", {
        "device": "cpu",
        "ppo": {"num_envs": 4, "rollout_steps": 8, "epochs": 1, "num_minibatches": 2},
        "distill": {"num_envs": 4, "rollout_steps": 8, "iterations": 2, "beta_iterations": 1,
                    "buffer_size": 256, "minibatch": 16, "updates_per_iteration": 2},
    })
    best = Distiller(config, teacher, tmp_path / "distill").run()
    trainer = Trainer(config, tmp_path / "rl", init_from=best)
    assert trainer.train(iterations=1).exists()
    regularized = with_overrides(config, {"ppo": {"teacher_kl_coef": 0.1,
                                                  "teacher_checkpoint": str(teacher)}})
    trainer = Trainer(regularized, tmp_path / "rl-kl", init_from=best)
    assert trainer.train(iterations=1).exists()
