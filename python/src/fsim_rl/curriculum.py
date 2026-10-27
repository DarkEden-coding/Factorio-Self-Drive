"""Target-distance curriculum: advance a stage when the rolling success rate is high enough."""

from __future__ import annotations

from collections import deque

from .config import CurriculumConfig, CurriculumStage, TargetConfig


class Curriculum:
    def __init__(self, cfg: CurriculumConfig, target: TargetConfig):
        self.cfg = cfg
        self.active = cfg.enabled and bool(cfg.stages)
        self.stages = cfg.stages if self.active else [
            CurriculumStage(target.min_distance, target.max_distance, target.max_tortuosity)
        ]
        self.stage_index = 0
        self.outcomes: deque[bool] = deque(maxlen=cfg.window)

    @property
    def stage(self) -> CurriculumStage:
        return self.stages[self.stage_index]

    def record(self, successes) -> bool:
        """Add finished-episode outcomes. Returns True when the stage advanced."""
        self.outcomes.extend(bool(s) for s in successes)
        if not self.active or self.stage_index + 1 >= len(self.stages):
            return False
        if len(self.outcomes) < max(self.cfg.min_episodes, 1):
            return False
        if self.success_rate() < self.cfg.success_threshold:
            return False
        self.stage_index += 1
        self.outcomes.clear()
        return True

    def success_rate(self) -> float:
        return sum(self.outcomes) / len(self.outcomes) if self.outcomes else float("nan")

    def state_dict(self) -> dict:
        return {"stage_index": self.stage_index, "outcomes": list(self.outcomes)}

    def load_state_dict(self, state: dict) -> None:
        self.stage_index = min(int(state["stage_index"]), len(self.stages) - 1)
        self.outcomes.clear()
        self.outcomes.extend(state["outcomes"])
