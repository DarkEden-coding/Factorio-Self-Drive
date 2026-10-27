import asyncio
import base64
import json

import numpy as np
import pytest
import torch
from conftest import SHAPES, tiny_config
from websockets.asyncio.client import connect

from fsim_rl.checkpoint import checkpoint_state, load_policy, save_checkpoint
from fsim_rl.curriculum import Curriculum
from fsim_rl.models import build_model
from fsim_rl.server import PolicyServer, run_server


def act_message(rng) -> str:
    def grid(shape):
        return base64.b64encode(rng.integers(0, 256, shape, dtype=np.uint8).tobytes()).decode()

    return json.dumps({
        "type": "act",
        "local": grid(SHAPES.local),
        "coarse": grid(SHAPES.coarse),
        "vector": rng.normal(size=SHAPES.vector).tolist(),
    })


async def round_trip(server: PolicyServer) -> tuple[dict, dict, dict]:
    ready = asyncio.get_running_loop().create_future()
    task = asyncio.create_task(run_server(server, "127.0.0.1", 0, ready))
    port = await ready
    rng = np.random.default_rng(0)
    async with connect(f"ws://127.0.0.1:{port}") as ws:
        hello = json.loads(await ws.recv())
        await ws.send(act_message(rng))
        action = json.loads(await ws.recv())
        await ws.send(json.dumps({"type": "act", "local": "", "coarse": "", "vector": []}))
        error = json.loads(await ws.recv())
    task.cancel()
    return hello, action, error


@pytest.mark.parametrize("kind", ["reactive", "direct_plan", "path_planner"])
def test_server_round_trip_from_checkpoint(tmp_path, kind):
    config = tiny_config(kind)
    model = build_model(config, SHAPES)
    optimizer = torch.optim.Adam(model.parameters())
    curriculum = Curriculum(config.curriculum, config.env.target)
    path = tmp_path / "ckpt.pt"
    save_checkpoint(path, checkpoint_state(model, optimizer, config, {"spec_hash": "abc"},
                                           curriculum.state_dict(), 0, 0))
    loaded, loaded_config, state = load_policy(path, torch.device("cpu"))
    server = PolicyServer(loaded, loaded_config, state["spec_hash"])

    hello, action, error = asyncio.run(round_trip(server))

    assert hello["type"] == "hello" and hello["mode"] == kind and hello["spec_hash"] == "abc"
    assert hello["decision_ticks"] == 4
    assert action["type"] == "action" and isinstance(action["value"], float)
    if kind == "reactive":
        assert isinstance(action["action"], int) and len(action["probs"]) == 9
    elif kind == "direct_plan":
        assert len(action["action"]) == 24 and hello["mode_config"]["execute"] == 4
    else:
        assert np.asarray(action["action"]).shape == (8, 2) and action["probs"] is None
    assert error["type"] == "error"


def test_server_round_trip_with_a_student():
    from fsim_rl.config import CONFIG_DIR, load_config
    from fsim_rl.obs import ObsShapes

    config = load_config(CONFIG_DIR / "student_mlp64.toml", {"device": "cpu"})
    model = build_model(config, ObsShapes(student=135))
    server = PolicyServer(model, config, "student-hash")

    async def run() -> tuple[dict, dict, dict]:
        ready = asyncio.get_running_loop().create_future()
        task = asyncio.create_task(run_server(server, "127.0.0.1", 0, ready))
        port = await ready
        async with connect(f"ws://127.0.0.1:{port}") as ws:
            hello = json.loads(await ws.recv())
            await ws.send(json.dumps({"type": "act", "student": [0.1] * 135}))
            action = json.loads(await ws.recv())
            await ws.send(json.dumps({"type": "act", "student": [0.1] * 10}))
            error = json.loads(await ws.recv())
        task.cancel()
        return hello, action, error

    hello, action, error = asyncio.run(run())
    assert hello["observation"] == "student" and hello["spec_hash"] == "student-hash"
    assert isinstance(action["action"], int) and len(action["probs"]) == 9
    assert error["type"] == "error"
