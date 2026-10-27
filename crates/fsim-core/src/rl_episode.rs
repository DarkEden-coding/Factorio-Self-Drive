//! One RL episode: start pose, target, distance field, termination and reward
//! bookkeeping (docs/rl.md, "Episodes" and "Reward"). Shared by the VecEnv and
//! the wasm `Sim`, so training and the viewer score identically.
use crate::{
    distance_field::{polyline_length, DistanceField, FieldParams, Region},
    geometry::Obb,
    physics,
    prototypes::Prototypes,
    raster::{MapRaster, RasterPatches},
    reward::RewardTerms,
    rl_config::{ArrivalMode, EpisodeRules},
    vehicle::Vehicles,
    world::World,
};
use serde::{Deserialize, Serialize};

pub const ACTIONS: usize = 9;

/// Collision kinds counted separately; the map edge counts as water.
pub const COLLISION_KINDS: [&str; 5] = ["tree", "rock", "cliff", "nest", "water"];

/// Backing up at least this far (tiles) counts as having reversed.
pub const REVERSED_TILES: f64 = 0.5;

/// `index = (throttle + 1) * 3 + (steer + 1)`.
pub fn decode_action(action: u8) -> (i8, i8) {
    (action as i8 / 3 - 1, action as i8 % 3 - 1)
}

pub fn encode_action(throttle: i8, steer: i8) -> u8 {
    ((throttle + 1) * 3 + steer + 1) as u8
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    pub orientation: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Everything that, with the map and inputs, determines an episode.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct EpisodeSetup {
    pub map: String,
    pub vehicle: String,
    pub fuel: String,
    pub start: Pose,
    pub target: Point,
    pub radius: f64,
    /// Signed start speed in tiles/tick along the start heading.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub start_speed: f64,
    /// Sampler seed, when the episode was sampled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Success,
    Destroyed,
    Timeout,
    Stalled,
}

impl Status {
    pub fn terminated(self) -> bool {
        matches!(self, Status::Success | Status::Destroyed)
    }
    pub fn truncated(self) -> bool {
        matches!(self, Status::Timeout | Status::Stalled)
    }
    pub fn done(self) -> bool {
        self != Status::Running
    }
}

/// The mutable simulation state an episode drives. The wasm `Sim` and each
/// VecEnv env own these separately.
pub struct Ctx<'a> {
    pub data: &'a Prototypes,
    pub raster: &'a MapRaster,
    pub world: &'a mut World,
    pub vehicles: &'a mut Vehicles,
    pub patches: &'a mut RasterPatches,
}

#[derive(Clone, Debug)]
pub struct Episode {
    pub setup: EpisodeSetup,
    pub rules: EpisodeRules,
    pub field: DistanceField,
    pub optimal_distance: f64,
    /// Geometric length of the field's route from the start, for SPL. Equals
    /// `optimal_distance` when the route has no extra tree or zone cost.
    pub optimal_length: f64,
    /// Field value at the vehicle; keeps the last finite value if the
    /// vehicle enters a tile the field can't evaluate.
    pub path_distance: f64,
    pub time_limit_ticks: u64,
    pub status: Status,
    pub ticks: u64,
    /// Undiscounted reward totals.
    pub totals: RewardTerms,
    pub damage: f64,
    /// Ticks with a collision event (entities, water or map edge).
    pub collisions: u64,
    /// `collisions` by `COLLISION_KINDS`.
    pub collisions_by_kind: [u64; 5],
    /// Ticks and decisions ending with the vehicle center in a nest zone.
    pub nest_zone_ticks: u64,
    pub nest_zone_decisions: u64,
    /// Distance moved backward (against the heading), in tiles.
    pub reverse_distance: f64,
    /// Distance the vehicle center actually moved, in tiles.
    pub path_length: f64,
    /// Executed actions, one per decision.
    pub actions: Vec<u8>,
    /// The center has been within the target radius (pays `arrival` once).
    pub reached: bool,
    best_distance: f64,
    best_tick: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Decision {
    pub terms: RewardTerms,
    pub ticks: u32,
}

impl Episode {
    /// Restores the world's obstacles, places the vehicle at the start pose
    /// and computes the distance field. Fails if the target is unreachable.
    pub fn start(ctx: Ctx<'_>, setup: EpisodeSetup, rules: EpisodeRules) -> Result<Self, String> {
        Self::start_inner(ctx, setup, rules, None)
    }

    /// Like `start`, reusing a field the sampler already computed for this
    /// exact vehicle, start, target and rules.
    pub fn start_with_field(
        ctx: Ctx<'_>,
        setup: EpisodeSetup,
        rules: EpisodeRules,
        field: DistanceField,
    ) -> Result<Self, String> {
        Self::start_inner(ctx, setup, rules, Some(field))
    }

    fn start_inner(
        ctx: Ctx<'_>,
        setup: EpisodeSetup,
        rules: EpisodeRules,
        field: Option<DistanceField>,
    ) -> Result<Self, String> {
        rules.validate()?;
        let Pose { x, y, orientation } = setup.start;
        if ![
            x,
            y,
            orientation,
            setup.target.x,
            setup.target.y,
            setup.radius,
            setup.start_speed,
        ]
        .iter()
        .all(|v| v.is_finite())
            || setup.radius <= 0.0
        {
            return Err("episode start, target and radius must be finite; radius > 0".into());
        }
        if setup.vehicle != "car" && setup.vehicle != "tank" {
            return Err("vehicle must be car or tank".into());
        }
        let data = ctx.data;
        let prototype = data.vehicle_index(&setup.vehicle)?;
        let fuel = data.fuel_index(&setup.fuel)?;
        ctx.world.restore_entities();
        ctx.patches.clear();
        let v = &mut *ctx.vehicles;
        v.prototype[0] = prototype;
        v.fuel[0] = fuel;
        v.x[0] = x;
        v.y[0] = y;
        v.orientation[0] = orientation.rem_euclid(1.0);
        v.speed[0] = setup.start_speed;
        v.health[0] = data.vehicles[prototype].max_health;
        v.tick[0] = 0;
        v.last_collision[0] = None;
        let field = field.unwrap_or_else(|| {
            episode_field(
                ctx.raster,
                data,
                prototype,
                &rules,
                [x, y],
                [setup.target.x, setup.target.y],
            )
        });
        Self::with_field(setup, rules, field)
    }

    /// Starts an episode from the vehicle's current state toward a new target,
    /// for the viewer's live session. Unlike `start`, the vehicle keeps its
    /// pose, speed and health and destroyed obstacles stay destroyed; only the
    /// distance field and the bookkeeping (time limit, stall timer, rewards,
    /// actions) start over. `Ok(None)` if the target is unreachable.
    pub fn retarget(
        ctx: Ctx<'_>,
        map: String,
        target: Point,
        radius: f64,
        rules: EpisodeRules,
    ) -> Result<Option<Self>, String> {
        rules.validate()?;
        if !target.x.is_finite() || !target.y.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return Err("target and radius must be finite; radius > 0".into());
        }
        let v = &*ctx.vehicles;
        if v.health[0] <= 0.0 {
            return Err("the vehicle is destroyed".into());
        }
        let data = ctx.data;
        let prototype = v.prototype[0];
        let start = Pose {
            x: v.x[0],
            y: v.y[0],
            orientation: v.orientation[0],
        };
        let setup = EpisodeSetup {
            map,
            vehicle: data.vehicles[prototype].name.clone(),
            fuel: data.fuels[v.fuel[0]].name.clone(),
            start,
            target,
            radius,
            start_speed: v.speed[0],
            seed: None,
        };
        let field = episode_field(
            ctx.raster,
            data,
            prototype,
            &rules,
            [start.x, start.y],
            [target.x, target.y],
        );
        if !field.sample(start.x, start.y).is_finite() {
            return Ok(None);
        }
        Self::with_field(setup, rules, field).map(Some)
    }

    /// Fresh bookkeeping for a vehicle at `setup.start` and its field.
    fn with_field(
        setup: EpisodeSetup,
        rules: EpisodeRules,
        field: DistanceField,
    ) -> Result<Self, String> {
        let optimal_distance = field.sample(setup.start.x, setup.start.y);
        if !optimal_distance.is_finite() {
            return Err("target is unreachable from the start pose".into());
        }
        let target = [setup.target.x, setup.target.y];
        let optimal_length = polyline_length(&field.route(setup.start.x, setup.start.y, target));
        let limit = &rules.time_limit;
        let time_limit_ticks =
            (limit.base_ticks + limit.per_tile_ticks * optimal_distance).ceil() as u64;
        Ok(Self {
            setup,
            rules,
            field,
            optimal_distance,
            optimal_length,
            path_distance: optimal_distance,
            time_limit_ticks: time_limit_ticks.max(1),
            status: Status::Running,
            ticks: 0,
            totals: RewardTerms::default(),
            damage: 0.0,
            collisions: 0,
            collisions_by_kind: [0; 5],
            nest_zone_ticks: 0,
            nest_zone_decisions: 0,
            reverse_distance: 0.0,
            path_length: 0.0,
            actions: Vec::new(),
            reached: false,
            best_distance: optimal_distance,
            best_tick: 0,
        })
    }

    /// An inert finished episode, for owners that start a real one before use.
    pub(crate) fn placeholder() -> Self {
        Self {
            setup: EpisodeSetup {
                map: String::new(),
                vehicle: String::new(),
                fuel: String::new(),
                start: Pose {
                    x: 0.0,
                    y: 0.0,
                    orientation: 0.0,
                },
                target: Point { x: 0.0, y: 0.0 },
                radius: 1.0,
                start_speed: 0.0,
                seed: None,
            },
            rules: EpisodeRules::default(),
            field: DistanceField {
                region: Region {
                    x0: 0,
                    y0: 0,
                    x1: 0,
                    y1: 0,
                },
                values: Vec::new(),
            },
            optimal_distance: 0.0,
            optimal_length: 0.0,
            path_distance: 0.0,
            time_limit_ticks: 1,
            status: Status::Timeout,
            ticks: 0,
            totals: RewardTerms::default(),
            damage: 0.0,
            collisions: 0,
            collisions_by_kind: [0; 5],
            nest_zone_ticks: 0,
            nest_zone_decisions: 0,
            reverse_distance: 0.0,
            path_length: 0.0,
            actions: Vec::new(),
            reached: false,
            best_distance: 0.0,
            best_tick: 0,
        }
    }

    pub fn decisions(&self) -> usize {
        self.actions.len()
    }

    /// Ticks since path distance last improved by a tile.
    pub fn stall_ticks(&self) -> u64 {
        self.ticks - self.best_tick
    }

    pub fn prev_action(&self) -> Option<u8> {
        self.actions.last().copied()
    }

    pub fn reversed(&self) -> bool {
        self.reverse_distance >= REVERSED_TILES
    }

    /// Distance from the vehicle center to the target point.
    pub fn target_distance(&self, vehicles: &Vehicles) -> f64 {
        let t = self.setup.target;
        (vehicles.x[0] - t.x).hypot(vehicles.y[0] - t.y)
    }

    /// Whether the vehicle has arrived under the episode's arrival rule.
    fn arrived(&self, vehicles: &Vehicles) -> bool {
        let distance = self.target_distance(vehicles);
        let a = &self.rules.arrival;
        match a.mode {
            ArrivalMode::Enter => distance <= self.setup.radius,
            ArrivalMode::Stop => {
                distance <= a.stop_radius && vehicles.speed[0].abs() <= a.stop_speed
            }
        }
    }

    /// Runs one decision (`decision_ticks` ticks, fewer if the episode ends).
    pub fn decide(&mut self, ctx: Ctx<'_>, action: u8) -> Result<Decision, String> {
        if action as usize >= ACTIONS {
            return Err("action index must be 0..9".into());
        }
        if self.status.done() {
            return Err("episode is finished".into());
        }
        let (throttle, steer) = decode_action(action);
        let v = &mut *ctx.vehicles;
        let max_health = ctx.data.vehicles[v.prototype[0]].max_health;
        let health_before = v.health[0];
        let distance_before = self.path_distance;
        let mut collided = false;
        let mut reached_now = false;
        let mut ticks = 0;
        let weights = self.rules.reward;
        let mut terms = RewardTerms {
            time: weights.time,
            ..RewardTerms::default()
        };
        while ticks < self.rules.decision_ticks {
            let (x0, y0, o0) = (v.x[0], v.y[0], v.orientation[0]);
            physics::step_vehicle(ctx.world, v, 0, ctx.data, throttle, steer);
            let (dx, dy) = (v.x[0] - x0, v.y[0] - y0);
            self.path_length += dx.hypot(dy);
            let (sin, cos) = (o0 * std::f64::consts::TAU).sin_cos();
            if dx * sin - dy * cos < 0.0 {
                self.reverse_distance += dx.hypot(dy);
            }
            ticks += 1;
            self.ticks += 1;
            if let Some(c) = v.last_collision[0].as_ref().filter(|c| c.tick == v.tick[0]) {
                collided = true;
                self.collisions += 1;
                if let Some(k) = COLLISION_KINDS.iter().position(|k| *k == c.kind) {
                    self.collisions_by_kind[k] += 1;
                }
            }
            if in_nest_zone(ctx.world, ctx.raster, v.x[0], v.y[0]) {
                self.nest_zone_ticks += 1;
            }
            let distance = self.field.sample(v.x[0], v.y[0]);
            if distance.is_finite() {
                self.path_distance = distance;
            }
            if self.path_distance < self.best_distance - 1.0 {
                self.best_distance = self.path_distance;
                self.best_tick = self.ticks;
            }
            if !self.reached && self.target_distance(v) <= self.setup.radius {
                self.reached = true;
                reached_now = true;
            }
            self.status = if v.health[0] <= 0.0 {
                Status::Destroyed
            } else if self.arrived(v) {
                Status::Success
            } else if self.ticks >= self.time_limit_ticks {
                Status::Timeout
            } else if self.ticks - self.best_tick >= self.rules.time_limit.stall_ticks {
                Status::Stalled
            } else {
                Status::Running
            };
            if self.status.done() {
                break;
            }
        }
        ctx.patches.sync(ctx.world, ctx.raster, ctx.data);
        let damage = (health_before - v.health[0]).max(0.0);
        self.damage += damage;
        terms.progress = weights.progress * (distance_before - self.path_distance);
        terms.damage = weights.damage * damage / max_health;
        if collided {
            terms.collision = weights.collision;
        }
        if in_nest_zone(ctx.world, ctx.raster, v.x[0], v.y[0]) {
            terms.nest_zone = weights.nest_zone;
            self.nest_zone_decisions += 1;
        }
        if self.rules.arrival.mode == ArrivalMode::Stop {
            if reached_now {
                terms.arrival = weights.arrival;
            }
            if self.target_distance(v) <= self.setup.radius {
                terms.target_speed = weights.target_speed * v.speed[0].abs();
            }
        }
        match self.status {
            Status::Success => terms.success = weights.success,
            Status::Destroyed => terms.destroyed = weights.destroyed,
            _ => {}
        }
        self.totals.add_scaled(&terms, 1.0);
        self.actions.push(action);
        Ok(Decision { terms, ticks })
    }
}

/// The distance field toward `target` over the start/target box plus margin.
pub fn episode_field(
    raster: &MapRaster,
    data: &Prototypes,
    vehicle: usize,
    rules: &EpisodeRules,
    start: [f64; 2],
    target: [f64; 2],
) -> DistanceField {
    let params = FieldParams::new(&data.vehicles[vehicle], &rules.distance_field);
    let region = Region::around(&[start, target], rules.distance_field.margin, raster);
    DistanceField::compute(raster, region, params, target, f64::INFINITY)
}

/// Whether `(x, y)` is within a live nest's penalty radius.
pub fn in_nest_zone(world: &World, raster: &MapRaster, x: f64, y: f64) -> bool {
    raster.nests.iter().any(|&i| {
        let e = &world.entities[i];
        e.penalty_radius
            .is_some_and(|r| (x - e.x).hypot(y - e.y) <= r)
            && world.alive(i)
    })
}

/// Whether the vehicle hull fits at `pose` without touching obstacles or water.
pub fn pose_is_clear(world: &World, data: &Prototypes, vehicle: usize, pose: Pose) -> bool {
    let shape = Obb::new(
        pose.x,
        pose.y,
        data.vehicles[vehicle].collision_box,
        pose.orientation,
    );
    !world.blocked(shape, data)
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}
