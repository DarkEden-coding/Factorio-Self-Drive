"""Observation shapes and conversions. Grids stay u8 until the model divides by 255."""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
import torch

OBS_KEYS = ("local", "coarse", "vector", "student")

Obs = dict[str, torch.Tensor]


@dataclass(frozen=True)
class ObsShapes:
    """Shapes of the observation parts a model reads. The full observation is `local`, `coarse`
    and `vector`; a student reads only `student`. Absent parts are None."""

    local: tuple[int, int, int] | None = None
    coarse: tuple[int, int, int] | None = None
    vector: int | None = None
    student: int | None = None

    @classmethod
    def from_obs(cls, obs: dict) -> ObsShapes:
        def dims(key: str):
            return tuple(int(d) for d in obs[key].shape[1:]) if key in obs else None

        vector, student = dims("vector"), dims("student")
        return cls(
            local=dims("local"),
            coarse=dims("coarse"),
            vector=vector[0] if vector else None,
            student=student[0] if student else None,
        )

    def only(self, keys: tuple[str, ...]) -> ObsShapes:
        """These shapes with every part not in `keys` dropped."""
        return ObsShapes(**{key: getattr(self, key) if key in keys else None for key in OBS_KEYS})

    def keys(self) -> tuple[str, ...]:
        return tuple(key for key in OBS_KEYS if getattr(self, key) is not None)

    def to_dict(self) -> dict:
        out = {}
        for key in self.keys():
            value = getattr(self, key)
            out[key] = list(value) if isinstance(value, tuple) else value
        return out

    @classmethod
    def from_dict(cls, data: dict) -> ObsShapes:
        return cls(
            local=tuple(data["local"]) if "local" in data else None,
            coarse=tuple(data["coarse"]) if "coarse" in data else None,
            vector=int(data["vector"]) if "vector" in data else None,
            student=int(data["student"]) if "student" in data else None,
        )


def obs_to_torch(obs: dict, device: torch.device) -> Obs:
    """Copy NumPy observations to the device. Always copies: the env reuses its output buffers."""
    dtypes = {"local": np.uint8, "coarse": np.uint8, "vector": np.float32, "student": np.float32}
    return {
        key: torch.from_numpy(np.asarray(obs[key], dtype=dtypes[key])).to(device, copy=True)
        for key in OBS_KEYS
        if key in obs
    }


def index_obs(obs: Obs, index) -> Obs:
    return {key: value[index] for key, value in obs.items()}
