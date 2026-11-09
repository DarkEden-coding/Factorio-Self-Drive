"""Escape rule for blocked poses, applied around a deterministic student at inference.

A vehicle pressed against a rock or cliff can map to an action that does not move it, and the
argmax policy then repeats that action until the stall limit. When the last `after` decisions
all pushed (throttle not coasting) and each moved the vehicle less than `MOVED` tiles, the rule
overrides the next `hold` decisions with a maneuver, trying in turn: the same throttle with
straight steering, the opposite throttle straight, the opposite throttle and steering, and the
opposite throttle with the same steering. A maneuver that also does not move the vehicle hands
straight to the next one; any movement resets the sequence. The mod's driver (`escape.lua`)
and `scripts/autodrive-sim.mjs` apply the same rule; keep them in step.
"""

from __future__ import annotations

import numpy as np

AFTER = 6
HOLD = 4
MOVED = 0.02


def maneuvers(action: int) -> list[int]:
    """Escape maneuvers for a blocked 9-action index, in the order they are tried."""
    throttle, steer = action // 3 - 1, action % 3 - 1
    out = [(throttle, 0)] if steer else []
    out += [(-throttle, 0), (-throttle, -steer or -1), (-throttle, steer or 1)]
    return [(t + 1) * 3 + s + 1 for t, s in out]


class Escape:
    """Per-env escape state for a batch of `n` episodes."""

    def __init__(self, n: int, after: int = AFTER, hold: int = HOLD):
        self.after, self.hold = after, hold
        self.blocked = np.zeros(n, dtype=np.int64)
        self.left = np.zeros(n, dtype=np.int64)
        self.action = np.zeros(n, dtype=np.int64)
        self.last = np.full(n, 4, dtype=np.int64)
        self.base = np.zeros(n, dtype=np.int64)    # the blocked action the maneuvers escape
        self.attempt = np.zeros(n, dtype=np.int64)  # maneuvers tried since the last movement
        self.xy: np.ndarray | None = None

    def apply(self, actions: np.ndarray) -> np.ndarray:
        """The actions to execute; marks rows it overrides in `self.active`."""
        actions = np.array(actions, dtype=np.int64)
        start = (self.left == 0) & ((self.blocked >= self.after)
                                    | ((self.attempt > 0) & (self.blocked >= self.hold)))
        for i in np.flatnonzero(start):
            if self.attempt[i] == 0:
                self.base[i] = self.last[i]
            options = maneuvers(int(self.base[i]))
            self.action[i] = options[self.attempt[i] % len(options)]
            self.attempt[i] += 1
            self.left[i], self.blocked[i] = self.hold, 0
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
            moved = np.hypot(*(xy - self.xy).T) >= MOVED
            pushing = self.last // 3 != 1
            self.blocked = np.where(pushing & ~moved, self.blocked + 1, 0)
            self.attempt[moved] = 0
        self.blocked[done] = 0
        self.left[done] = 0
        self.attempt[done] = 0
        self.last[done] = 4
        self.xy = xy


# The mod limits cars on Vulcanus (driver.lua): above this speed forward throttle becomes
# coasting with the same steering, after the escape rule.
VULCANUS_CAR_LIMIT_KMH = 130.0
KMH_PER_TILE_TICK = 216.0


class SpeedLimit:
    """The mod's Vulcanus car speed limit for a batch of envs. A finished row's info still
    describes the old episode, so a new episode's first decision is not limited."""

    def __init__(self, map_names: list[tuple[int, str]], vehicle_names: list[str], n: int):
        self.maps = np.array(sorted(i for i, name in map_names if name.startswith("vulcanus")), dtype=np.int64)
        self.car = vehicle_names.index("car") if "car" in vehicle_names else -1
        self.limited = np.zeros(n, dtype=bool)
        self.speed = np.zeros(n)

    def apply(self, actions: np.ndarray) -> np.ndarray:
        fast = self.limited & (self.speed * KMH_PER_TILE_TICK > VULCANUS_CAR_LIMIT_KMH) & (actions // 3 == 2)
        return np.where(fast, 3 + actions % 3, actions)

    def observe(self, info: dict, done) -> None:
        self.limited = (np.isin(np.asarray(info["map_index"]), self.maps)
                        & (np.asarray(info["vehicle_index"]) == self.car) & ~np.asarray(done, dtype=bool))
        self.speed = np.abs(np.asarray(info["speed"], dtype=np.float64))
