import sys
from pathlib import Path

import pytest
import torch

sys.path.insert(0, str(Path(__file__).parent))

from fsim_rl.config import CONFIG_DIR, load_config  # noqa: E402
from fsim_rl.obs import ObsShapes  # noqa: E402

SHAPES = ObsShapes(local=(8, 64, 64), coarse=(8, 32, 32), vector=12)


@pytest.fixture
def shapes() -> ObsShapes:
    return SHAPES


def tiny_config(kind: str, **ppo):
    """A real config file with a small model and rollout, on CPU."""
    overrides = {
        "device": "cpu",
        "model": {"local_cnn": [8, 8], "coarse_cnn": [8], "local_features": 32,
                  "coarse_features": 16, "vector_hidden": 16, "torso_hidden": [32]},
        "ppo": {"num_envs": 4, "rollout_steps": 8, "epochs": 2, "num_minibatches": 2, **ppo},
    }
    return load_config(CONFIG_DIR / f"{kind}.toml", overrides)


def random_obs(batch: int, shapes: ObsShapes = SHAPES) -> dict:
    return {
        "local": torch.randint(0, 256, (batch, *shapes.local), dtype=torch.uint8),
        "coarse": torch.randint(0, 256, (batch, *shapes.coarse), dtype=torch.uint8),
        "vector": torch.randn(batch, shapes.vector),
    }
