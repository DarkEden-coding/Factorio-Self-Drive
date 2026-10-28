"""WebSocket policy server for the viewer (docs/rl.md "Policy server")."""

from __future__ import annotations

import asyncio
import base64
import dataclasses
import json

import numpy as np
import torch
from websockets.asyncio.server import ServerConnection, serve

from .config import Config
from .models import ActorCritic


class PolicyServer:
    def __init__(
        self,
        model: ActorCritic,
        config: Config,
        spec_hash: str,
        deterministic: bool = True,
        device: torch.device = torch.device("cpu"),
    ):
        self.model = model.to(device).eval()
        self.config = config
        self.spec_hash = spec_hash
        self.deterministic = deterministic
        self.device = device

    @property
    def observation(self) -> str:
        return "student" if self.model.shapes.student is not None else "full"

    def hello(self) -> dict:
        kind = self.config.model.kind
        mode_config = {}
        if kind == "direct_plan":
            mode_config = dataclasses.asdict(self.config.env.direct_plan)
        elif kind == "path_planner":
            mode_config = dataclasses.asdict(self.config.env.path_planner)
        return {
            "type": "hello",
            "mode": kind,
            "decision_ticks": self.config.env.decision_ticks,
            "spec_hash": self.spec_hash,
            "observation": self.observation,
            "route_source": self.config.env.route_source,
            "field": dataclasses.asdict(self.config.env.distance_field),
            "model": {"kind": kind, "deterministic": self.deterministic, "arch": self.config.model.arch},
            "mode_config": mode_config,
            "arrival": dataclasses.asdict(self.config.env.arrival),
        }

    def _decode(self, message: dict) -> dict[str, torch.Tensor]:
        shapes = self.model.shapes
        if self.observation == "student":
            student = np.asarray(message["student"], dtype=np.float32)
            if student.size != shapes.student:
                raise ValueError(f"student: expected {shapes.student} values, got {student.size}")
            return {"student": torch.from_numpy(student.reshape(1, -1)).to(self.device)}

        def grid(key: str, shape: tuple[int, ...]) -> torch.Tensor:
            raw = np.frombuffer(base64.b64decode(message[key]), dtype=np.uint8)
            if raw.size != int(np.prod(shape)):
                raise ValueError(f"{key}: expected {int(np.prod(shape))} bytes, got {raw.size}")
            return torch.from_numpy(raw.reshape(1, *shape).copy()).to(self.device)

        vector = np.asarray(message["vector"], dtype=np.float32)
        if vector.size != shapes.vector:
            raise ValueError(f"vector: expected {shapes.vector} values, got {vector.size}")
        return {
            "local": grid("local", shapes.local),
            "coarse": grid("coarse", shapes.coarse),
            "vector": torch.from_numpy(vector.reshape(1, -1)).to(self.device),
        }

    def act(self, message: dict) -> dict:
        obs = self._decode(message)
        result = self.model.act(obs, deterministic=self.deterministic)
        action = self.model.env_action(result.action)[0].tolist()
        overlay = self.model.overlays(result)[0]
        return {
            "type": "action",
            "action": action,
            "probs": overlay.get("probs"),
            "value": float(result.value[0]),
        }

    def handle(self, raw: str | bytes) -> dict:
        try:
            message = json.loads(raw)
            if message.get("type") != "act":
                raise ValueError(f"unknown message type {message.get('type')!r}")
            return self.act(message)
        except (ValueError, KeyError, TypeError) as exc:
            return {"type": "error", "message": str(exc)}


async def _connection(server: PolicyServer, websocket: ServerConnection) -> None:
    await websocket.send(json.dumps(server.hello()))
    async for raw in websocket:
        await websocket.send(json.dumps(server.handle(raw)))


async def run_server(server: PolicyServer, host: str, port: int, ready: asyncio.Future | None = None) -> None:
    async with serve(lambda ws: _connection(server, ws), host, port, max_size=2**24) as ws_server:
        if ready is not None:
            ready.set_result(ws_server.sockets[0].getsockname()[1])
        else:
            print(f"policy server on ws://{host}:{port} ({server.config.model.kind}, spec_hash {server.spec_hash})")
        await asyncio.Future()
