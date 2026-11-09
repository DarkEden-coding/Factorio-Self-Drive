import numpy as np

from fsim_rl.escape import Escape, mirror


def test_mirror_flips_throttle_and_steering():
    assert mirror(0) == 8  # reverse left -> forward right
    assert mirror(6) == 2  # forward left -> reverse right
    assert mirror(4) == 4  # coasting straight stays


def test_blocked_reverse_escapes_then_hands_back():
    # Same sequence as scripts/check-progressive-driver.lua.
    rule, executed = Escape(1), []
    for i in range(1, 13):
        executed.append(int(rule.apply(np.array([0]))[0]))
        y = 5.0 if i < 10 else 5.0 + i + 1  # the pose after decision i
        rule.observe([5.0], [y], [False])
    assert executed == [0, 0, 0, 0, 0, 0, 0, 8, 8, 8, 8, 0]
