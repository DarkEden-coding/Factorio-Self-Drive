use fsim_core::{
    baseline,
    controller::Plan,
    observation, physics,
    policy_obs::{self, ObsScratch, ObsSource, COARSE_LEN, LOCAL_LEN, VECTOR_LEN},
    raster::{MapRaster, RasterPatches},
    rl_config::{
        ArrivalRules, DirectPlanConfig, EpisodeRules, FieldConfig, Mode, PathPlannerConfig,
        RewardWeights, TargetConfig, TimeLimit,
    },
    rl_episode::{Ctx, Episode, EpisodeSetup, Point, Pose},
    sampler,
    student_obs::{self, STUDENT_LEN},
    target::Target,
    Config, Prototypes, Simulator,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

const PROTOTYPES: &str = include_str!("../../../data/prototypes.json");
/// `place_random` draws: far more than any real map needs.
const PLACE_ATTEMPTS: usize = 100_000;

#[wasm_bindgen]
pub struct Sim {
    inner: Simulator,
    /// Static raster of the loaded map, built on first RL use.
    raster: Option<MapRaster>,
    patches: RasterPatches,
    episode: Option<Episode>,
    /// The `follow_path` plan and the world waypoints it was started with.
    plan: Option<(Vec<f32>, Plan)>,
}

/// Input of `start_episode` and `sample_episode`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EpisodeRequest {
    start: Option<Pose>,
    start_speed: Option<f64>,
    target: Option<Point>,
    radius: Option<f64>,
    vehicle: Option<String>,
    fuel: Option<String>,
    map: Option<String>,
    seed: Option<u64>,
    min_distance: Option<f64>,
    max_distance: Option<f64>,
    max_tortuosity: Option<f64>,
    mode: Option<Mode>,
    decision_ticks: Option<u32>,
    gamma: Option<f64>,
    reward: Option<RewardWeights>,
    time_limit: Option<TimeLimit>,
    distance_field: Option<FieldConfig>,
    direct_plan: Option<DirectPlanConfig>,
    path_planner: Option<PathPlannerConfig>,
    arrival: Option<ArrivalRules>,
    // Episode-record and episode-info fields, accepted so records and the
    // JSON returned by start/sample_episode can be passed back directly.
    #[allow(dead_code)]
    optimal_distance: Option<serde_json::Value>,
    #[allow(dead_code)]
    time_limit_ticks: Option<serde_json::Value>,
    #[allow(dead_code)]
    map_index: Option<serde_json::Value>,
    #[allow(dead_code)]
    env_index: Option<serde_json::Value>,
    #[allow(dead_code)]
    scenario: Option<serde_json::Value>,
    #[allow(dead_code)]
    actions: Option<serde_json::Value>,
    #[allow(dead_code)]
    overlays: Option<serde_json::Value>,
    #[allow(dead_code)]
    result: Option<serde_json::Value>,
}

impl EpisodeRequest {
    fn rules(&self) -> EpisodeRules {
        let d = EpisodeRules::default();
        EpisodeRules {
            mode: self.mode.unwrap_or(d.mode),
            decision_ticks: self.decision_ticks.unwrap_or(d.decision_ticks),
            gamma: self.gamma.unwrap_or(d.gamma),
            reward: self.reward.unwrap_or(d.reward),
            time_limit: self.time_limit.unwrap_or(d.time_limit),
            distance_field: self.distance_field.unwrap_or(d.distance_field),
            direct_plan: self.direct_plan.unwrap_or(d.direct_plan),
            path_planner: self.path_planner.unwrap_or(d.path_planner),
            arrival: self.arrival.unwrap_or(d.arrival),
        }
    }
}

/// Optional rule keys of `retarget`; missing keys keep the current episode's rules.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RulesRequest {
    mode: Option<Mode>,
    decision_ticks: Option<u32>,
    gamma: Option<f64>,
    reward: Option<RewardWeights>,
    time_limit: Option<TimeLimit>,
    distance_field: Option<FieldConfig>,
    direct_plan: Option<DirectPlanConfig>,
    path_planner: Option<PathPlannerConfig>,
    arrival: Option<ArrivalRules>,
}

impl RulesRequest {
    fn apply(self, base: EpisodeRules) -> EpisodeRules {
        EpisodeRules {
            mode: self.mode.unwrap_or(base.mode),
            decision_ticks: self.decision_ticks.unwrap_or(base.decision_ticks),
            gamma: self.gamma.unwrap_or(base.gamma),
            reward: self.reward.unwrap_or(base.reward),
            time_limit: self.time_limit.unwrap_or(base.time_limit),
            distance_field: self.distance_field.unwrap_or(base.distance_field),
            direct_plan: self.direct_plan.unwrap_or(base.direct_plan),
            path_planner: self.path_planner.unwrap_or(base.path_planner),
            arrival: self.arrival.unwrap_or(base.arrival),
        }
    }
}

#[derive(Serialize)]
struct EpisodeInfo<'a> {
    #[serde(flatten)]
    setup: &'a EpisodeSetup,
    #[serde(flatten)]
    rules: &'a EpisodeRules,
    optimal_distance: f64,
    time_limit_ticks: u64,
}

#[derive(Serialize)]
struct EpisodeState {
    status: fsim_core::rl_episode::Status,
    ticks: u64,
    decisions: usize,
    time_limit_ticks: u64,
    path_distance: f64,
    optimal_distance: f64,
    reward: fsim_core::reward::RewardTerms,
    reward_total: f64,
    damage: f64,
    collisions: u64,
    prev_action: Option<u8>,
}

#[derive(Serialize)]
struct DecisionInfo {
    action: u8,
    reward: f64,
    terms: fsim_core::reward::RewardTerms,
    ticks: u32,
    status: fsim_core::rl_episode::Status,
    done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    replan: Option<bool>,
}

#[wasm_bindgen]
impl Sim {
    #[wasm_bindgen(constructor)]
    pub fn new(config_json: &str, map: &[u8]) -> Result<Sim, JsValue> {
        let data = Prototypes::from_json(PROTOTYPES).map_err(js_error)?;
        let config = Config::from_json(config_json).map_err(js_error)?;
        let (world, _) = fsim_core::map_format::load_map(map, &data).map_err(js_error)?;
        Ok(Self {
            inner: Simulator::new(data, config, world).map_err(js_error)?,
            raster: None,
            patches: RasterPatches::default(),
            episode: None,
            plan: None,
        })
    }
    pub fn load_map(&mut self, map: &[u8]) -> Result<String, JsValue> {
        let report = self.inner.load_map(map).map_err(js_error)?;
        self.raster = None;
        self.end_episode();
        Ok(json(&report))
    }
    pub fn reset(&mut self) -> Result<(), JsValue> {
        self.inner.reset().map_err(js_error)?;
        self.end_episode();
        Ok(())
    }
    pub fn set_vehicle(&mut self, kind: &str, fuel: &str) -> Result<(), JsValue> {
        self.inner.set_vehicle(kind, fuel).map_err(js_error)?;
        self.end_episode();
        Ok(())
    }
    pub fn step(&mut self, throttle: i32, steer: i32, ticks: u32) -> Result<(), JsValue> {
        self.inner.step(throttle, steer, ticks).map_err(js_error)
    }
    pub fn vehicle_state(&self) -> String {
        json(&self.inner.vehicle_state())
    }
    pub fn target_state(&self) -> String {
        json(&self.inner.target_state())
    }
    pub fn set_target(&mut self, x: f64, y: f64) -> Result<(), JsValue> {
        self.inner.set_target(x, y).map_err(js_error)
    }
    pub fn clear_target(&mut self) {
        self.inner.target = None;
    }
    pub fn map_info(&self) -> String {
        json(&self.inner.world.info)
    }
    pub fn tile_palette(&self) -> String {
        json(&self.inner.data.tiles)
    }
    pub fn tiles(&self) -> Vec<u8> {
        self.inner.world.tiles.to_vec()
    }
    pub fn resources(&self) -> Vec<u8> {
        self.inner.world.resources.to_vec()
    }
    pub fn resource_palette(&self) -> String {
        let mut palette = vec![fsim_core::prototypes::ResourcePrototype {
            id: 0,
            name: "none".into(),
            color: [0, 0, 0],
        }];
        palette.extend(self.inner.data.resources.iter().cloned());
        json(&palette)
    }
    pub fn entities_in_rect(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> Result<String, JsValue> {
        if [x0, y0, x1, y1].iter().any(|x| !x.is_finite()) || x0 >= x1 || y0 >= y1 {
            return Err(js_error(
                "rectangle must be finite with positive width and height",
            ));
        }
        Ok(json(
            &self.inner.world.entities_in_rect([[x0, y0], [x1, y1]]),
        ))
    }
    pub fn map_revision(&self) -> u32 {
        self.inner.world.revision
    }
    pub fn tile_revision(&self) -> u32 {
        self.inner.world.tile_revision
    }
    pub fn observation_spec(&self) -> String {
        json(&observation::spec())
    }
    pub fn observation_local(&self) -> Vec<f32> {
        self.inner.observation_local()
    }
    pub fn observation_coarse(&self) -> Vec<f32> {
        self.inner.observation_coarse()
    }
    pub fn vehicle_list(&self) -> String {
        json(&self.inner.data.vehicles)
    }
    pub fn fuel_list(&self) -> String {
        json(&self.inner.data.fuels)
    }

    // RL episodes (docs/rl.md, "WASM additions").

    /// Policy spec, including `spec_hash`.
    pub fn spec(&self) -> String {
        json(&policy_obs::spec(&self.inner.data))
    }

    /// Starts an episode at an explicit start pose and target. Restores
    /// destroyed obstacles and computes the distance field.
    pub fn start_episode(&mut self, request_json: &str) -> Result<String, JsValue> {
        let request = parse_request(request_json)?;
        let (Some(start), Some(target)) = (request.start, request.target) else {
            return Err(js_error("start_episode needs start and target"));
        };
        if let Some(map) = &request.map {
            if *map != self.inner.world.info.name {
                return Err(js_error(format!(
                    "episode is for map {map}, but {} is loaded",
                    self.inner.world.info.name
                )));
            }
        }
        let setup = EpisodeSetup {
            map: self.inner.world.info.name.clone(),
            vehicle: request
                .vehicle
                .clone()
                .unwrap_or_else(|| self.inner.config.vehicle.clone()),
            fuel: request
                .fuel
                .clone()
                .unwrap_or_else(|| self.inner.config.fuel.clone()),
            start,
            target,
            radius: request
                .radius
                .unwrap_or(self.inner.data.physics.target_radius),
            start_speed: request.start_speed.unwrap_or(0.0),
            seed: request.seed,
        };
        self.begin(setup, request.rules(), None)
    }

    /// Samples an episode with the training sampler for the current vehicle,
    /// then starts it. Same seed, map, vehicle and settings give the same episode.
    pub fn sample_episode(&mut self, request_json: &str) -> Result<String, JsValue> {
        let request = parse_request(request_json)?;
        let seed = request
            .seed
            .ok_or_else(|| js_error("sample_episode needs a seed"))?;
        let defaults = TargetConfig::default();
        let target = TargetConfig {
            min_distance: request.min_distance.unwrap_or(defaults.min_distance),
            max_distance: request.max_distance.unwrap_or(defaults.max_distance),
            max_tortuosity: request.max_tortuosity.unwrap_or(defaults.max_tortuosity),
            radius: request.radius.unwrap_or(defaults.radius),
        };
        let rules = request.rules();
        rules.validate().map_err(js_error)?;
        self.ensure_raster();
        let vehicle = self.inner.vehicles.prototype[0];
        self.inner.world.restore_entities();
        let raster = self.raster.as_ref().expect("raster built");
        let sampled = sampler::sample(
            &self.inner.world,
            raster,
            &self.inner.data,
            vehicle,
            &rules,
            &target,
            seed,
        )
        .map_err(js_error)?;
        let setup = EpisodeSetup {
            map: self.inner.world.info.name.clone(),
            vehicle: self.inner.config.vehicle.clone(),
            fuel: self.inner.config.fuel.clone(),
            start: sampled.start,
            target: sampled.target,
            radius: target.radius,
            start_speed: sampled.start_speed,
            seed: Some(seed),
        };
        self.begin(setup, rules, Some(sampled.field))
    }

    /// Starts an episode toward `(x, y)` from the vehicle's current state: it
    /// keeps its pose, speed and health; the distance field, time limit and
    /// bookkeeping start over. `rules_json` holds optional rule keys (as in
    /// `start_episode`); missing ones keep the current episode's rules, or the
    /// defaults. An unreachable target returns `{"reachable": false}` and
    /// leaves the current episode unchanged.
    pub fn retarget(
        &mut self,
        x: f64,
        y: f64,
        radius: Option<f64>,
        rules_json: Option<String>,
    ) -> Result<String, JsValue> {
        let request: RulesRequest = match rules_json.as_deref() {
            Some(text) => serde_json::from_str(text).map_err(|e| js_error(e.to_string()))?,
            None => RulesRequest::default(),
        };
        let base = self
            .episode
            .as_ref()
            .map_or_else(EpisodeRules::default, |e| e.rules.clone());
        let rules = request.apply(base);
        let radius = radius.unwrap_or(self.inner.data.physics.target_radius);
        let map = self.inner.world.info.name.clone();
        self.ensure_raster();
        let raster = self.raster.as_ref().expect("raster built");
        let ctx = Ctx {
            data: &self.inner.data,
            raster,
            world: &mut self.inner.world,
            vehicles: &mut self.inner.vehicles,
            patches: &mut self.patches,
        };
        let Some(episode) =
            Episode::retarget(ctx, map, Point { x, y }, radius, rules).map_err(js_error)?
        else {
            return Ok(json(&serde_json::json!({ "reachable": false })));
        };
        self.plan = None;
        self.inner.target = Some(Target { x, y, radius });
        let mut info = serde_json::to_value(episode_info(&episode)).expect("episode JSON");
        info["reachable"] = true.into();
        self.episode = Some(episode);
        Ok(json(&info))
    }

    /// Moves the vehicle to a random valid start pose (the training sampler's
    /// rules) with a random heading, full health and zero speed. Restores
    /// destroyed obstacles, ends the episode and clears the target. Returns
    /// the pose JSON `{x, y, orientation}`. Deterministic in `seed`.
    pub fn place_random(&mut self, seed: u32) -> Result<String, JsValue> {
        self.ensure_raster();
        let raster = self.raster.as_ref().expect("raster built");
        let world = &mut self.inner.world;
        world.restore_entities();
        let v = &mut self.inner.vehicles;
        let prototype = v.prototype[0];
        let pose = sampler::sample_start(
            world,
            raster,
            &self.inner.data,
            prototype,
            &EpisodeRules::default().distance_field,
            seed as u64,
            PLACE_ATTEMPTS,
        )
        .map_err(js_error)?;
        self.patches.clear();
        v.x[0] = pose.x;
        v.y[0] = pose.y;
        v.orientation[0] = pose.orientation;
        v.speed[0] = 0.0;
        v.health[0] = self.inner.data.vehicles[prototype].max_health;
        v.tick[0] = 0;
        v.last_collision[0] = None;
        self.inner.target = None;
        self.end_episode();
        Ok(json(&pose))
    }

    /// `null` without an episode; otherwise status, reward totals by term and distances.
    pub fn episode_state(&self) -> String {
        let Some(e) = &self.episode else {
            return "null".into();
        };
        json(&EpisodeState {
            status: e.status,
            ticks: e.ticks,
            decisions: e.decisions(),
            time_limit_ticks: e.time_limit_ticks,
            path_distance: e.path_distance,
            optimal_distance: e.optimal_distance,
            reward: e.totals,
            reward_total: e.totals.total(),
            damage: e.damage,
            collisions: e.collisions,
            prev_action: e.prev_action(),
        })
    }

    pub fn policy_local(&self) -> Result<Vec<u8>, JsValue> {
        let mut out = vec![0; LOCAL_LEN];
        self.write_policy_obs(Some(&mut out), None, None, None, false)?;
        Ok(out)
    }
    pub fn policy_coarse(&self) -> Result<Vec<u8>, JsValue> {
        let mut out = vec![0; COARSE_LEN];
        self.write_policy_obs(None, Some(&mut out), None, None, false)?;
        Ok(out)
    }
    pub fn policy_vector(&self) -> Result<Vec<f32>, JsValue> {
        let mut out = vec![0.0; VECTOR_LEN];
        self.write_policy_obs(None, None, Some(&mut out), None, false)?;
        Ok(out)
    }
    /// The compact student observation (docs/rl.md, "Student observation").
    pub fn policy_student(&self) -> Result<Vec<f32>, JsValue> {
        let mut out = vec![0.0; STUDENT_LEN];
        self.write_policy_obs(None, None, None, Some(&mut out), false)?;
        Ok(out)
    }

    /// Student observation with cost-aware any-angle field guidance.
    pub fn policy_student_smooth(&self) -> Result<Vec<f32>, JsValue> {
        let mut out = vec![0.0; STUDENT_LEN];
        self.write_policy_obs(None, None, None, Some(&mut out), true)?;
        Ok(out)
    }

    /// The student observation with the coarse map appended (docs/rl.md, "Student map").
    pub fn policy_student_map(&self) -> Result<Vec<f32>, JsValue> {
        let mut out = vec![0.0; STUDENT_LEN + student_obs::MAP_LEN];
        self.write_policy_obs(None, None, None, Some(&mut out), false)?;
        Ok(out)
    }

    /// Runs one decision (`decision_ticks` ticks) with reward bookkeeping.
    pub fn decide(&mut self, action: u8) -> Result<String, JsValue> {
        let info = self.run_decision(action)?;
        Ok(json(&info))
    }

    /// Vehicle-frame waypoints (`[x0, y0, x1, y1, ...]`, x = right, y = forward,
    /// as in a `path_planner` action) to world coordinates, clipped to `max_reach`.
    pub fn plan_waypoints(&self, local: &[f32]) -> Result<Vec<f32>, JsValue> {
        let e = self.active_episode()?;
        let plan = Plan::from_local(
            &self.inner.vehicles,
            e.ticks,
            local,
            e.rules.path_planner.max_reach,
        )
        .map_err(js_error)?;
        Ok(flatten(plan.waypoints()))
    }

    /// One pure-pursuit decision along world waypoints. Passing the same
    /// array again continues the plan; a different array starts a new one.
    /// `replan` is true once the env would ask the policy for a new plan.
    pub fn follow_path(&mut self, waypoints: &[f32]) -> Result<String, JsValue> {
        let (ticks, decision_ticks) = {
            let e = self.active_episode()?;
            (e.ticks, e.rules.decision_ticks)
        };
        if waypoints.len() < 2
            || !waypoints.len().is_multiple_of(2)
            || !waypoints.iter().all(|v| v.is_finite())
        {
            return Err(js_error("waypoints must be finite x,y pairs"));
        }
        if self
            .plan
            .as_ref()
            .is_none_or(|(input, _)| input != waypoints)
        {
            let points = waypoints
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| [p[0] as f64, p[1] as f64])
                .collect();
            let plan = Plan::from_world(&self.inner.vehicles, ticks, points);
            self.plan = Some((waypoints.to_vec(), plan));
        }
        let (_, plan) = self.plan.as_ref().expect("plan set");
        let action = plan.action(&self.inner.data, &self.inner.vehicles, decision_ticks);
        let mut info = self.run_decision(action)?;
        let e = self.episode.as_ref().expect("episode active");
        let (_, plan) = self.plan.as_ref().expect("plan set");
        info.replan = Some(plan.finished(&self.inner.vehicles, e, &e.rules.path_planner));
        Ok(json(&info))
    }

    /// Rolls 9-way actions forward on copies of the world and vehicle, one
    /// decision each. Returns x,y after each decision, stopping before the
    /// first decision with a collision.
    pub fn predict_path(&self, actions: &[u8]) -> Result<Vec<f32>, JsValue> {
        let ticks = self
            .episode
            .as_ref()
            .map_or(EpisodeRules::default().decision_ticks, |e| {
                e.rules.decision_ticks
            });
        let mut world = self.inner.world.clone();
        let mut vehicles = self.inner.vehicles.clone();
        let mut out = Vec::with_capacity(actions.len() * 2);
        for &a in actions {
            if a >= 9 {
                return Err(js_error("action index must be 0..9"));
            }
            let (throttle, steer) = fsim_core::rl_episode::decode_action(a);
            let before = vehicles.last_collision[0].clone();
            for _ in 0..ticks {
                physics::step_vehicle(
                    &mut world,
                    &mut vehicles,
                    0,
                    &self.inner.data,
                    throttle,
                    steer,
                );
            }
            if vehicles.last_collision[0] != before || vehicles.health[0] <= 0.0 {
                break;
            }
            out.push(vehicles.x[0] as f32);
            out.push(vehicles.y[0] as f32);
        }
        Ok(out)
    }

    /// The A* baseline path (world x,y pairs) from the vehicle to the target.
    pub fn baseline_path(&self) -> Result<Vec<f32>, JsValue> {
        let e = self.active_episode()?;
        Ok(flatten(&baseline::full_path(e, &self.inner.vehicles)))
    }

    /// Cost-preserving any-angle field path (world x,y pairs).
    pub fn smooth_path(&self) -> Result<Vec<f32>, JsValue> {
        let e = self.active_episode()?;
        let v = &self.inner.vehicles;
        Ok(flatten(&e.field.smooth_route(
            v.x[0],
            v.y[0],
            [e.setup.target.x, e.setup.target.y],
        )))
    }

    /// `{x_min, y_min, width, height}` of the episode's distance field.
    pub fn distance_field(&self) -> Result<String, JsValue> {
        let r = self.active_episode()?.field.region;
        Ok(json(&serde_json::json!({
            "x_min": r.x0, "y_min": r.y0, "width": r.width(), "height": r.height()
        })))
    }

    /// Row-major field values at tile centers; `Infinity` = blocked or unreachable.
    pub fn distance_field_values(&self) -> Result<Vec<f32>, JsValue> {
        Ok(self.active_episode()?.field.values.clone())
    }
}

impl Sim {
    fn end_episode(&mut self) {
        self.episode = None;
        self.plan = None;
    }

    fn ensure_raster(&mut self) {
        if self.raster.is_none() {
            self.raster = Some(MapRaster::build(&self.inner.world, &self.inner.data));
        }
    }

    fn active_episode(&self) -> Result<&Episode, JsValue> {
        self.episode
            .as_ref()
            .ok_or_else(|| js_error("no active episode; call start_episode or sample_episode"))
    }

    fn begin(
        &mut self,
        setup: EpisodeSetup,
        rules: EpisodeRules,
        field: Option<fsim_core::distance_field::DistanceField>,
    ) -> Result<String, JsValue> {
        self.ensure_raster();
        self.end_episode();
        let raster = self.raster.as_ref().expect("raster built");
        let ctx = Ctx {
            data: &self.inner.data,
            raster,
            world: &mut self.inner.world,
            vehicles: &mut self.inner.vehicles,
            patches: &mut self.patches,
        };
        let episode = match field {
            Some(field) => Episode::start_with_field(ctx, setup, rules, field),
            None => Episode::start(ctx, setup, rules),
        }
        .map_err(js_error)?;
        self.inner.config.vehicle = episode.setup.vehicle.clone();
        self.inner.config.fuel = episode.setup.fuel.clone();
        self.inner.target = Some(Target {
            x: episode.setup.target.x,
            y: episode.setup.target.y,
            radius: episode.setup.radius,
        });
        let info = json(&episode_info(&episode));
        self.episode = Some(episode);
        Ok(info)
    }

    fn run_decision(&mut self, action: u8) -> Result<DecisionInfo, JsValue> {
        let raster = self
            .raster
            .as_ref()
            .ok_or_else(|| js_error("no active episode"))?;
        let episode = self
            .episode
            .as_mut()
            .ok_or_else(|| js_error("no active episode; call start_episode or sample_episode"))?;
        let ctx = Ctx {
            data: &self.inner.data,
            raster,
            world: &mut self.inner.world,
            vehicles: &mut self.inner.vehicles,
            patches: &mut self.patches,
        };
        let decision = episode.decide(ctx, action).map_err(js_error)?;
        Ok(DecisionInfo {
            action,
            reward: decision.terms.total(),
            terms: decision.terms,
            ticks: decision.ticks,
            status: episode.status,
            done: episode.status.done(),
            replan: None,
        })
    }

    fn write_policy_obs(
        &self,
        local: Option<&mut [u8]>,
        coarse: Option<&mut [u8]>,
        vector: Option<&mut [f32]>,
        student: Option<&mut [f32]>,
        smooth: bool,
    ) -> Result<(), JsValue> {
        let episode = self.active_episode()?;
        let raster = self.raster.as_ref().expect("raster built with episode");
        let v = &self.inner.vehicles;
        let tile = [v.x[0].floor() as i32, v.y[0].floor() as i32];
        let target = Some((
            [episode.setup.target.x, episode.setup.target.y],
            episode.setup.radius,
        ));
        let mut scratch = ObsScratch::default();
        if let Some(out) = local {
            policy_obs::write_local(raster, &self.patches, tile, target, &mut scratch, out);
        }
        if let Some(out) = coarse {
            policy_obs::write_coarse(raster, &self.patches, tile, target, &mut scratch, out);
        }
        let route = (smooth && student.is_some()).then(|| {
            episode.field.smooth_route(
                v.x[0],
                v.y[0],
                [episode.setup.target.x, episode.setup.target.y],
            )
        });
        let source = ObsSource {
            data: &self.inner.data,
            world: &self.inner.world,
            raster,
            patches: &self.patches,
            vehicles: v,
            episode,
            route: route.as_deref(),
        };
        if let Some(out) = vector {
            policy_obs::write_vector(source, out);
        }
        if let Some(out) = student {
            student_obs::write_student(source, out);
        }
        Ok(())
    }
}

fn episode_info(episode: &Episode) -> EpisodeInfo<'_> {
    EpisodeInfo {
        setup: &episode.setup,
        rules: &episode.rules,
        optimal_distance: episode.optimal_distance,
        time_limit_ticks: episode.time_limit_ticks,
    }
}

fn parse_request(json: &str) -> Result<EpisodeRequest, JsValue> {
    serde_json::from_str(json).map_err(|e| js_error(e.to_string()))
}

fn flatten(points: &[[f64; 2]]) -> Vec<f32> {
    points
        .iter()
        .flat_map(|p| [p[0] as f32, p[1] as f32])
        .collect()
}

fn js_error(message: impl AsRef<str>) -> JsValue {
    JsValue::from_str(message.as_ref())
}
fn json(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).expect("finite simulator state")
}
