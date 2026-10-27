"""Train, checkpoint, resume and evaluate end to end against the fake fsim backend."""

import json

import numpy as np
import pytest
import torch
from conftest import tiny_config

import fake_fsim
from fsim_rl.checkpoint import load_checkpoint, save_checkpoint
from fsim_rl.evaluate import evaluate, evaluate_checkpoint, suite, suite_table
from fsim_rl.trainer import Trainer


@pytest.mark.parametrize("kind", ["reactive", "direct_plan", "path_planner"])
def test_train_resume_eval(tmp_path, kind):
    config = tiny_config(kind)
    run_dir = tmp_path / "run"
    trainer = Trainer(config, run_dir, backend=fake_fsim)
    checkpoint = trainer.train(iterations=2)
    assert trainer.env.env.curriculum == (8.0, 24.0, 1.2)  # stage 0 applied

    lines = (run_dir / "metrics.csv").read_text().splitlines()
    assert len(lines) == 3 and "loss/policy" in lines[0] and "reward/progress" in lines[0]
    assert (run_dir / "tb").is_dir()

    resumed = Trainer(config, run_dir, resume=checkpoint, backend=fake_fsim)
    assert resumed.iteration == 2 and resumed.global_step == 2 * 4 * 8
    resumed.train(iterations=1)
    assert len((run_dir / "metrics.csv").read_text().splitlines()) == 4

    replays = tmp_path / "replays.json"
    report = evaluate_checkpoint(checkpoint, "holdout", episodes=5, num_envs=2, device="cpu",
                                 replays=replays, backend=fake_fsim)
    assert report["episodes"] == 5 and 0.0 <= report["success_rate"] <= 1.0
    mix = {f"{m.vehicle}/{m.fuel}" for m in config.env.vehicle_mix}
    assert report["per_vehicle"] and set(report["per_vehicle"]) <= mix
    records = json.loads(replays.read_text())
    assert len(records) == 5
    overlay = records[0]["overlays"][0]
    assert overlay["decision"] == 0 and "value" in overlay
    if kind == "path_planner":
        # first call converts with the record's start pose (facing east): forward is +x
        points = np.asarray(overlay["waypoints"])
        assert points[:, 0].min() > 0
    if kind == "direct_plan":
        decisions = [o["decision"] for o in records[0]["overlays"]]
        assert decisions == list(range(0, 4 * len(decisions), 4))


def test_baseline_eval():
    config = tiny_config("reactive")
    report = evaluate(config, "holdout", episodes=3, num_envs=3, seed=0,
                      device=torch.device("cpu"), backend=fake_fsim)
    assert report["policy"] == "baseline" and report["episodes"] == 3


def test_init_from_copies_weights_with_a_fresh_optimizer(tmp_path):
    config = tiny_config("reactive")
    checkpoint = Trainer(config, tmp_path / "a", backend=fake_fsim).train(iterations=2)
    state = load_checkpoint(checkpoint)
    tuned = Trainer(config, tmp_path / "b", init_from=checkpoint, backend=fake_fsim)
    for key, value in state["model"].items():
        assert torch.equal(tuned.model.state_dict()[key].cpu(), value), key
    assert any("vector_norm" in key for key in state["model"])  # normalizer travels with it
    assert tuned.iteration == 0 and tuned.global_step == 0
    assert not tuned.optimizer.state_dict()["state"]
    assert json.loads((tmp_path / "b" / "init_from.json").read_text())["iteration"] == 2

    with pytest.raises(SystemExit, match="model kind"):
        Trainer(tiny_config("direct_plan"), tmp_path / "c", init_from=checkpoint, backend=fake_fsim)
    state["spec_hash"] = "other"
    save_checkpoint(tmp_path / "other.pt", state)
    with pytest.raises(SystemExit, match="spec_hash"):
        Trainer(config, tmp_path / "d", init_from=tmp_path / "other.pt", backend=fake_fsim)


def test_training_logs_per_scenario_metrics(tmp_path):
    config = tiny_config("reactive")
    config.env.scenarios = [{"kind": "standard", "weight": 1.0}, {"kind": "nest", "weight": 1.0}]
    Trainer(config, tmp_path / "run", backend=fake_fsim).train(iterations=1)
    header = (tmp_path / "run" / "metrics.csv").read_text().splitlines()[0].split(",")
    for key in ("scenario/nest/success_rate", "scenario/reverse/count", "episode/collisions_tree",
                "episode/nest_zone_fraction", "episode/reversed_rate", "episode/spl"):
        assert key in header


def test_suite_covers_every_scenario_for_model_and_baseline(tmp_path):
    config = tiny_config("reactive")
    checkpoint = Trainer(config, tmp_path / "run", backend=fake_fsim).train(iterations=1)
    from fsim_rl.checkpoint import load_policy

    model, config, state = load_policy(checkpoint, torch.device("cpu"))
    report = suite(config, "holdout", episodes=4, num_envs=2, seed=0, device=torch.device("cpu"),
                   model=model, spec_hash=state["spec_hash"], backend=fake_fsim)
    assert list(report["rows"]) == ["standard", "long", "forest", "reverse", "nest", "arrival"]
    for kind, row in report["rows"].items():
        assert set(row) == {"model", "baseline"}
        assert row["model"]["episodes"] == 4
        assert set(row["model"]["per_scenario"]) == {kind}
        assert set(row["model"]["collisions_per_episode"]) == {"tree", "rock", "cliff", "nest", "water"}
    table = suite_table(report).splitlines()
    assert len(table) == 2 + 12 and "tree coll/ep" in table[0]
