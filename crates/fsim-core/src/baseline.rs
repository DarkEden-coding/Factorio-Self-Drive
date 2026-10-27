//! Classical baseline: the shortest path on the episode's distance field plus
//! pure pursuit. The field is an exact cost-to-go, so A* with it as the
//! heuristic expands only path nodes; following its steepest descent gives the
//! same path without a search.
use crate::{
    controller::{pursue, to_vehicle_frame},
    physics,
    prototypes::Prototypes,
    rl_config::PathPlannerConfig,
    rl_episode::{decode_action, Episode},
    vehicle::Vehicles,
    world::World,
};

/// Tiles of path the controller needs per decision.
const PURSUIT_STEPS: usize = 48;

/// World points from the vehicle to the target along the field, at most
/// `max_steps` tiles plus the target itself.
pub fn path(episode: &Episode, vehicles: &Vehicles, max_steps: usize) -> Vec<[f64; 2]> {
    let (x, y) = (vehicles.x[0], vehicles.y[0]);
    let mut points = vec![[x, y]];
    let steps = episode.field.descend(x, y, max_steps);
    let complete = steps.len() < max_steps;
    points.extend(steps);
    if complete {
        points.push([episode.setup.target.x, episode.setup.target.y]);
    }
    points
}

/// The full baseline path, for drawing.
pub fn full_path(episode: &Episode, vehicles: &Vehicles) -> Vec<[f64; 2]> {
    let region = episode.field.region;
    path(episode, vehicles, region.width() * region.height())
}

pub fn action(data: &Prototypes, episode: &Episode, vehicles: &Vehicles) -> u8 {
    let route = path(episode, vehicles, PURSUIT_STEPS);
    pursue(data, vehicles, episode.rules.decision_ticks, &route)
}

/// `horizon` actions from rolling the baseline forward on copies of the world
/// and vehicle.
pub fn plan(
    data: &Prototypes,
    world: &World,
    vehicles: &Vehicles,
    episode: &Episode,
    horizon: usize,
) -> Vec<u8> {
    let mut world = world.clone();
    let mut vehicles = vehicles.clone();
    let mut actions = Vec::with_capacity(horizon);
    for _ in 0..horizon {
        let a = action(data, episode, &vehicles);
        actions.push(a);
        let (throttle, steer) = decode_action(a);
        for _ in 0..episode.rules.decision_ticks {
            physics::step_vehicle(&mut world, &mut vehicles, 0, data, throttle, steer);
        }
    }
    actions
}

/// `waypoints` points evenly spaced along the first `max_reach` tiles of the
/// path, in the vehicle frame (x = right, y = forward).
pub fn waypoints(
    episode: &Episode,
    vehicles: &Vehicles,
    config: &PathPlannerConfig,
) -> Vec<[f64; 2]> {
    let route = path(episode, vehicles, config.max_reach.ceil() as usize * 2);
    let mut lengths = vec![0.0];
    for w in route.windows(2) {
        let last = *lengths.last().unwrap();
        lengths.push(last + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
    }
    let reach = lengths.last().unwrap().min(config.max_reach);
    let (x, y, o) = (vehicles.x[0], vehicles.y[0], vehicles.orientation[0]);
    (1..=config.waypoints)
        .map(|k| {
            let s = reach * k as f64 / config.waypoints as f64;
            let i = lengths
                .partition_point(|l| *l < s)
                .clamp(1, route.len().max(2) - 1);
            let point = if route.len() == 1 {
                route[0]
            } else {
                let span = lengths[i] - lengths[i - 1];
                let t = if span > 0.0 {
                    (s - lengths[i - 1]) / span
                } else {
                    1.0
                };
                let [a, b] = [route[i - 1], route[i]];
                [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
            };
            to_vehicle_frame(o, point[0] - x, point[1] - y)
        })
        .collect()
}
