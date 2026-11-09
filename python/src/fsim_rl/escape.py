"""Escape rule for blocked poses, applied around a deterministic student at inference.

A vehicle pressed against a cliff end or rock can map to an action that does not move it, and
the argmax policy then repeats that action until the stall limit. When the last `after`
decisions all pushed (throttle not coasting) and each moved the vehicle less than `MOVED`
tiles, the rule overrides the next `hold` decisions with the opposite throttle and the
opposite steering of the blocked action. The mod's driver (`escape.lua`) and
`scripts/autodrive-sim.mjs` apply the same rule; keep them in step.
"""

from __future__ import annotations

import numpy as np

AFTER = 6
HOLD = 4
MOVED = 0.02


def mirror(action: int) -> int:
    """Opposite throttle and opposite steering of a 9-action index."""
    throttle, steer = action // 3 - 1, action % 3 - 1
    return (1 - throttle) * 3 + (1 - steer)


class Escape:
    """Per-env escape state for a batch of `n` episodes."""

    def __init__(self, n: int, after: int = AFTER, hold: int = HOLD):
        self.after, self.hold = after, hold
        self.blocked = np.zeros(n, dtype=np.int64)
        self.left = np.zeros(n, dtype=np.int64)
        self.action = np.zeros(n, dtype=np.int64)
        self.last = np.full(n, 4, dtype=np.int64)
        self.xy: np.ndarray | None = None

    def apply(self, actions: np.ndarray) -> np.ndarray:
        """The actions to execute; marks rows it overrides in `self.active`."""
        actions = np.array(actions, dtype=np.int64)
        start = (self.blocked >= self.after) & (self.left == 0)
        for i in np.flatnonzero(start):
            self.action[i], self.left[i], self.blocked[i] = mirror(int(self.last[i])), self.hold, 0
        self.active = self.left > 0
        actions[self.active] = self.action[self.active]
        self.left[self.active] -= 1
        self.last = actions
        return actions

    def observe(self, x, y, done) -> None:
        """Record the poses after the executed decision; `done` rows start a new episode."""
        xy = np.stack([np.asarray(x, dtype=np.float64), np.asarray(y, dtype=np.float64)], 1)
        done = np.asarray(done, dtype=bool)
        if self.xy is not None:
            moved = np.hypot(*(xy - self.xy).T)
            pushing = self.last // 3 != 1
            self.blocked = np.where(pushing & (moved < MOVED), self.blocked + 1, 0)
        self.blocked[done] = 0
        self.left[done] = 0
        self.last[done] = 4
        self.xy = xy
