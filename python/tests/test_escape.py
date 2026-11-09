import numpy as np

from fsim_rl.escape import Escape, maneuvers


def test_maneuvers_try_straight_before_mirrored_steering():
    assert maneuvers(6) == [7, 1, 2, 0]  # forward-left: forward, reverse, reverse-right, reverse-left
    assert maneuvers(0) == [1, 7, 8, 6]  # reverse-left
    assert maneuvers(7) == [1, 0, 2]     # forward straight: reverse, reverse-left, reverse-right


def test_blocked_reverse_escapes_then_hands_back():
    # Same sequence as scripts/check-progressive-driver.lua.
    rule, executed = Escape(1), []
    for i in range(1, 13):
        executed.append(int(rule.apply(np.array([0]))[0]))
        y = 5.0 if i < 10 else 5.0 + i + 1  # the pose after decision i
        rule.observe([5.0], [y], [False])
    assert executed == [0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0]


def test_wedged_vehicle_cycles_maneuvers_without_waiting():
    rule, executed = Escape(1), []
    for _ in range(40):
        executed.append(int(rule.apply(np.array([6]))[0]))
        rule.observe([5.0], [5.0], [False])
    assert executed[7:23] == [7] * 4 + [1] * 4 + [2] * 4 + [0] * 4
