//! Batched RL environment (docs/rl.md). Maps from the pool split are loaded
//! once and shared; each env keeps its own world state (entity health and
//! raster patches), vehicle, episode and RNG. Envs auto-reset.
use crate::{
    baseline,
    controller::Plan,
    map_pool::MapPool,
    pathfinder::PathRoute,
    policy_obs::{self, ObsMut, ObsScratch, ObsSource, COARSE_LEN, LOCAL_LEN, VECTOR_LEN},
    prototypes::Prototypes,
    raster::{MapRaster, RasterPatches},
    reward::{RewardTerms, TERMS},
    rl_config::{
        validate_scenarios, validate_target, ArrivalRules, EpisodeRules, FieldConfig, Mode,
        RewardWeights, RlConfig, RouteSource, Scenario, TimeLimit, HEADING_LOOKAHEAD, SCENARIOS,
    },
    rl_episode::{action_count, widen_action, Ctx, Episode, EpisodeSetup, Point, Pose, Status},
    sampler, student_obs,
    vehicle::Vehicles,
    world::World,
};
use rand::Rng;
use rand_pcg::Pcg32;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const EPISODE_ATTEMPTS: usize = 32;

/// A loaded pool map shared by all envs.
pub struct MapData {
    pub pool_index: usize,
    pub template: World,
    pub raster: MapRaster,
}

/// Per-policy-call overlay for replays. Python adds model outputs.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Overlay {
    pub decision: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_actions: Option<Vec<u8>>,
    /// World coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waypoints: Option<Vec<[f64; 2]>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct FinalState {
    pub x: f64,
    pub y: f64,
    pub orientation: f64,
    pub speed: f64,
    pub health: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct EpisodeResult {
    pub status: Status,
    pub ticks: u64,
    pub decisions: usize,
    /// Undiscounted totals per reward term.
    pub reward: RewardTerms,
    pub path_distance: f64,
    pub optimal_distance: f64,
    pub damage: f64,
    pub collisions: u64,
    pub path_length: f64,
    /// Geometric length of the field's route from the start.
    #[serde(default)]
    pub optimal_length: f64,
    /// `collisions` by kind (`rl_episode::COLLISION_KINDS`).
    #[serde(default)]
    pub collisions_by_kind: CollisionCounts,
    #[serde(default)]
    pub nest_zone_ticks: u64,
    #[serde(default)]
    pub nest_zone_decisions: u64,
    #[serde(default)]
    pub reverse_distance: f64,
    #[serde(rename = "final")]
    pub final_state: FinalState,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct CollisionCounts {
    pub tree: u64,
    pub rock: u64,
    pub cliff: u64,
    pub nest: u64,
    pub water: u64,
}

impl From<[u64; 5]> for CollisionCounts {
    fn from([tree, rock, cliff, nest, water]: [u64; 5]) -> Self {
        Self {
            tree,
            rock,
            cliff,
            nest,
            water,
        }
    }
}

/// A finished episode. Also valid input for the wasm `start_episode`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct EpisodeRecord {
    pub map: String,
    pub map_index: usize,
    /// The VecEnv env that ran the episode (0 elsewhere).
    #[serde(default)]
    pub env_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Scenario kind that sampled the episode (`rl_config::SCENARIOS`).
    #[serde(default = "standard_scenario")]
    pub scenario: String,
    pub vehicle: String,
    pub fuel: String,
    pub start: Pose,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub start_speed: f64,
    pub target: Point,
    pub radius: f64,
    pub mode: Mode,
    pub decision_ticks: u32,
    #[serde(default = "one")]
    pub steer_levels: u32,
    pub gamma: f64,
    pub reward: RewardWeights,
    pub time_limit: TimeLimit,
    pub distance_field: FieldConfig,
    #[serde(default)]
    pub arrival: ArrivalRules,
    pub actions: Vec<u8>,
    pub overlays: Vec<Overlay>,
    pub result: EpisodeResult,
}

impl EpisodeRecord {
    pub fn setup(&self) -> EpisodeSetup {
        EpisodeSetup {
            map: self.map.clone(),
            vehicle: self.vehicle.clone(),
            fuel: self.fuel.clone(),
            start: self.start,
            target: self.target,
            radius: self.radius,
            start_speed: self.start_speed,
            seed: self.seed,
        }
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
            arrival: self.arrival,
            ..EpisodeRules::default()
        }
    }
    pub fn new(
        episode: &Episode,
        map_index: usize,
        env_index: usize,
        scenario: usize,
        vehicles: &Vehicles,
        overlays: Vec<Overlay>,
    ) -> Self {
        let s = &episode.setup;
        let r = &episode.rules;
        Self {
            map: s.map.clone(),
            map_index,
            env_index,
            seed: s.seed,
            scenario: SCENARIOS[scenario].into(),
            vehicle: s.vehicle.clone(),
            fuel: s.fuel.clone(),
            start: s.start,
            start_speed: s.start_speed,
            target: s.target,
            radius: s.radius,
            mode: r.mode,
            decision_ticks: r.decision_ticks,
            steer_levels: r.steer_levels,
            gamma: r.gamma,
            reward: r.reward,
            time_limit: r.time_limit,
            distance_field: r.distance_field,
            arrival: r.arrival,
            actions: episode.actions.clone(),
            overlays,
            result: EpisodeResult {
                status: episode.status,
                ticks: episode.ticks,
                decisions: episode.decisions(),
                reward: episode.totals,
                path_distance: episode.path_distance,
                optimal_distance: episode.optimal_distance,
                damage: episode.damage,
                collisions: episode.collisions,
                path_length: episode.path_length,
                optimal_length: episode.optimal_length,
                collisions_by_kind: episode.collisions_by_kind.into(),
                nest_zone_ticks: episode.nest_zone_ticks,
                nest_zone_decisions: episode.nest_zone_decisions,
                reverse_distance: episode.reverse_distance,
                final_state: FinalState {
                    x: vehicles.x[0],
                    y: vehicles.y[0],
                    orientation: vehicles.orientation[0],
                    speed: vehicles.speed[0],
                    health: vehicles.health[0],
                },
            },
        }
    }
}

/// Per-env results of one `step`. Reward terms are discounted sums over the
/// executed decisions, like `reward`. Episode fields describe the episode that
/// was stepped (the finished one on done rows); `x, y, orientation` are the
/// pose for the returned observation (the new episode's start on done rows).
#[derive(Clone, Copy, Debug, Default)]
pub struct StepInfo {
    pub reward: f32,
    pub discount: f32,
    pub terminated: bool,
    pub truncated: bool,
    pub success: bool,
    pub destroyed: bool,
    pub timeout: bool,
    pub stalled: bool,
    pub path_distance: f32,
    pub optimal_distance: f32,
    pub optimal_length: f32,
    pub ticks: u64,
    pub damage: f32,
    pub collisions: u64,
    /// By `rl_episode::COLLISION_KINDS`.
    pub collisions_by_kind: [u64; 5],
    pub nest_zone_ticks: u64,
    pub nest_zone_decisions: u64,
    pub reverse_distance: f32,
    pub reversed: bool,
    pub path_length: f32,
    /// Vehicle |speed| (tiles/tick) and distance to the target point at the
    /// end of the step.
    pub speed: f32,
    pub target_distance: f32,
    pub map_index: u32,
    /// Scenario index (`rl_config::SCENARIOS`).
    pub scenario: u32,
    /// Vehicle prototype index (`spec()["vehicles"]`).
    pub vehicle_index: u32,
    /// Fuel index (`spec()["fuels"]`).
    pub fuel_index: u32,
    pub decisions: u32,
    pub terms: [f32; TERMS.len()],
    pub x: f32,
    pub y: f32,
    pub orientation: f32,
}

/// Batched observation buffers: `local [N, 8, 64, 64]`, `coarse [N, 8, 32, 32]`, `vector [N, V]`
/// for the full observation and `student [N, S]`. Buffers the config's `observation` doesn't
/// ask for must be empty.
pub struct ObsBatch<'a> {
    pub local: &'a mut [u8],
    pub coarse: &'a mut [u8],
    pub vector: &'a mut [f32],
    pub student: &'a mut [f32],
}

/// `n` chunks of `len`, or `n` empty slices when `len` is 0.
fn rows<T>(buffer: &mut [T], n: usize, len: usize) -> Vec<&mut [T]> {
    if len == 0 {
        (0..n).map(|_| <&mut [T]>::default()).collect()
    } else {
        buffer.chunks_mut(len).collect()
    }
}

impl<'a> ObsBatch<'a> {
    fn split(self, n: usize, config: &RlConfig) -> Result<Vec<ObsMut<'a>>, String> {
        let kind = config.observation;
        let full = usize::from(kind.full());
        let student = usize::from(kind.student());
        let [local, coarse, vector, student] = [
            LOCAL_LEN * full,
            COARSE_LEN * full,
            VECTOR_LEN * full,
            config.student_len() * student,
        ];
        if self.local.len() != n * local
            || self.coarse.len() != n * coarse
            || self.vector.len() != n * vector
            || self.student.len() != n * student
        {
            return Err("observation buffers have the wrong size".into());
        }
        Ok(rows(self.local, n, local)
            .into_iter()
            .zip(rows(self.coarse, n, coarse))
            .zip(rows(self.vector, n, vector))
            .zip(rows(self.student, n, student))
            .map(|(((local, coarse), vector), student)| ObsMut {
                local,
                coarse,
                vector,
                student,
            })
            .collect())
    }
}

/// Actions for all envs, flattened row-major, in the env's mode.
#[derive(Clone, Copy, Debug)]
pub enum Actions<'a> {
    /// `[N]` action indices.
    Reactive(&'a [i64]),
    /// `[N, horizon]` action indices.
    DirectPlan(&'a [i64]),
    /// `[N, waypoints, 2]` vehicle-frame waypoints.
    PathPlanner(&'a [f32]),
}

#[derive(Clone, Debug, PartialEq)]
pub enum OwnedActions {
    Discrete(Vec<i64>),
    Waypoints(Vec<f32>),
}

struct Shared<'a> {
    data: &'a Prototypes,
    maps: &'a [Arc<MapData>],
    config: &'a RlConfig,
    rules: &'a EpisodeRules,
    vehicles: &'a [(usize, usize)],
    recording: bool,
    step: u64,
}

struct Env {
    index: usize,
    rng: Pcg32,
    map: Arc<MapData>,
    world: World,
    vehicles: Vehicles,
    patches: RasterPatches,
    episode: Episode,
    /// Scenario index of the current episode.
    scenario: usize,
    scratch: ObsScratch,
    /// The student's pathfinder route (`RouteSource::Pathfinder`).
    path: Option<PathRoute>,
    route_source: RouteSource,
    overlays: Vec<Overlay>,
    /// Finished records with the VecEnv step that finished them.
    finished: Vec<(u64, EpisodeRecord)>,
}

impl Env {
    fn new(shared: &Shared<'_>, index: usize, rng: Pcg32) -> Result<Self, String> {
        let map = shared.maps[0].clone();
        let world = map.template.clone();
        let mut vehicles = Vehicles::default();
        vehicles.push(shared.data, shared.vehicles[0].0, shared.vehicles[0].1);
        let mut env = Self {
            index,
            rng,
            episode: Episode::placeholder(),
            scenario: 0,
            map,
            world,
            vehicles,
            patches: RasterPatches::default(),
            scratch: ObsScratch::default(),
            path: None,
            route_source: shared.config.route_source,
            overlays: Vec::new(),
            finished: Vec::new(),
        };
        env.new_episode(shared)?;
        Ok(env)
    }

    /// Starts an episode of a weighted-random scenario kind. If that kind
    /// finds nothing on `EPISODE_ATTEMPTS` maps, falls back to `standard`.
    fn new_episode(&mut self, shared: &Shared<'_>) -> Result<(), String> {
        let scenarios = &shared.config.scenarios;
        // A single kind draws nothing, so standard-only runs keep their sequences.
        let pick = if scenarios.len() > 1 {
            weighted_pick(&mut self.rng, scenarios.iter().map(Scenario::weight))
        } else {
            0
        };
        let scenario = scenarios[pick];
        match self.try_new_episode(shared, &scenario) {
            Err(_) if scenario.index() != 0 => {
                self.try_new_episode(shared, &Scenario::Standard(Default::default()))
            }
            result => result,
        }
    }

    fn try_new_episode(&mut self, shared: &Shared<'_>, scenario: &Scenario) -> Result<(), String> {
        let mut last_error = String::new();
        for _ in 0..EPISODE_ATTEMPTS {
            let map = shared.maps[self.rng.gen_range(0..shared.maps.len())].clone();
            let choice = weighted_pick(
                &mut self.rng,
                shared.config.vehicle_mix.iter().map(|v| v.weight),
            );
            let seed = self.rng.gen::<u64>();
            if Arc::ptr_eq(&map, &self.map) {
                self.world.restore_entities();
            } else {
                self.world.restore_from(&map.template);
                self.map = map;
            }
            let vehicle = shared.vehicles[choice].0;
            let sampled = match sampler::sample_scenario(
                &self.world,
                &self.map.raster,
                shared.data,
                vehicle,
                shared.rules,
                scenario,
                &shared.config.target,
                seed,
                &mut sampler::SampleStats::default(),
            ) {
                Ok(s) => s,
                Err(e) => {
                    last_error = e;
                    continue;
                }
            };
            let choice_config = &shared.config.vehicle_mix[choice];
            let setup = EpisodeSetup {
                map: self.world.info.name.clone(),
                vehicle: choice_config.vehicle.clone(),
                fuel: choice_config.fuel.clone(),
                start: sampled.start,
                target: sampled.target,
                radius: shared.config.target.radius,
                start_speed: sampled.start_speed,
                seed: Some(seed),
            };
            self.episode = Episode::start_with_field(
                self.ctx(shared.data),
                setup,
                shared.rules.clone(),
                sampled.field,
            )?;
            self.scenario = scenario.index();
            self.path = (shared.config.route_source == RouteSource::Pathfinder).then(|| {
                let s = &self.episode.setup;
                PathRoute::new(
                    &self.map.raster,
                    &shared.data.vehicles[vehicle],
                    &shared.rules.distance_field,
                    [s.start.x, s.start.y],
                    [s.target.x, s.target.y],
                )
            });
            self.overlays.clear();
            return Ok(());
        }
        Err(format!("episode sampling failed: {last_error}"))
    }

    fn ctx<'a>(&'a mut self, data: &'a Prototypes) -> Ctx<'a> {
        Ctx {
            data,
            raster: &self.map.raster,
            world: &mut self.world,
            vehicles: &mut self.vehicles,
            patches: &mut self.patches,
        }
    }

    /// Angle in radians between the heading and the route point `HEADING_LOOKAHEAD` tiles ahead;
    /// zero when not driving forward in reactive mode.
    fn heading_error(&self, rules: &EpisodeRules) -> f64 {
        let v = &self.vehicles;
        if rules.mode != Mode::Reactive || v.speed[0] < 0.02 {
            return 0.0;
        }
        let (x, y) = (v.x[0], v.y[0]);
        let target = [self.episode.setup.target.x, self.episode.setup.target.y];
        let route = if self.route_source == RouteSource::FieldSmooth {
            self.episode.field.smooth_route(x, y, target)
        } else {
            self.episode.field.route(x, y, target)
        };
        let ahead = student_obs::point_along(&route, HEADING_LOOKAHEAD);
        let (sin, cos) = (v.orientation[0] * std::f64::consts::TAU).sin_cos();
        let (px, py) = (ahead[0] - x, ahead[1] - y);
        (sin * py + cos * px).atan2(sin * px - cos * py).abs()
    }

    fn write_obs(&mut self, data: &Prototypes, out: ObsMut<'_>) {
        let route = match (&mut self.path, out.student.is_empty()) {
            (_, false)
                if self.episode.rules.mode == Mode::Reactive
                    && self.route_source == RouteSource::FieldSmooth =>
            {
                Some(self.episode.field.smooth_route(
                    self.vehicles.x[0],
                    self.vehicles.y[0],
                    [self.episode.setup.target.x, self.episode.setup.target.y],
                ))
            }
            (Some(path), false) => Some(path.route([self.vehicles.x[0], self.vehicles.y[0]])),
            _ => None,
        };
        let source = ObsSource {
            data,
            world: &self.world,
            raster: &self.map.raster,
            patches: &self.patches,
            vehicles: &self.vehicles,
            episode: &self.episode,
            route: route.as_deref(),
        };
        policy_obs::write_obs(source, &mut self.scratch, out);
    }

    fn decide(&mut self, data: &Prototypes, action: i64) -> Result<RewardTerms, String> {
        let action = u8::try_from(action).map_err(|_| "action index must be 0..255")?;
        let ctx = Ctx {
            data,
            raster: &self.map.raster,
            world: &mut self.world,
            vehicles: &mut self.vehicles,
            patches: &mut self.patches,
        };
        Ok(self.episode.decide(ctx, action)?.terms)
    }

    fn step(
        &mut self,
        shared: &Shared<'_>,
        action: EnvAction<'_>,
        obs: ObsMut<'_>,
        final_obs: ObsMut<'_>,
    ) -> Result<StepInfo, String> {
        let data = shared.data;
        let rules = shared.rules;
        let mut sum = RewardTerms::default();
        let mut scale = 1.0;
        let mut executed = 0;
        let mut overlay = Overlay {
            decision: self.episode.decisions(),
            ..Overlay::default()
        };
        match action {
            EnvAction::Single(action) => {
                let terms = self.decide(data, action)?;
                sum.add_scaled(&terms, 1.0);
                executed = 1;
            }
            EnvAction::Plan(actions) => {
                // Actions were validated as 0..9 by `VecEnv::step`.
                overlay.plan_actions = Some(actions.iter().map(|a| *a as u8).collect());
                for &a in &actions[..rules.direct_plan.execute] {
                    let terms = self.decide(data, a)?;
                    sum.add_scaled(&terms, scale);
                    scale *= rules.gamma;
                    executed += 1;
                    if self.episode.status.done() {
                        break;
                    }
                }
            }
            EnvAction::Waypoints(points) => {
                let config = &rules.path_planner;
                let plan =
                    Plan::from_local(&self.vehicles, self.episode.ticks, points, config.max_reach)?;
                overlay.waypoints = Some(plan.waypoints().to_vec());
                loop {
                    let a = plan.action(data, &self.vehicles, rules.decision_ticks);
                    let terms = self.decide(data, widen_action(a, rules.steer_levels) as i64)?;
                    sum.add_scaled(&terms, scale);
                    scale *= rules.gamma;
                    executed += 1;
                    if plan.finished(&self.vehicles, &self.episode, config) {
                        break;
                    }
                }
            }
        }
        if rules.reward.heading_error != 0.0 && !self.episode.status.done() {
            sum.heading_error = rules.reward.heading_error * self.heading_error(rules);
        }
        if shared.recording {
            self.overlays.push(overlay);
        }
        let e = &self.episode;
        let mut terms = [0.0; TERMS.len()];
        for (t, v) in terms.iter_mut().zip(sum.values()) {
            *t = v as f32;
        }
        let mut info = StepInfo {
            reward: sum.total() as f32,
            discount: rules.gamma.powi(executed) as f32,
            terminated: e.status.terminated(),
            truncated: e.status.truncated(),
            success: e.status == Status::Success,
            destroyed: e.status == Status::Destroyed,
            timeout: e.status == Status::Timeout,
            stalled: e.status == Status::Stalled,
            path_distance: e.path_distance as f32,
            optimal_distance: e.optimal_distance as f32,
            optimal_length: e.optimal_length as f32,
            ticks: e.ticks,
            damage: e.damage as f32,
            collisions: e.collisions,
            collisions_by_kind: e.collisions_by_kind,
            nest_zone_ticks: e.nest_zone_ticks,
            nest_zone_decisions: e.nest_zone_decisions,
            reverse_distance: e.reverse_distance as f32,
            reversed: e.reversed(),
            path_length: e.path_length as f32,
            speed: self.vehicles.speed[0].abs() as f32,
            target_distance: e.target_distance(&self.vehicles) as f32,
            map_index: self.map.pool_index as u32,
            scenario: self.scenario as u32,
            vehicle_index: self.vehicles.prototype[0] as u32,
            fuel_index: self.vehicles.fuel[0] as u32,
            decisions: executed as u32,
            terms,
            ..StepInfo::default()
        };
        if e.status.done() {
            self.write_obs(data, final_obs);
            if shared.recording {
                let overlays = std::mem::take(&mut self.overlays);
                let record = EpisodeRecord::new(
                    &self.episode,
                    self.map.pool_index,
                    self.index,
                    self.scenario,
                    &self.vehicles,
                    overlays,
                );
                self.finished.push((shared.step, record));
            }
            self.new_episode(shared)?;
        }
        self.write_obs(data, obs);
        info.x = self.vehicles.x[0] as f32;
        info.y = self.vehicles.y[0] as f32;
        info.orientation = self.vehicles.orientation[0] as f32;
        Ok(info)
    }

    fn baseline(&self, data: &Prototypes, rules: &EpisodeRules) -> OwnedActions {
        match rules.mode {
            Mode::Reactive => OwnedActions::Discrete(vec![widen_action(
                baseline::action(data, &self.episode, &self.vehicles),
                rules.steer_levels,
            ) as i64]),
            Mode::DirectPlan => OwnedActions::Discrete(
                baseline::plan(
                    data,
                    &self.world,
                    &self.vehicles,
                    &self.episode,
                    rules.direct_plan.horizon,
                )
                .into_iter()
                .map(|a| i64::from(widen_action(a, rules.steer_levels)))
                .collect(),
            ),
            Mode::PathPlanner => OwnedActions::Waypoints(
                baseline::waypoints(&self.episode, &self.vehicles, &rules.path_planner)
                    .into_iter()
                    .flat_map(|p| [p[0] as f32, p[1] as f32])
                    .collect(),
            ),
        }
    }
}

enum EnvAction<'a> {
    Single(i64),
    Plan(&'a [i64]),
    Waypoints(&'a [f32]),
}

#[cfg(feature = "parallel")]
fn map_tasks<T: Send, R: Send>(tasks: Vec<T>, f: impl Fn(T) -> R + Sync + Send) -> Vec<R> {
    use rayon::prelude::*;
    tasks.into_par_iter().map(f).collect()
}

#[cfg(not(feature = "parallel"))]
fn map_tasks<T: Send, R: Send>(tasks: Vec<T>, f: impl Fn(T) -> R + Sync + Send) -> Vec<R> {
    tasks.into_iter().map(f).collect()
}

pub struct VecEnv {
    pub config: RlConfig,
    rules: EpisodeRules,
    data: Arc<Prototypes>,
    maps: Vec<Arc<MapData>>,
    vehicles: Vec<(usize, usize)>,
    envs: Vec<Env>,
    recording: bool,
    steps: u64,
}

impl VecEnv {
    pub fn new(
        config: RlConfig,
        data: Arc<Prototypes>,
        num_envs: usize,
        seed: u64,
    ) -> Result<Self, String> {
        let pool = MapPool::load(&config.map_pool, seed)?;
        let indices = pool.split_indices(config.split);
        if indices.is_empty() {
            return Err(format!("map pool has no {:?} maps", config.split));
        }
        let loaded = map_tasks(indices, |index| {
            pool.map(index, &data).map(|(world, _)| {
                let raster = MapRaster::build(&world, &data);
                Arc::new(MapData {
                    pool_index: index,
                    template: world,
                    raster,
                })
            })
        });
        let maps = loaded.into_iter().collect::<Result<Vec<_>, _>>()?;
        Self::with_maps(config, data, maps, num_envs, seed)
    }

    /// Builds envs over already-loaded maps (tests and tools).
    pub fn with_maps(
        config: RlConfig,
        data: Arc<Prototypes>,
        maps: Vec<Arc<MapData>>,
        num_envs: usize,
        seed: u64,
    ) -> Result<Self, String> {
        let rules = config.rules();
        rules.validate()?;
        validate_target(&config.target)?;
        validate_scenarios(&config.scenarios)?;
        if num_envs == 0 || maps.is_empty() {
            return Err("VecEnv needs at least one env and one map".into());
        }
        let mut vehicles = Vec::new();
        for choice in &config.vehicle_mix {
            if choice.vehicle != "car" && choice.vehicle != "tank" {
                return Err("vehicle must be car or tank".into());
            }
            vehicles.push((
                data.vehicle_index(&choice.vehicle)?,
                data.fuel_index(&choice.fuel)?,
            ));
        }
        let mut env = Self {
            config,
            rules,
            data,
            maps,
            vehicles,
            envs: Vec::new(),
            recording: false,
            steps: 0,
        };
        let (shared, _) = env.parts();
        let envs = map_tasks((0..num_envs).collect(), |i| {
            Env::new(&shared, i, Pcg32::new(splitmix(seed), i as u64))
        });
        env.envs = envs.into_iter().collect::<Result<_, _>>()?;
        Ok(env)
    }

    /// Read-only shared state plus the envs, borrowed separately.
    fn parts(&mut self) -> (Shared<'_>, &mut [Env]) {
        let shared = Shared {
            data: &self.data,
            maps: &self.maps,
            config: &self.config,
            rules: &self.rules,
            vehicles: &self.vehicles,
            recording: self.recording,
            step: self.steps,
        };
        (shared, &mut self.envs)
    }

    pub fn num_envs(&self) -> usize {
        self.envs.len()
    }

    pub fn mode(&self) -> Mode {
        self.rules.mode
    }

    pub fn rules(&self) -> &EpisodeRules {
        &self.rules
    }

    pub fn maps(&self) -> &[Arc<MapData>] {
        &self.maps
    }

    pub fn data(&self) -> &Prototypes {
        &self.data
    }

    /// Starts a new episode in every env and writes the first observations.
    pub fn reset(&mut self, obs: ObsBatch<'_>) -> Result<(), String> {
        let outs = obs.split(self.envs.len(), &self.config)?;
        let (shared, envs) = self.parts();
        let tasks: Vec<_> = envs.iter_mut().zip(outs).collect();
        map_tasks(tasks, |(env, out)| {
            env.overlays.clear();
            env.new_episode(&shared)?;
            env.write_obs(shared.data, out);
            Ok::<_, String>(())
        })
        .into_iter()
        .collect()
    }

    /// Steps every env. `final_obs` rows are written only for envs whose
    /// episode ended; `obs` then holds the next episode's first observation.
    pub fn step(
        &mut self,
        actions: Actions<'_>,
        obs: ObsBatch<'_>,
        final_obs: ObsBatch<'_>,
    ) -> Result<Vec<StepInfo>, String> {
        let n = self.envs.len();
        let per_env: Vec<EnvAction<'_>> = match (self.rules.mode, actions) {
            (Mode::Reactive, Actions::Reactive(a)) if a.len() == n => {
                a.iter().copied().map(EnvAction::Single).collect()
            }
            (Mode::DirectPlan, Actions::DirectPlan(a))
                if a.len() == n * self.rules.direct_plan.horizon =>
            {
                a.chunks(self.rules.direct_plan.horizon)
                    .map(EnvAction::Plan)
                    .collect()
            }
            (Mode::PathPlanner, Actions::PathPlanner(w))
                if w.len() == n * self.rules.path_planner.waypoints * 2 =>
            {
                w.chunks(self.rules.path_planner.waypoints * 2)
                    .map(EnvAction::Waypoints)
                    .collect()
            }
            _ => {
                return Err(format!(
                    "actions do not match mode {}",
                    self.rules.mode.name()
                ))
            }
        };
        if let Actions::Reactive(a) | Actions::DirectPlan(a) = actions {
            let count = action_count(self.rules.steer_levels) as i64;
            if a.iter().any(|a| !(0..count).contains(a)) {
                return Err(format!("action index must be 0..{count}"));
            }
        }
        let outs = obs.split(n, &self.config)?;
        let finals = final_obs.split(n, &self.config)?;
        let (shared, envs) = self.parts();
        let tasks: Vec<_> = envs.iter_mut().zip(per_env).zip(outs).zip(finals).collect();
        let infos = map_tasks(tasks, |(((env, action), out), fin)| {
            env.step(&shared, action, out, fin)
        })
        .into_iter()
        .collect();
        self.steps += 1;
        infos
    }

    pub fn set_curriculum(
        &mut self,
        min_distance: f64,
        max_distance: f64,
        max_tortuosity: f64,
    ) -> Result<(), String> {
        let target = crate::rl_config::TargetConfig {
            min_distance,
            max_distance,
            max_tortuosity,
            ..self.config.target
        };
        validate_target(&target)?;
        self.config.target = target;
        Ok(())
    }

    /// Baseline actions for the current state of every env, in the env's mode:
    /// `[N]` / `[N, horizon]` indices, or `[N, waypoints, 2]` waypoints.
    pub fn baseline_actions(&self) -> OwnedActions {
        let data: &Prototypes = &self.data;
        let rules = &self.rules;
        let per_env = map_tasks(self.envs.iter().collect(), |env| env.baseline(data, rules));
        match rules.mode {
            Mode::PathPlanner => OwnedActions::Waypoints(
                per_env
                    .into_iter()
                    .flat_map(|a| match a {
                        OwnedActions::Waypoints(w) => w,
                        OwnedActions::Discrete(_) => unreachable!(),
                    })
                    .collect(),
            ),
            _ => OwnedActions::Discrete(
                per_env
                    .into_iter()
                    .flat_map(|a| match a {
                        OwnedActions::Discrete(d) => d,
                        OwnedActions::Waypoints(_) => unreachable!(),
                    })
                    .collect(),
            ),
        }
    }

    pub fn set_recording(&mut self, enabled: bool) {
        self.recording = enabled;
        if !enabled {
            for env in &mut self.envs {
                env.overlays.clear();
            }
        }
    }

    /// Finished-episode records since the last call, ordered by the step
    /// that finished them, then env index.
    pub fn take_records(&mut self) -> Vec<EpisodeRecord> {
        let mut records: Vec<(u64, EpisodeRecord)> = self
            .envs
            .iter_mut()
            .flat_map(|e| std::mem::take(&mut e.finished))
            .collect();
        records.sort_by_key(|(step, record)| (*step, record.env_index));
        records.into_iter().map(|(_, record)| record).collect()
    }

    /// The current episode of env `i`, for inspection.
    pub fn episode(&self, i: usize) -> &Episode {
        &self.envs[i].episode
    }

    /// Scenario index (`rl_config::SCENARIOS`) of env `i`'s current episode.
    pub fn scenario(&self, i: usize) -> usize {
        self.envs[i].scenario
    }

    pub fn vehicles(&self, i: usize) -> &Vehicles {
        &self.envs[i].vehicles
    }
}

/// Index drawn with probability proportional to its weight.
fn weighted_pick(rng: &mut Pcg32, weights: impl Iterator<Item = f64> + Clone) -> usize {
    let total: f64 = weights.clone().sum();
    let mut pick = rng.gen::<f64>() * total;
    let mut last = 0;
    for (i, w) in weights.enumerate() {
        if pick < w {
            return i;
        }
        pick -= w;
        if w > 0.0 {
            last = i;
        }
    }
    last
}

fn standard_scenario() -> String {
    SCENARIOS[0].into()
}

fn splitmix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e3779b97f4a7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

fn one() -> u32 {
    1
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}
