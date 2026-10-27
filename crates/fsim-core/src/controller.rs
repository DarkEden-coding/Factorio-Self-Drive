//! Fixed pure-pursuit controller: picks one of the 9 discrete actions that
//! steers toward a lookahead point on a world-space polyline. Used by the
//! `path_planner` mode, the A* baseline and the wasm `follow_path`.
use crate::{
    physics,
    prototypes::Prototypes,
    rl_config::PathPlannerConfig,
    rl_episode::{encode_action, Episode},
    vehicle::Vehicles,
};
use std::f64::consts::{FRAC_PI_2, TAU};

/// Lookahead distance in tiles: `MIN + PER_SPEED * |speed|`, capped at `MAX`.
const LOOKAHEAD_MIN: f64 = 2.5;
const LOOKAHEAD_PER_SPEED: f64 = 12.0;
const LOOKAHEAD_MAX: f64 = 8.0;
/// Cruise speed cap in tiles/tick (65 km/h) and margin under the turning limit.
const CRUISE_SPEED: f64 = 0.3;
const TURN_MARGIN: f64 = 0.7;
/// Car speed while turning around toward a point behind it.
const CAR_TURNAROUND_SPEED: f64 = 0.06;
/// After a contact, back up for this many ticks while turning toward the path.
const RECOVERY_TICKS: u64 = 30;

/// World offset -> vehicle frame (x = right, y = forward).
pub fn to_vehicle_frame(orientation: f64, dx: f64, dy: f64) -> [f64; 2] {
    let (sin, cos) = (orientation * TAU).sin_cos();
    [dx * cos + dy * sin, dx * sin - dy * cos]
}

/// Vehicle-frame offset -> world offset.
pub fn to_world_offset(orientation: f64, right: f64, forward: f64) -> [f64; 2] {
    let (sin, cos) = (orientation * TAU).sin_cos();
    [right * cos + forward * sin, right * sin - forward * cos]
}

/// Closest point on the polyline: (segment index, segment parameter, distance).
pub fn closest_on_path(path: &[[f64; 2]], x: f64, y: f64) -> (usize, f64, f64) {
    if path.len() == 1 {
        return (0, 0.0, (path[0][0] - x).hypot(path[0][1] - y));
    }
    let mut best = (0, 0.0, f64::INFINITY);
    for i in 0..path.len() - 1 {
        let [ax, ay] = path[i];
        let [bx, by] = path[i + 1];
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 {
            (((x - ax) * dx + (y - ay) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let d = (ax + t * dx - x).hypot(ay + t * dy - y);
        if d < best.2 {
            best = (i, t, d);
        }
    }
    best
}

/// The point `distance` tiles further along the path from `(segment, t)`.
fn advance(path: &[[f64; 2]], segment: usize, t: f64, distance: f64) -> [f64; 2] {
    if path.len() == 1 {
        return path[0];
    }
    let mut left = distance;
    let mut t = t;
    for i in segment..path.len() - 1 {
        let [ax, ay] = path[i];
        let [bx, by] = path[i + 1];
        let len = (bx - ax).hypot(by - ay);
        let remaining = len * (1.0 - t);
        if remaining >= left && len > 0.0 {
            let u = t + left / len;
            return [ax + u * (bx - ax), ay + u * (by - ay)];
        }
        left -= remaining;
        t = 0.0;
    }
    path[path.len() - 1]
}

/// One action following `path` (world points) with vehicle 0.
pub fn pursue(
    data: &Prototypes,
    vehicles: &Vehicles,
    decision_ticks: u32,
    path: &[[f64; 2]],
) -> u8 {
    if path.is_empty() {
        return encode_action(0, 0);
    }
    let p = &data.vehicles[vehicles.prototype[0]];
    let (x, y) = (vehicles.x[0], vehicles.y[0]);
    let speed = vehicles.speed[0];
    let lookahead = (LOOKAHEAD_MIN + LOOKAHEAD_PER_SPEED * speed.abs()).min(LOOKAHEAD_MAX);
    let (segment, t, _) = closest_on_path(path, x, y);
    let goal = advance(path, segment, t, lookahead);
    let local = to_vehicle_frame(vehicles.orientation[0], goal[0] - x, goal[1] - y);
    let reach = local[0].hypot(local[1]);
    let alpha = local[0].atan2(local[1]);
    if reach < 1e-6 {
        return encode_action(if speed > 0.01 { -1 } else { 0 }, 0);
    }
    let recent_contact = vehicles.last_collision[0]
        .as_ref()
        .is_some_and(|c| vehicles.tick[0] < c.tick + RECOVERY_TICKS);
    if recent_contact && alpha.abs() > 0.05 {
        // Reversing flips car steering; tank steering doesn't depend on direction.
        let toward = if alpha > 0.0 { 1 } else { -1 };
        let steer = if p.tank_driving || speed > 0.0 {
            toward
        } else {
            -toward
        };
        return encode_action(-1, steer);
    }
    // Speed whose achievable curvature matches the pure-pursuit curvature.
    let top = physics::terminal_speed(
        data,
        vehicles.prototype[0],
        vehicles.fuel[0],
        data.tile_id("grass-1"),
        false,
    );
    let curvature = (2.0 * alpha.sin() / reach).abs();
    let turn_rate = TAU * p.rotation_speed;
    let mut desired = CRUISE_SPEED.min(0.9 * top);
    if curvature > 1e-6 {
        let limit = if p.tank_driving {
            turn_rate / curvature
        } else {
            (turn_rate / curvature).powi(2)
        };
        desired = desired.min(TURN_MARGIN * limit);
    }
    if alpha.abs() > FRAC_PI_2 {
        desired = if p.tank_driving {
            0.0
        } else {
            CAR_TURNAROUND_SPEED
        };
    }
    // Rolling backward counts as too slow: accelerating forward brakes first.
    let throttle = if speed < -0.001 || speed < desired - 0.01 {
        1
    } else if speed > desired + 0.02 {
        -1
    } else {
        0
    };
    let steer_factor = if p.tank_driving {
        1.0
    } else {
        speed.abs().max(desired).sqrt()
    };
    let turn_per_decision = turn_rate * steer_factor * decision_ticks as f64;
    let deadband = (0.5 * turn_per_decision).max(0.02);
    let steer = if alpha > deadband {
        1
    } else if alpha < -deadband {
        -1
    } else {
        0
    };
    encode_action(throttle, steer)
}

/// A plan ends when the vehicle is this close to its last waypoint.
pub const WAYPOINT_REACHED_TILES: f64 = 2.0;

/// A `path_planner` plan: the vehicle position when planned, then waypoints,
/// all in world coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub path: Vec<[f64; 2]>,
    pub start_tick: u64,
}

impl Plan {
    /// From vehicle-frame waypoints (x = right, y = forward), each clipped to `max_reach`.
    pub fn from_local(
        vehicles: &Vehicles,
        start_tick: u64,
        local: &[f32],
        max_reach: f64,
    ) -> Result<Self, String> {
        let (x, y, o) = (vehicles.x[0], vehicles.y[0], vehicles.orientation[0]);
        let mut points = Vec::with_capacity(local.len() / 2);
        for p in local.as_chunks::<2>().0 {
            let (mut right, mut forward) = (p[0] as f64, p[1] as f64);
            if !right.is_finite() || !forward.is_finite() {
                return Err("waypoints must be finite".into());
            }
            let norm = right.hypot(forward);
            if norm > max_reach {
                right *= max_reach / norm;
                forward *= max_reach / norm;
            }
            let [dx, dy] = to_world_offset(o, right, forward);
            points.push([x + dx, y + dy]);
        }
        Ok(Self::from_world(vehicles, start_tick, points))
    }

    pub fn from_world(vehicles: &Vehicles, start_tick: u64, points: Vec<[f64; 2]>) -> Self {
        let mut path = vec![[vehicles.x[0], vehicles.y[0]]];
        path.extend(points);
        Self { path, start_tick }
    }

    pub fn waypoints(&self) -> &[[f64; 2]] {
        &self.path[1..]
    }

    pub fn action(&self, data: &Prototypes, vehicles: &Vehicles, decision_ticks: u32) -> u8 {
        pursue(data, vehicles, decision_ticks, &self.path)
    }

    /// Whether the policy should replan: the episode ended, `replan_ticks`
    /// elapsed, the vehicle drifted from the path, or it reached the last waypoint.
    pub fn finished(
        &self,
        vehicles: &Vehicles,
        episode: &Episode,
        config: &PathPlannerConfig,
    ) -> bool {
        let (x, y) = (vehicles.x[0], vehicles.y[0]);
        let last = self.path[self.path.len() - 1];
        episode.status.done()
            || episode.ticks - self.start_tick >= config.replan_ticks
            || closest_on_path(&self.path, x, y).2 > config.drift_tiles
            || (x - last[0]).hypot(y - last[1]) <= WAYPOINT_REACHED_TILES
    }
}
