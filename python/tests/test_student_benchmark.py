"""Paired comparisons must match initial states and report failures separately."""
import importlib.util
from pathlib import Path

import pytest

path = Path(__file__).resolve().parents[2] / 'scripts/benchmark-students.py'
spec = importlib.util.spec_from_file_location('student_benchmark', path)
benchmark = importlib.util.module_from_spec(spec)
spec.loader.exec_module(benchmark)


def record(seed, ticks=60, length=10, status='success', speed=0):
    return dict(scenario='standard', map='map', seed=seed, env_index=0, vehicle='car', fuel='coal',
                start=dict(x=0, y=0, orientation=0), target=dict(x=10, y=0), start_speed=speed,
                result=dict(status=status, ticks=ticks, path_length=length))


def test_comparison_pairs_exact_states_and_uses_only_common_successes():
    old = [record(1), record(2)]
    new = [record(2, status='timeout'), record(1, ticks=30, length=8)]
    result = benchmark.paired(old, new)
    assert result['matched_episodes'] == 2
    assert result['common_successes'] == 1
    assert result['time_s']['change_percent'] == -50
    assert result['path_tiles']['change_percent'] == pytest.approx(-20)
    with pytest.raises(ValueError, match='did not match'):
        benchmark.paired(old, [record(1), record(2, speed=0.1)])


def test_omitted_zero_speed_matches_explicit_zero():
    first = record(1)
    second = record(1)
    del second['start_speed']
    assert benchmark.trip_key(first) == benchmark.trip_key(second)
