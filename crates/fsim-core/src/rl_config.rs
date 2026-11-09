//! RL environment configuration. Field names and defaults follow docs/rl.md.
use crate::map_pool::Split;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Reactive,
    DirectPlan,
    PathPlanner,
}

/// Which policy observations the VecEnv writes (docs/rl.md, "Student observation").
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    /// The teacher's `local`, `coarse` and `vector`.
    #[default]
    Full,
    /// Only the compact `student` vector.
    Student,
    /// Both, for distillation.
    Both,
}

/// Where the student's route features come from (docs/rl.md, "Pathfinder route").
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteSource {
    /// Descent on the episode's distance field, recomputed every decision.
    #[default]
    Field,
    /// Cost-aware any-angle shortcuts over the episode field.
    FieldSmooth,
    /// Waypoints from a stand-in for Factorio's pathfinder (`pathfinder.rs`).
    Pathfinder,
}

impl Observation {
    pub fn full(self) -> bool {
        self != Observation::Student
    }
    pub fn student(self) -> bool {
        self != Observation::Full
    }
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Reactive => "reactive",
            Mode::DirectPlan => "direct_plan",
            Mode::PathPlanner => "path_planner",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VehicleChoice {
    pub vehicle: String,
    pub fuel: String,
    #[serde(default = "one")]
    pub weight: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TargetConfig {
    pub min_distance: f64,
    pub max_distance: f64,
    pub max_tortuosity: f64,
    pub radius: f64,
}

impl Default for TargetConfig {
    fn default() -> Self {
        Self {
            min_distance: 16.0,
            max_distance: 64.0,
            max_tortuosity: 1.5,
            radius: 3.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TimeLimit {
    pub base_ticks: f64,
    pub per_tile_ticks: f64,
    pub stall_ticks: u64,
}

impl Default for TimeLimit {
    fn default() -> Self {
        Self {
            base_ticks: 600.0,
            per_tile_ticks: 12.0,
            stall_ticks: 1200,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RewardWeights {
    pub progress: f64,
    pub time: f64,
    pub damage: f64,
    pub collision: f64,
    pub nest_zone: f64,
    pub success: f64,
    pub destroyed: f64,
    /// `stop` arrival only: once, when the center first comes within the target radius.
    pub arrival: f64,
    /// `stop` arrival only: per decision ending within the target radius, times |speed| in
    /// tiles/tick.
    pub target_speed: f64,
    /// Per tile actually traveled; zero preserves existing training rewards.
    pub path_length: f64,
    /// Per steering step changed from the previous decision (left to right is 2); zero disables.
    pub steer_change: f64,
    /// Per radian between the heading and the point `HEADING_LOOKAHEAD` tiles along the route,
    /// charged after each forward decision (reactive mode); zero disables.
    pub heading_error: f64,
}

impl Default for RewardWeights {
    fn default() -> Self {
        Self {
            progress: 0.1,
            time: -0.005,
            damage: -5.0,
            collision: -0.2,
            nest_zone: -0.15,
            success: 10.0,
            destroyed: -10.0,
            arrival: 2.0,
            target_speed: -1.0,
            path_length: 0.0,
            steer_change: 0.0,
            heading_error: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct FieldConfig {
    pub margin: i32,
    pub tank_tree_cost: f64,
    /// Extra cost per tile within `nest_zone_margin` tiles of a nest zone tile
    /// (a tile whose center is inside a nest penalty radius).
    pub nest_zone_cost: f64,
    pub nest_zone_margin: i32,
    /// Weight of the slow-ground cost: each tile costs this times the
    /// vehicle's slowdown relative to grass (`distance_field::friction_extra`).
    /// 0 keeps every open tile at cost 1.
    pub friction_cost: f64,
}

impl Default for FieldConfig {
    fn default() -> Self {
        Self {
            margin: 48,
            tank_tree_cost: 4.0,
            nest_zone_cost: 3.0,
            nest_zone_margin: 2,
            friction_cost: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DirectPlanConfig {
    pub horizon: usize,
    pub execute: usize,
}

impl Default for DirectPlanConfig {
    fn default() -> Self {
        Self {
            horizon: 24,
            execute: 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PathPlannerConfig {
    pub waypoints: usize,
    pub max_reach: f64,
    pub replan_ticks: u64,
    pub drift_tiles: f64,
}

impl Default for PathPlannerConfig {
    fn default() -> Self {
        Self {
            waypoints: 8,
            max_reach: 100.0,
            replan_ticks: 30,
            drift_tiles: 3.0,
        }
    }
}

/// What counts as reaching the target (docs/rl.md, "Episodes").
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArrivalMode {
    /// Success as soon as the vehicle center is within the target radius.
    #[default]
    Enter,
    /// Success when the vehicle is stopped near the target center.
    Stop,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ArrivalRules {
    pub mode: ArrivalMode,
    /// `stop`: the center must be within this many tiles of the target point.
    pub stop_radius: f64,
    /// `stop`: and |speed| at most this, in tiles/tick.
    pub stop_speed: f64,
}

impl Default for ArrivalRules {
    fn default() -> Self {
        Self {
            mode: ArrivalMode::Enter,
            stop_radius: 1.0,
            stop_speed: 0.01,
        }
    }
}

/// Rules that decide how an episode runs and is rewarded. Shared by the
/// VecEnv and the wasm `Sim`, and stored in episode records.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct EpisodeRules {
    pub mode: Mode,
    pub decision_ticks: u32,
    /// Steering strengths per side: strength `k` steers the first `k * decision_ticks /
    /// steer_levels` ticks of a decision and holds straight for the rest. 1 is the classic
    /// left/straight/right (9 actions); it must divide `decision_ticks`.
    pub steer_levels: u32,
    pub gamma: f64,
    pub reward: RewardWeights,
    pub time_limit: TimeLimit,
    pub distance_field: FieldConfig,
    pub direct_plan: DirectPlanConfig,
    pub path_planner: PathPlannerConfig,
    pub arrival: ArrivalRules,
}

impl Default for EpisodeRules {
    fn default() -> Self {
        Self {
            mode: Mode::Reactive,
            decision_ticks: 4,
            steer_levels: 1,
            gamma: 0.99,
            reward: RewardWeights::default(),
            time_limit: TimeLimit::default(),
            distance_field: FieldConfig::default(),
            direct_plan: DirectPlanConfig::default(),
            path_planner: PathPlannerConfig::default(),
            arrival: ArrivalRules::default(),
        }
    }
}

impl EpisodeRules {
    pub fn validate(&self) -> Result<(), String> {
        let d = &self.direct_plan;
        let p = &self.path_planner;
        if self.decision_ticks == 0
            || !(1..=self.decision_ticks).contains(&self.steer_levels)
            || !self.decision_ticks.is_multiple_of(self.steer_levels)
            || !(0.0..=1.0).contains(&self.gamma)
            || d.horizon == 0
            || d.execute == 0
            || d.execute > d.horizon
            || p.waypoints == 0
            || !positive(p.max_reach)
            || p.replan_ticks == 0
            || !positive(p.drift_tiles)
            || self.distance_field.margin < 0
            || !nonnegative(self.distance_field.tank_tree_cost)
            || !nonnegative(self.distance_field.nest_zone_cost)
            || self.distance_field.nest_zone_margin < 0
            || !nonnegative(self.time_limit.base_ticks)
            || !nonnegative(self.time_limit.per_tile_ticks)
            || self.time_limit.stall_ticks == 0
            || !positive(self.arrival.stop_radius)
            || !nonnegative(self.arrival.stop_speed)
        {
            return Err("invalid episode rules".into());
        }
        let r = &self.reward;
        if [
            r.progress,
            r.time,
            r.damage,
            r.collision,
            r.nest_zone,
            r.success,
            r.destroyed,
            r.arrival,
            r.target_speed,
        ]
        .iter()
        .any(|w| !w.is_finite())
        {
            return Err("reward weights must be finite".into());
        }
        Ok(())
    }
}

/// Route distance of the point whose bearing the `heading_error` reward measures.
pub const HEADING_LOOKAHEAD: f64 = 8.0;

/// The VecEnv `config_json` from docs/rl.md.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RlConfig {
    pub map_pool: String,
    pub split: Split,
    pub mode: Mode,
    pub decision_ticks: u32,
    pub steer_levels: u32,
    pub gamma: f64,
    pub vehicle_mix: Vec<VehicleChoice>,
    pub target: TargetConfig,
    pub time_limit: TimeLimit,
    pub reward: RewardWeights,
    pub distance_field: FieldConfig,
    pub direct_plan: DirectPlanConfig,
    pub path_planner: PathPlannerConfig,
    /// Weighted scenario kinds; each reset picks one.
    pub scenarios: Vec<Scenario>,
    pub observation: Observation,
    /// Append the coarse vehicle-frame map to the student vector (`student_obs::MAP_LEN`).
    pub student_map: bool,
    pub route_source: RouteSource,
    pub arrival: ArrivalRules,
}

impl Default for RlConfig {
    fn default() -> Self {
        let rules = EpisodeRules::default();
        Self {
            map_pool: "data/game/maps".into(),
            split: Split::Train,
            mode: rules.mode,
            decision_ticks: rules.decision_ticks,
            steer_levels: rules.steer_levels,
            gamma: rules.gamma,
            vehicle_mix: vec![VehicleChoice {
                vehicle: "car".into(),
                fuel: "coal".into(),
                weight: 1.0,
            }],
            target: TargetConfig::default(),
            time_limit: rules.time_limit,
            reward: rules.reward,
            distance_field: rules.distance_field,
            direct_plan: rules.direct_plan,
            path_planner: rules.path_planner,
            scenarios: vec![Scenario::Standard(StandardScenario::default())],
            observation: Observation::Full,
            student_map: false,
            route_source: RouteSource::Field,
            arrival: rules.arrival,
        }
    }
}

impl RlConfig {
    /// Length of one student observation row.
    pub fn student_len(&self) -> usize {
        crate::student_obs::STUDENT_LEN
            + if self.student_map {
                crate::student_obs::MAP_LEN
            } else {
                0
            }
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let config: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        config.rules().validate()?;
        validate_target(&config.target)?;
        if config.vehicle_mix.is_empty()
            || config.vehicle_mix.iter().any(|v| !nonnegative(v.weight))
            || config.vehicle_mix.iter().all(|v| v.weight == 0.0)
        {
            return Err("vehicle_mix needs nonnegative weights with a positive total".into());
        }
        validate_scenarios(&config.scenarios)?;
        Ok(config)
    }

    pub fn rules(&self) -> EpisodeRules {
        EpisodeRules {
            mode: self.mode,
            decision_ticks: self.decision_ticks,
            steer_levels: self.steer_levels,
            gamma: self.gamma,
            reward: self.reward,
            time_limit: self.time_limit,
            distance_field: self.distance_field,
            direct_plan: self.direct_plan,
            path_planner: self.path_planner,
            arrival: self.arrival,
        }
    }
}

pub fn validate_target(t: &TargetConfig) -> Result<(), String> {
    if !nonnegative(t.min_distance)
        || !nonnegative(t.max_distance - t.min_distance)
        || !nonnegative(t.max_tortuosity - 1.0)
        || !positive(t.radius)
    {
        return Err(
            "target needs 0 <= min_distance <= max_distance, max_tortuosity >= 1, radius > 0"
                .into(),
        );
    }
    Ok(())
}

/// Episode kinds the sampler can produce (docs/rl.md, "Scenarios"). Each
/// kind has its own parameters; `weight` sets how often a reset picks it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scenario {
    /// Start and target from the curriculum range (`target` / `set_curriculum`).
    Standard(StandardScenario),
    /// Long routes across most of the map.
    Long(LongScenario),
    /// Start in dense trees; the route runs through dense trees.
    Forest(ForestScenario),
    /// Nose close to an obstacle with the route leading behind or beside.
    Reverse(ReverseScenario),
    /// The zone-free shortest route crosses a nest zone; a bounded detour avoids it.
    Nest(NestScenario),
    /// Short targets, some approached at speed or backward, for learning to stop.
    Arrival(ArrivalScenario),
}

/// `Scenario` names in index order; `info["scenario"]` indexes this list.
pub const SCENARIOS: [&str; 6] = ["standard", "long", "forest", "reverse", "nest", "arrival"];

impl Scenario {
    pub fn index(&self) -> usize {
        match self {
            Scenario::Standard(_) => 0,
            Scenario::Long(_) => 1,
            Scenario::Forest(_) => 2,
            Scenario::Reverse(_) => 3,
            Scenario::Nest(_) => 4,
            Scenario::Arrival(_) => 5,
        }
    }

    pub fn name(&self) -> &'static str {
        SCENARIOS[self.index()]
    }

    pub fn weight(&self) -> f64 {
        match self {
            Scenario::Standard(s) => s.weight,
            Scenario::Long(s) => s.weight,
            Scenario::Forest(s) => s.weight,
            Scenario::Reverse(s) => s.weight,
            Scenario::Nest(s) => s.weight,
            Scenario::Arrival(s) => s.weight,
        }
    }

    fn validate(&self) -> Result<(), String> {
        let ok = match self {
            Scenario::Standard(_) => true,
            Scenario::Long(s) => valid_range(s.min_distance, s.max_distance, s.max_tortuosity),
            Scenario::Forest(s) => {
                valid_range(s.min_distance, s.max_distance, s.max_tortuosity)
                    && s.density_radius >= 1
                    && [s.start_density, s.route_density, s.route_fraction]
                        .iter()
                        .all(|v| (0.0..=1.0).contains(v))
            }
            Scenario::Reverse(s) => {
                valid_range(s.min_distance, s.max_distance, s.max_tortuosity)
                    && positive(s.min_gap)
                    && s.max_gap >= s.min_gap
                    && s.max_gap.is_finite()
                    && (0.0..=180.0).contains(&s.min_route_angle)
                    && s.route_lookahead >= 1
            }
            Scenario::Nest(s) => {
                valid_range(s.min_distance, s.max_distance, 1.0)
                    && s.min_zone_tiles >= 1
                    && s.max_detour >= 1.0
                    && s.max_detour.is_finite()
            }
            Scenario::Arrival(s) => {
                valid_range(s.min_distance, s.max_distance, s.max_tortuosity)
                    && (0.0..=1.0).contains(&s.moving_fraction)
                    && (0.0..=1.0).contains(&(s.moving_fraction + s.backward_fraction))
                    && s.backward_fraction >= 0.0
                    && nonnegative(s.max_start_speed)
            }
        };
        if !ok || !nonnegative(self.weight()) {
            return Err(format!("invalid {} scenario parameters", self.name()));
        }
        Ok(())
    }
}

pub fn validate_scenarios(scenarios: &[Scenario]) -> Result<(), String> {
    if scenarios.is_empty() || scenarios.iter().all(|s| s.weight() == 0.0) {
        return Err("scenarios needs nonnegative weights with a positive total".into());
    }
    scenarios.iter().try_for_each(Scenario::validate)
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct StandardScenario {
    pub weight: f64,
}

impl Default for StandardScenario {
    fn default() -> Self {
        Self { weight: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LongScenario {
    pub weight: f64,
    pub min_distance: f64,
    pub max_distance: f64,
    pub max_tortuosity: f64,
}

impl Default for LongScenario {
    fn default() -> Self {
        Self {
            weight: 1.0,
            min_distance: 200.0,
            max_distance: 700.0,
            max_tortuosity: 3.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ForestScenario {
    pub weight: f64,
    pub min_distance: f64,
    pub max_distance: f64,
    pub max_tortuosity: f64,
    /// Tree density at a tile = fraction of tree tiles in the square of this
    /// half-size around it.
    pub density_radius: i32,
    /// Minimum tree density at the start tile.
    pub start_density: f64,
    /// A route tile counts as dense at this tree density or more.
    pub route_density: f64,
    /// Minimum fraction of dense tiles along the field route.
    pub route_fraction: f64,
}

impl Default for ForestScenario {
    fn default() -> Self {
        Self {
            weight: 1.0,
            min_distance: 16.0,
            max_distance: 96.0,
            max_tortuosity: 2.0,
            density_radius: 6,
            start_density: 0.2,
            route_density: 0.2,
            route_fraction: 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ReverseScenario {
    pub weight: f64,
    pub min_distance: f64,
    pub max_distance: f64,
    pub max_tortuosity: f64,
    /// Free distance in tiles from the hull front to the obstacle ahead.
    pub min_gap: f64,
    pub max_gap: f64,
    /// Minimum angle in degrees between the heading and the route's direction
    /// over its first `route_lookahead` tiles.
    pub min_route_angle: f64,
    pub route_lookahead: usize,
}

impl Default for ReverseScenario {
    fn default() -> Self {
        Self {
            weight: 1.0,
            min_distance: 8.0,
            max_distance: 48.0,
            max_tortuosity: 3.0,
            min_gap: 0.3,
            max_gap: 3.0,
            min_route_angle: 110.0,
            route_lookahead: 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct NestScenario {
    pub weight: f64,
    /// Range for the zone-free path distance.
    pub min_distance: f64,
    pub max_distance: f64,
    /// Minimum nest-zone tiles on the zone-free route.
    pub min_zone_tiles: usize,
    /// Maximum zone-avoiding route length / zone-free route length.
    pub max_detour: f64,
}

impl Default for NestScenario {
    fn default() -> Self {
        Self {
            weight: 1.0,
            min_distance: 24.0,
            max_distance: 160.0,
            min_zone_tiles: 4,
            max_detour: 1.6,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ArrivalScenario {
    pub weight: f64,
    pub min_distance: f64,
    pub max_distance: f64,
    pub max_tortuosity: f64,
    /// Share of episodes that start moving along the route at a speed drawn
    /// uniformly from [0, `max_start_speed`] tiles/tick.
    pub moving_fraction: f64,
    /// Share of episodes that start at rest facing away from the route.
    pub backward_fraction: f64,
    pub max_start_speed: f64,
}

impl Default for ArrivalScenario {
    fn default() -> Self {
        Self {
            weight: 1.0,
            min_distance: 6.0,
            max_distance: 40.0,
            max_tortuosity: 2.0,
            moving_fraction: 0.4,
            backward_fraction: 0.2,
            max_start_speed: 0.25,
        }
    }
}

fn valid_range(min_distance: f64, max_distance: f64, max_tortuosity: f64) -> bool {
    nonnegative(min_distance)
        && nonnegative(max_distance - min_distance)
        && nonnegative(max_tortuosity - 1.0)
}

/// Finite and > 0; false for NaN.
fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

/// Finite and >= 0; false for NaN.
fn nonnegative(x: f64) -> bool {
    x.is_finite() && x >= 0.0
}

fn one() -> f64 {
    1.0
}
