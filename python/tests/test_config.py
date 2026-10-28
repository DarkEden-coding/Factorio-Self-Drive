import json

import pytest

from fsim_rl.config import CONFIG_DIR, ConfigError, load_config
from fsim_rl.env import env_config_json


@pytest.mark.parametrize("kind", ["reactive", "direct_plan", "path_planner"])
def test_configs_inherit_base(kind):
    config = load_config(CONFIG_DIR / f"{kind}.toml")
    assert config.model.kind == kind
    assert config.ppo.num_envs == 128  # from base.toml
    assert config.env.reward.success == 10.0
    assert isinstance(config.env.target.min_distance, float)
    assert len(config.curriculum.stages) == 4


def test_child_overrides_and_lists_replace():
    config = load_config(CONFIG_DIR / "path_planner.toml", {"model": {"torso_hidden": [64]}})
    assert config.ppo.rollout_steps == 64
    assert config.ppo.ent_coef == 0.0
    assert config.ppo.learning_rate == 3e-4
    assert config.model.torso_hidden == [64]


def test_unknown_key_and_bad_reach_rejected():
    with pytest.raises(ConfigError, match="unknown keys"):
        load_config(CONFIG_DIR / "reactive.toml", {"ppo": {"num_env": 4}})
    with pytest.raises(ConfigError, match="max_reach"):
        load_config(CONFIG_DIR / "path_planner.toml", {"model": {"segment_max_length": 20.0}})


def test_env_json_matches_contract_keys():
    config = load_config(CONFIG_DIR / "direct_plan.toml")
    data = json.loads(env_config_json(config, split="holdout"))
    assert data["mode"] == "direct_plan" and data["split"] == "holdout"
    assert data["map_pool"].endswith("data/game/maps")
    assert set(data) == {
        "map_pool", "split", "mode", "decision_ticks", "steer_levels", "gamma", "vehicle_mix", "target",
        "time_limit", "reward", "distance_field", "direct_plan", "path_planner", "scenarios",
        "observation", "student_map", "route_source", "arrival",
    }
    assert data["scenarios"] == [{"kind": "standard", "weight": 1.0}]
    pairs = {(m["vehicle"], m["fuel"]) for m in data["vehicle_mix"]}
    fuels = {"wood", "coal", "solid-fuel", "rocket-fuel", "nuclear-fuel"}
    assert pairs == {(v, f) for v in ("car", "tank") for f in fuels}


@pytest.mark.parametrize("name", ["finetune_moderate", "finetune_strong"])
def test_finetune_configs_mix_scenarios_and_inherit_the_reactive_model(name):
    config = load_config(CONFIG_DIR / f"{name}.toml")
    reactive = load_config(CONFIG_DIR / "reactive.toml")
    assert config.model == reactive.model  # so --init-from reactive checkpoints fits
    kinds = {s["kind"] for s in config.env.scenarios}
    assert kinds == {"standard", "long", "forest", "reverse", "nest"}
    assert config.ppo.learning_rate < reactive.ppo.learning_rate
    assert config.env.reward.collision <= reactive.env.reward.collision < 0
    assert config.curriculum.stages[0].max_distance == 128


def test_strong_penalties_exceed_moderate():
    moderate = load_config(CONFIG_DIR / "finetune_moderate.toml").env.reward
    strong = load_config(CONFIG_DIR / "finetune_strong.toml").env.reward
    for term in ("collision", "nest_zone", "damage"):
        assert getattr(strong, term) < getattr(moderate, term)


def test_scenarios_validated():
    with pytest.raises(ConfigError, match="kind"):
        load_config(CONFIG_DIR / "reactive.toml", {"env": {"scenarios": [{"kind": "warp"}]}})
    with pytest.raises(ConfigError, match="weight"):
        load_config(CONFIG_DIR / "reactive.toml", {"env": {"scenarios": [{"kind": "nest", "weight": 0}]}})


def test_set_overrides_parse_as_toml():
    from fsim_rl.cli import parse_overrides

    assert parse_overrides(["ppo.num_envs=256", "device=cuda", "ppo.learning_rate=5e-5"]) == {
        "ppo": {"num_envs": 256, "learning_rate": 5e-5}, "device": "cuda",
    }


def test_stop_student_config_keeps_the_student_model_and_stops_at_targets():
    config = load_config(CONFIG_DIR / "student_mlp64_stop.toml")
    student = load_config(CONFIG_DIR / "student_mlp64.toml")
    assert config.model == student.model  # so --init-from the mlp64 student fits
    assert config.env.observation == "student"
    assert config.env.arrival.mode == "stop" and student.env.arrival.mode == "enter"
    weights = {s["kind"]: s["weight"] for s in config.env.scenarios}
    assert weights["arrival"] == pytest.approx(0.5)
    assert set(weights) == {"standard", "long", "forest", "reverse", "nest", "arrival"}
    with pytest.raises(ConfigError):
        load_config(CONFIG_DIR / "student_mlp64.toml", {"env": {"arrival": {"mode": "halt"}}})
