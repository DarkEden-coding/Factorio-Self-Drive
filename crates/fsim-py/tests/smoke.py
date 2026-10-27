"""Smoke test for the fsim module on the real map pool.

Builds a VecEnv per action mode, steps random and baseline actions, and checks
shapes, dtypes, auto-reset outputs, records and errors. Run from anywhere:

    python crates/fsim-py/tests/smoke.py
"""
import json
from pathlib import Path
import resource
import time

import numpy as np

import fsim

ROOT = Path(__file__).resolve().parents[3]
POOL = ROOT / "data/game/maps"
N = 16


def config(mode, split="train"):
    return json.dumps({
        "map_pool": str(POOL), "split": split, "mode": mode,
        "vehicle_mix": [{"vehicle": "car", "fuel": "coal", "weight": 1.0},
                        {"vehicle": "tank", "fuel": "nuclear-fuel", "weight": 1.0}],
        "target": {"min_distance": 16, "max_distance": 48, "max_tortuosity": 1.5, "radius": 3},
    })


def check_obs(obs, spec, n=N, prefix=""):
    channels = len(spec["channels"])
    for key, shape, dtype in [("local", (n, channels, 64, 64), np.uint8),
                              ("coarse", (n, channels, 32, 32), np.uint8),
                              ("vector", (n, spec["vector_size"]), np.float32)]:
        array = obs[prefix + key]
        assert array.shape == shape and array.dtype == dtype, (key, array.shape, array.dtype)
    assert np.isfinite(obs[prefix + "vector"]).all()


def random_actions(env, rng):
    if env.mode == "reactive":
        return rng.integers(0, 9, size=N)
    if env.mode == "direct_plan":
        return rng.integers(0, 9, size=(N, 24))
    ahead = np.arange(1, 9, dtype=np.float32) * 4
    right = rng.uniform(-0.5, 0.5, size=(N, 8)).astype(np.float32) * ahead
    return np.stack([right, np.broadcast_to(ahead, (N, 8))], axis=-1)


def main():
    spec = fsim.spec()
    assert len(spec["spec_hash"]) == 16
    assert spec["local_shape"] == [8, 64, 64] and spec["coarse_shape"] == [8, 32, 32]
    assert spec["vector_size"] == len(spec["vector_features"])
    assert spec["vehicles"] == ["car", "tank"] and "nuclear-fuel" in spec["fuels"]
    terms = spec["reward_terms"]
    rng = np.random.default_rng(0)
    for mode in ["reactive", "direct_plan", "path_planner"]:
        started = time.perf_counter()
        env = fsim.VecEnv(config(mode), N, 7)
        load = time.perf_counter() - started
        assert env.num_envs == N and env.mode == mode
        obs = env.reset()
        check_obs(obs, spec)
        env.set_recording(True)
        done_total, decisions, steps = 0, 0, 0
        started = time.perf_counter()
        for step in range(300):
            actions = env.baseline_actions() if step % 2 else random_actions(env, rng)
            obs, reward, discount, terminated, truncated, info = env.step(actions)
            steps += 1
            check_obs(obs, spec)
            check_obs(info, spec, prefix="final_")
            assert reward.shape == (N,) and reward.dtype == np.float32
            assert discount.dtype == np.float32 and ((discount > 0) & (discount <= 1)).all()
            assert terminated.dtype == np.bool_ and truncated.dtype == np.bool_
            assert not (terminated & truncated).any()
            for key in ["success", "destroyed", "timeout", "stalled"]:
                assert info[key].dtype == np.bool_ and info[key].shape == (N,)
            assert (info["success"] | info["destroyed"] == terminated).all()
            assert (info["timeout"] | info["stalled"] == truncated).all()
            for key in ["ticks", "collisions", "map_index", "vehicle_index", "fuel_index", "decisions"]:
                assert info[key].dtype == np.int64 and info[key].shape == (N,)
            for key in ["path_distance", "optimal_distance", "path_length", "damage", "x", "y", "orientation"]:
                assert info[key].dtype == np.float32 and info[key].shape == (N,)
            assert set(np.unique(info["vehicle_index"])) <= {spec["vehicles"].index("car"),
                                                             spec["vehicles"].index("tank")}
            assert (info["path_length"] >= 0).all()
            total = sum(info[f"reward_{t}"] for t in terms)
            assert np.allclose(total, reward, atol=1e-4)
            done = terminated | truncated
            if done.any():
                # Final observations are filled for finished envs only.
                assert info["final_local"][done].any()
            if mode == "reactive":
                assert np.allclose(discount, 0.99)
            done_total += int(done.sum())
            decisions += int(info["decisions"].sum())
        elapsed = time.perf_counter() - started
        records = env.episode_records()
        assert len(records) == done_total, (len(records), done_total)
        for record in records:
            assert record["mode"] == mode and record["decision_ticks"] == 4
            assert 0 <= record["env_index"] < N and record["result"]["path_length"] >= 0
            assert len(record["actions"]) == record["result"]["decisions"]
            assert record["overlays"] and record["overlays"][0]["decision"] == 0
            if mode == "path_planner":
                assert all("waypoints" in o for o in record["overlays"])
        assert env.episode_records() == []
        try:
            env.step(np.zeros((N + 1, 3)))
            raise AssertionError("bad action shape accepted")
        except ValueError:
            pass
        print(f"{mode:13} load {load:.2f}s, {steps} steps, {decisions / elapsed:,.0f} decisions/s "
              f"(incl. baseline), {done_total} episodes")
    env = fsim.VecEnv(config("reactive", split="holdout"), 4, 1)
    env.set_curriculum(32, 96, 1.3)
    assert len(env.map_names) == 8
    try:
        env.set_curriculum(10, 5, 1.0)
        raise AssertionError("invalid curriculum accepted")
    except ValueError:
        pass
    try:
        fsim.VecEnv(json.dumps({"unknown": 1}), 1, 0)
        raise AssertionError("unknown config key accepted")
    except ValueError:
        pass

    # Throughput of pure random reactive stepping from Python at N = 64.
    env = fsim.VecEnv(config("reactive"), 64, 3)
    env.reset()
    actions = rng.integers(0, 9, size=(200, 64))
    started = time.perf_counter()
    for a in actions:
        env.step(a)
    rate = actions.size / (time.perf_counter() - started)
    rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 2**30
    print(f"reactive N=64 from Python: {rate:,.0f} decisions/s; peak RSS {rss:.2f} GiB")
    print("fsim smoke test passed")


if __name__ == "__main__":
    main()
