use crate::{
    collision,
    geometry::Obb,
    prototypes::Prototypes,
    vehicle::{CollisionEvent, Vehicles},
    world::World,
};
use std::f64::consts::TAU;

pub const TICKS_PER_SECOND: f64 = 60.0;

pub fn step_vehicle(
    world: &mut World,
    vehicles: &mut Vehicles,
    i: usize,
    data: &Prototypes,
    throttle: i8,
    steer: i8,
) {
    if vehicles.health[i] <= 0.0 {
        return;
    }
    vehicles.tick[i] += 1;
    let p = &data.vehicles[vehicles.prototype[i]];
    let fuel = &data.fuels[vehicles.fuel[i]];
    let tuning = &data.physics;
    let old_speed = vehicles.speed[i];
    let tile_friction = terrain_friction(world, vehicles, i, data);
    let friction = p.friction * (1.0 + (tile_friction - 1.0) * p.terrain_friction_modifier);
    let retention = (1.0 - friction).clamp(0.0, 1.0);
    let mut energy = 0.5 * p.weight * (old_speed * TICKS_PER_SECOND).powi(2);
    let braking = throttle != 0 && old_speed * (throttle as f64) < 0.0;
    let direction = if braking || throttle == 0 {
        old_speed.signum()
    } else {
        throttle as f64
    };
    if braking {
        energy = (energy - p.braking_power / TICKS_PER_SECOND).max(0.0);
    } else if throttle != 0 {
        let reverse = if throttle < 0 {
            tuning.reverse_power_multiplier
        } else {
            1.0
        };
        energy += p.consumption
            * p.effectivity
            * fuel.fuel_acceleration_multiplier
            * fuel.fuel_top_speed_multiplier.powi(2)
            * reverse
            / TICKS_PER_SECOND;
    }
    let mut speed = direction * (2.0 * energy / p.weight).sqrt() / TICKS_PER_SECOND * retention;
    if speed.abs() < tuning.stop_speed && !(p.tank_driving && steer != 0) {
        speed = 0.0;
    }
    vehicles.speed[i] = speed;
    // Factorio truncates each displacement, not the resulting absolute position.
    // Translation uses the heading before this tick's steering update.
    let orientation = vehicles.orientation[i];
    let steering_factor = if p.tank_driving {
        1.0
    } else {
        speed.abs().sqrt() * speed.signum()
    };
    let turn = steer as f64 * p.rotation_speed * steering_factor;
    let mut next_orientation = orientation;
    if speed != 0.0 || (p.tank_driving && steer != 0) {
        next_orientation = (orientation + turn).rem_euclid(1.0);
        if steer == 0 && speed != 0.0 {
            let snapped = (next_orientation * 8.0).round() / 8.0;
            if (next_orientation - snapped).abs() < p.rotation_snap_angle {
                next_orientation = snapped.rem_euclid(1.0);
            }
        }
        next_orientation = quantize_orientation(next_orientation);
    }
    let x = vehicles.x[i] + quantize_position((orientation * TAU).sin() * speed);
    let y = vehicles.y[i] + quantize_position(-(orientation * TAU).cos() * speed);
    let shape = Obb::new(x, y, p.collision_box, next_orientation);
    let mut blocked = false;
    if world.water(x.floor() as i32, y.floor() as i32, data) {
        vehicles.last_collision[i] = Some(CollisionEvent {
            tick: vehicles.tick[i],
            entity_id: 0,
            kind: "water".into(),
            damage_taken: 0.0,
        });
        vehicles.speed[i] = 0.0;
        blocked = true;
    } else {
        // This is movement energy in game units, distinct from engine energy.
        let rotation_energy = if p.tank_driving { turn.powi(2) } else { 0.0 };
        let energy =
            p.weight * (old_speed.powi(2) + 0.5 * ((speed - old_speed).powi(2) + rotation_energy));
        for entity in world.candidates(shape.aabb()) {
            if shape.intersects(world.entities[entity].shape()) {
                collision::impact_with_energy(world, vehicles, i, entity, data, energy);
                blocked = true;
                break;
            }
        }
    }
    if !blocked {
        vehicles.x[i] = x;
        vehicles.y[i] = y;
        vehicles.orientation[i] = next_orientation;
    }
}

fn terrain_friction(world: &World, vehicles: &Vehicles, i: usize, data: &Prototypes) -> f64 {
    let p = &data.vehicles[vehicles.prototype[i]];
    let shape = Obb::new(
        vehicles.x[i],
        vehicles.y[i],
        p.collision_box,
        vehicles.orientation[i],
    );
    let bounds = shape.aabb();
    let mut total = 0.0;
    let mut count = 0;
    for y in bounds[0][1].floor() as i32..bounds[1][1].ceil() as i32 {
        for x in bounds[0][0].floor() as i32..bounds[1][0].ceil() as i32 {
            if shape.intersects(Obb::new(x as f64, y as f64, [[0.0, 0.0], [1.0, 1.0]], 0.0)) {
                total += world
                    .tile(x, y)
                    .map_or(1.0, |id| data.tiles[id as usize].vehicle_friction_modifier);
                count += 1;
            }
        }
    }
    if count == 0 {
        1.0
    } else {
        total / count as f64
    }
}

pub fn quantize_position(value: f64) -> f64 {
    (value * 256.0).trunc() / 256.0
}

/// The engine stores a direction on a signed 16-bit integer circle. Only the
/// minor component is rounded; the major component follows from the radius.
/// Converting the first octant before reflecting it preserves the engine's
/// asymmetric least-significant bit at quadrant boundaries.
pub fn quantize_orientation(value: f64) -> f64 {
    const RADIUS: f64 = 32767.0;
    const TURN_UNITS: f64 = 4294967296.0;
    let angle = value.rem_euclid(1.0) as f32 as f64 * TAU;
    let (s, c) = angle.sin_cos();
    let minor = (s.abs().min(c.abs()) * RADIUS).round();
    let major = (RADIUS * RADIUS - minor * minor).sqrt().round();
    let mut orientation = (minor.atan2(major) / TAU * TURN_UNITS).trunc() / TURN_UNITS;
    if s.abs() > c.abs() {
        orientation = 0.25 - orientation;
    }
    if c < 0.0 {
        orientation = 0.5 - orientation;
    }
    if s < 0.0 {
        orientation = 1.0 - orientation;
    }
    orientation.rem_euclid(1.0)
}

pub fn terminal_speed(
    data: &Prototypes,
    vehicle: usize,
    fuel: usize,
    tile: u8,
    reverse: bool,
) -> f64 {
    let p = &data.vehicles[vehicle];
    let friction = p.friction
        * (1.0
            + (data.tiles[tile as usize].vehicle_friction_modifier - 1.0)
                * p.terrain_friction_modifier);
    let r2 = (1.0 - friction).powi(2);
    let power = p.consumption
        * p.effectivity
        * data.fuels[fuel].fuel_acceleration_multiplier
        * data.fuels[fuel].fuel_top_speed_multiplier.powi(2)
        * if reverse {
            data.physics.reverse_power_multiplier
        } else {
            1.0
        };
    (2.0 * power / TICKS_PER_SECOND / p.weight * r2 / (1.0 - r2)).sqrt() / TICKS_PER_SECOND
}
