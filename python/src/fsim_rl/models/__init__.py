from __future__ import annotations

from ..config import Config
from ..obs import ObsShapes
from .base import ActorCritic, ActResult
from .direct_plan import DirectPlanPolicy
from .path_planner import PathPlannerPolicy
from .reactive import ReactivePolicy
from .student import StudentPolicy

FULL_KEYS = ("local", "coarse", "vector")

__all__ = ["ActorCritic", "ActResult", "build_model"]


def build_model(config: Config, shapes: ObsShapes) -> ActorCritic:
    """`shapes` may list more parts than the model reads (an env returning both observations)."""
    kind = config.model.kind
    if config.model.arch == "student":
        return StudentPolicy(shapes.only(("student",)), config.model)
    shapes = shapes.only(FULL_KEYS)
    if kind == "reactive":
        return ReactivePolicy(shapes, config.model)
    if kind == "direct_plan":
        return DirectPlanPolicy(shapes, config.model, config.env.direct_plan)
    if kind == "path_planner":
        return PathPlannerPolicy(shapes, config.model, config.env.path_planner)
    raise ValueError(f"unknown model kind {kind!r}")
