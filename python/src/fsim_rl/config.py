"""TOML configs loaded into dataclasses. A config may name a `base` file; tables merge, lists replace."""

from __future__ import annotations

import dataclasses
import tomllib
import typing
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

PYTHON_DIR = Path(__file__).resolve().parents[2]
REPO_ROOT = PYTHON_DIR.parent
CONFIG_DIR = PYTHON_DIR / "configs"

MODEL_KINDS = ("reactive", "direct_plan", "path_planner")
OBSERVATIONS = ("full", "student", "both")
# Scenario kinds in Rust's index order (`spec()["scenarios"]`, `info["scenario"]`).
SCENARIO_KINDS = ("standard", "long", "forest", "reverse", "nest", "arrival", "impact", "recover")
ARRIVAL_MODES = ("enter", "stop")


class ConfigError(ValueError):
    pass


@dataclass
class VehicleMix:
    vehicle: str = "car"
    fuel: str = "coal"
    weight: float = 1.0


@dataclass
class TargetConfig:
    min_distance: float = 16.0
    max_distance: float = 64.0
    max_tortuosity: float = 1.5
    radius: float = 3.0


@dataclass
class TimeLimitConfig:
    base_ticks: int = 600
    per_tile_ticks: int = 12
    stall_ticks: int = 1200


@dataclass
class RewardConfig:
    progress: float = 0.1
    time: float = -0.005
    damage: float = -5.0
    collision: float = -0.2
    nest_zone: float = -0.15
    success: float = 10.0
    destroyed: float = -10.0
    arrival: float = 2.0         # stop arrival only: once, on first entering the target radius
    target_speed: float = -1.0   # stop arrival only: per decision inside the radius, times |speed| (tiles/tick)
    path_length: float = 0.0     # per tile actually traveled, including reverse travel
    steer_change: float = 0.0    # per steering step changed from the previous decision (left to right = 2)
    heading_error: float = 0.0   # per radian between heading and the route 8 tiles ahead, after forward decisions
    clearance: float = 0.0       # per decision ending near an obstacle: half within 0.5 tiles, all within 0.25


@dataclass
class ArrivalConfig:
    """What counts as reaching the target (docs/rl.md, "Episodes")."""
    mode: str = "enter"          # enter: center within the radius | stop: stopped near the center
    stop_radius: float = 1.0     # stop: center within this many tiles of the target point
    stop_speed: float = 0.01     # stop: |speed| at most this, tiles/tick


@dataclass
class DistanceFieldConfig:
    margin: int = 48
    tank_tree_cost: float = 4.0
    nest_zone_cost: float = 3.0
    nest_zone_margin: int = 2
    # Slow-ground cost weight (docs/rl.md, "Distance field"); 0 = every open tile costs 1.
    friction_cost: float = 0.0
    # Extra cost per tile within clearance_margin tiles beyond the blocked dilation; 0 = off.
    clearance_cost: float = 0.0
    clearance_margin: int = 1


@dataclass
class DirectPlanConfig:
    horizon: int = 24
    execute: int = 4


@dataclass
class PathPlannerConfig:
    waypoints: int = 8
    max_reach: float = 100.0
    replan_ticks: int = 30
    drift_tiles: float = 3.0


@dataclass
class EnvConfig:
    map_pool: str = "data/game/maps"
    # Weighted pools ({pool, weight} tables, e.g. one per planet); replaces map_pool when set.
    map_pools: list[dict[str, Any]] = field(default_factory=list)
    split: str = "train"
    decision_ticks: int = 4
    steer_levels: int = 1        # steering strengths per side; 1 = left/straight/right (9 actions)
    gamma: float = 0.99
    vehicle_mix: list[VehicleMix] = field(default_factory=lambda: [VehicleMix()])
    target: TargetConfig = field(default_factory=TargetConfig)
    time_limit: TimeLimitConfig = field(default_factory=TimeLimitConfig)
    reward: RewardConfig = field(default_factory=RewardConfig)
    distance_field: DistanceFieldConfig = field(default_factory=DistanceFieldConfig)
    direct_plan: DirectPlanConfig = field(default_factory=DirectPlanConfig)
    path_planner: PathPlannerConfig = field(default_factory=PathPlannerConfig)
    # Weighted scenario kinds, each a table with `kind`, `weight` and that kind's parameters
    # (docs/rl.md, "Scenarios"); Rust fills missing parameters and validates them.
    scenarios: list[dict[str, Any]] = field(default_factory=lambda: [{"kind": "standard", "weight": 1.0}])
    # Policy observations the env returns: full (teacher grids + vector), student, or both.
    observation: str = "full"
    # Append the coarse vehicle-frame map to the student observation (docs/rl.md, "Student map").
    student_map: bool = False
    # Student guidance: raw field descent, cost-aware any-angle field_smooth, or
    # pathfinder waypoints (needs student_map; docs/rl.md).
    route_source: str = "field"
    arrival: ArrivalConfig = field(default_factory=ArrivalConfig)


@dataclass
class ModelConfig:
    kind: str = "reactive"
    arch: str = "full"  # full: CNN encoder on the full observation | student: MLP on the student one
    local_cnn: list[int] = field(default_factory=lambda: [32, 64, 64])
    coarse_cnn: list[int] = field(default_factory=lambda: [32, 64])
    local_features: int = 256
    coarse_features: int = 128
    vector_hidden: int = 128
    torso_hidden: list[int] = field(default_factory=lambda: [512, 256])
    normalize_vector: bool = True
    precision: str = "bf16"  # bf16 | fp32: encoder compute on mps/cuda; heads stay fp32
    plan_consistency_coef: float = 0.5
    segment_min_length: float = 1.0
    segment_max_length: float = 12.0
    max_turn: float = 0.6
    init_log_std: float = -0.5
    # Student (arch = "student"): the shipped actor and the training-only critic.
    student_hidden: list[int] = field(default_factory=lambda: [64, 64])
    ray_conv: list[int] = field(default_factory=list)  # channels of circular 1D convs over the rays
    student_route: bool = True  # read the route features; false drops them from the actor
    map_conv: list[int] = field(default_factory=list)  # channels of stride-2 convs over the student map
    critic_hidden: list[int] = field(default_factory=lambda: [256, 256])


@dataclass
class PPOConfig:
    num_envs: int = 64
    rollout_steps: int = 128
    total_steps: int = 50_000_000
    epochs: int = 4
    num_minibatches: int = 8
    learning_rate: float = 3e-4
    lr_schedule: str = "linear"
    gae_lambda: float = 0.95
    clip_coef: float = 0.2
    clip_value: float = 0.2
    vf_coef: float = 0.5
    ent_coef: float = 0.01
    max_grad_norm: float = 0.5
    target_kl: float = 0.03
    norm_adv: bool = True
    # KL(teacher || policy) penalty for student fine-tuning; 0 disables it.
    teacher_kl_coef: float = 0.0
    # Student fine-tuning toward a proportional steering rule on open ground (steer_levels > 1); 0 disables.
    steer_prior_coef: float = 0.0
    teacher_checkpoint: str = ""


@dataclass
class DistillConfig:
    """DAgger distillation of a student from a teacher checkpoint (`fsim-rl distill`)."""
    num_envs: int = 256
    rollout_steps: int = 64           # env steps per iteration
    iterations: int = 400             # upper bound; stops earlier on a plateau
    beta_start: float = 1.0           # probability of executing the teacher's action
    beta_iterations: int = 60         # beta falls linearly to 0 over these iterations
    buffer_size: int = 1_000_000      # aggregated DAgger samples kept (oldest dropped)
    minibatch: int = 4096
    updates_per_iteration: int = 32
    learning_rate: float = 1e-3
    final_learning_rate: float = 1e-4  # cosine decay over `iterations`
    value_coef: float = 0.5           # weight of the regression to the teacher's value
    student_sample: bool = True       # sample the student's actions (else its argmax)
    patience: int = 40                # iterations without a 2% better fresh KL after beta hits 0


@dataclass
class CurriculumStage:
    min_distance: float
    max_distance: float
    max_tortuosity: float


@dataclass
class CurriculumConfig:
    enabled: bool = True
    success_threshold: float = 0.7
    window: int = 200
    min_episodes: int = 100
    stages: list[CurriculumStage] = field(default_factory=list)


@dataclass
class Config:
    seed: int = 1
    device: str = "auto"
    run_dir: str = "runs"
    checkpoint_interval: int = 10
    env: EnvConfig = field(default_factory=EnvConfig)
    model: ModelConfig = field(default_factory=ModelConfig)
    ppo: PPOConfig = field(default_factory=PPOConfig)
    curriculum: CurriculumConfig = field(default_factory=CurriculumConfig)
    distill: DistillConfig = field(default_factory=DistillConfig)

    def validate(self) -> None:
        if self.model.kind not in MODEL_KINDS:
            raise ConfigError(f"model.kind must be one of {MODEL_KINDS}, got {self.model.kind!r}")
        if self.env.observation not in OBSERVATIONS:
            raise ConfigError(f"env.observation must be one of {OBSERVATIONS}")
        if self.env.arrival.mode not in ARRIVAL_MODES:
            raise ConfigError(f"env.arrival.mode must be one of {ARRIVAL_MODES}")
        if self.model.arch == "student":
            if self.model.kind != "reactive":
                raise ConfigError("model.arch = student needs model.kind = reactive")
            if self.env.observation == "full":
                raise ConfigError("model.arch = student needs env.observation student or both")
            if not self.model.student_hidden or not self.model.critic_hidden:
                raise ConfigError("model.student_hidden and model.critic_hidden need a layer")
            if self.model.map_conv and not self.env.student_map:
                raise ConfigError("model.map_conv needs env.student_map = true")
        elif self.model.arch == "full":
            if self.env.observation == "student":
                raise ConfigError("model.arch = full needs env.observation full or both")
        else:
            raise ConfigError(f"model.arch must be full or student, got {self.model.arch!r}")
        if self.env.route_source not in ("field", "field_smooth", "pathfinder"):
            raise ConfigError("env.route_source must be field, field_smooth or pathfinder")
        if self.env.route_source == "pathfinder" and not self.env.student_map:
            raise ConfigError("env.route_source = pathfinder needs env.student_map = true")
        if self.ppo.lr_schedule not in ("linear", "constant"):
            raise ConfigError(f"ppo.lr_schedule must be linear or constant, got {self.ppo.lr_schedule!r}")
        if self.ppo.num_envs * self.ppo.rollout_steps < self.ppo.num_minibatches:
            raise ConfigError("ppo.num_minibatches exceeds the rollout size")
        plan = self.env.direct_plan
        if not 1 <= plan.execute <= plan.horizon:
            raise ConfigError("env.direct_plan.execute must be in [1, horizon]")
        if not 0 < self.model.segment_min_length <= self.model.segment_max_length:
            raise ConfigError("model.segment_min_length must be in (0, segment_max_length]")
        validate_scenarios(self.env.scenarios)
        reach = self.env.path_planner.waypoints * self.model.segment_max_length
        if reach > self.env.path_planner.max_reach + 1e-6:
            raise ConfigError(
                f"waypoints * segment_max_length = {reach} exceeds env.path_planner.max_reach"
            )


def validate_scenarios(scenarios: list[dict]) -> None:
    total = 0.0
    for i, scenario in enumerate(scenarios):
        if scenario.get("kind") not in SCENARIO_KINDS:
            raise ConfigError(f"env.scenarios[{i}].kind must be one of {SCENARIO_KINDS}")
        weight = scenario.get("weight", 1.0)
        if isinstance(weight, bool) or not isinstance(weight, (int, float)) or weight < 0:
            raise ConfigError(f"env.scenarios[{i}].weight must be a nonnegative number")
        total += weight
    if total <= 0:
        raise ConfigError("env.scenarios needs a positive total weight")


def deep_merge(base: dict, override: dict) -> dict:
    merged = dict(base)
    for key, value in override.items():
        if isinstance(value, dict) and isinstance(merged.get(key), dict):
            merged[key] = deep_merge(merged[key], value)
        else:
            merged[key] = value
    return merged


def read_toml(path: Path) -> dict:
    """Read a TOML file and resolve its `base` chain into one dict."""
    path = Path(path)
    with path.open("rb") as f:
        data = tomllib.load(f)
    base = data.pop("base", None)
    if base is None:
        return data
    return deep_merge(read_toml(path.parent / base), data)


def load_config(path: str | Path, overrides: dict | None = None) -> Config:
    data = read_toml(Path(path))
    if overrides:
        data = deep_merge(data, overrides)
    return config_from_dict(data)


def with_overrides(config: Config, overrides: dict | None) -> Config:
    """A copy of `config` with `overrides` merged in (tables merge, lists replace)."""
    if not overrides:
        return config
    return config_from_dict(deep_merge(config_to_dict(config), overrides))


def config_from_dict(data: dict) -> Config:
    config = _build(Config, data, "config")
    config.validate()
    return config


def config_to_dict(config: Config) -> dict:
    return dataclasses.asdict(config)


def _build(cls: type, data: Any, path: str) -> Any:
    if not isinstance(data, dict):
        raise ConfigError(f"{path} must be a table")
    hints = typing.get_type_hints(cls)
    unknown = set(data) - set(hints)
    if unknown:
        raise ConfigError(f"unknown keys in {path}: {sorted(unknown)}")
    return cls(**{key: _convert(hints[key], value, f"{path}.{key}") for key, value in data.items()})


def _convert(tp: Any, value: Any, path: str) -> Any:
    if dataclasses.is_dataclass(tp):
        return _build(tp, value, path)
    if typing.get_origin(tp) is dict:
        if not isinstance(value, dict):
            raise ConfigError(f"{path} must be a table")
        return dict(value)
    if typing.get_origin(tp) is list:
        if not isinstance(value, list):
            raise ConfigError(f"{path} must be a list")
        (item,) = typing.get_args(tp)
        return [_convert(item, v, f"{path}[{i}]") for i, v in enumerate(value)]
    if tp is float and isinstance(value, int) and not isinstance(value, bool):
        return float(value)
    if tp in (int, float, str, bool) and not isinstance(value, tp):
        raise ConfigError(f"{path} must be {tp.__name__}, got {value!r}")
    return value
