//! Replay post-step calibration samples without changing the simulator's physics.
use crate::{
    map_import::{add_entity, dimensions, validate_entity, MapEntity},
    physics,
    prototypes::Prototypes,
    vehicle::Vehicles,
    world::World,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Trace {
    pub schema_version: u32,
    pub header: TraceHeader,
    pub samples: Vec<TraceSample>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TraceHeader {
    pub name: String,
    pub surface: TraceSurface,
    pub vehicle: TraceVehicle,
    pub fuel: String,
    pub obstacles: Vec<MapEntity>,
    pub inputs: Vec<InputSegment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TraceSurface {
    pub tile: String,
    pub area: [[i32; 2]; 2],
    pub water: Vec<TilePatch>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TilePatch {
    pub name: String,
    pub area: [[i32; 2]; 2],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TraceVehicle {
    pub name: String,
    pub position: [f64; 2],
    pub orientation: f64,
    pub speed: f64,
    pub health: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Inputs {
    pub throttle: i8,
    pub steer: i8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InputSegment {
    pub ticks: u32,
    pub throttle: i8,
    pub steer: i8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TraceSample {
    pub tick: u64,
    pub inputs: Inputs,
    pub x: f64,
    pub y: f64,
    pub orientation: f64,
    pub speed: f64,
    pub health: f64,
    pub obstacle_health: Vec<f64>,
}

#[derive(Debug, Default, Serialize)]
pub struct ErrorStats {
    pub max: f64,
    pub rms: f64,
    #[serde(skip)]
    count: usize,
    #[serde(skip)]
    sum_squares: f64,
}
impl ErrorStats {
    fn add(&mut self, error: f64) {
        self.max = self.max.max(error.abs());
        self.sum_squares += error * error;
        self.count += 1;
        self.rms = (self.sum_squares / self.count as f64).sqrt();
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ReplayReport {
    pub name: String,
    pub samples: usize,
    pub position: ErrorStats,
    pub orientation: ErrorStats,
    pub speed: ErrorStats,
    pub health: ErrorStats,
    pub obstacle_health: ErrorStats,
    pub first_position_error_tick: Option<u64>,
}

pub fn reconstruct(header: &TraceHeader, data: &Prototypes) -> Result<(World, Vehicles), String> {
    let [width, height] = dimensions(header.surface.area)?;
    let area = header.surface.area;
    if area[0] != [-(width as i32 / 2), -(height as i32 / 2)] {
        return Err("trace surface must be centered on the origin".into());
    }
    let tile = |name: &str| {
        data.tiles
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.id)
            .ok_or_else(|| format!("unknown trace tile {name}; import matching prototypes first"))
    };
    let mut world = World::empty_rect(width, height, 1, tile(&header.surface.tile)?);
    for patch in &header.surface.water {
        if patch.area[0][0] < area[0][0]
            || patch.area[0][1] < area[0][1]
            || patch.area[1][0] > area[1][0]
            || patch.area[1][1] > area[1][1]
            || patch.area[0][0] >= patch.area[1][0]
            || patch.area[0][1] >= patch.area[1][1]
        {
            return Err("invalid trace tile patch".into());
        }
        let id = tile(&patch.name)?;
        for y in patch.area[0][1]..patch.area[1][1] {
            for x in patch.area[0][0]..patch.area[1][0] {
                let index = world.index(x, y).unwrap();
                std::sync::Arc::make_mut(&mut world.tiles)[index] = id;
            }
        }
    }
    let mut warnings = BTreeSet::new();
    for obstacle in &header.obstacles {
        validate_entity(obstacle)?;
        add_entity(&mut world, data, obstacle, &mut warnings)?;
    }
    if !warnings.is_empty() {
        return Err(format!(
            "trace requires exact prototypes: {}",
            warnings.into_iter().collect::<Vec<_>>().join("; ")
        ));
    }
    let v = &header.vehicle;
    if v.position
        .iter()
        .chain([&v.orientation, &v.speed, &v.health])
        .any(|v| !v.is_finite())
        || v.health <= 0.0
        || !(0.0..1.0).contains(&v.orientation)
    {
        return Err("invalid initial vehicle state".into());
    }
    let mut vehicles = Vehicles::default();
    vehicles.push(
        data,
        data.vehicle_index(&v.name)?,
        data.fuel_index(&header.fuel)?,
    );
    vehicles.x[0] = v.position[0];
    vehicles.y[0] = v.position[1];
    vehicles.orientation[0] = v.orientation;
    vehicles.speed[0] = v.speed;
    vehicles.health[0] = v.health;
    Ok((world, vehicles))
}

pub fn replay(trace: &Trace, data: &Prototypes) -> Result<ReplayReport, String> {
    if trace.schema_version != 1 || trace.samples.is_empty() {
        return Err("expected trace schema 1 with tick-zero and post-step samples".into());
    }
    for input in &trace.header.inputs {
        if input.ticks == 0
            || !(-1..=1).contains(&input.throttle)
            || !(-1..=1).contains(&input.steer)
        {
            return Err("invalid input segment".into());
        }
    }
    let (mut world, mut vehicles) = reconstruct(&trace.header, data)?;
    let mut report = ReplayReport {
        name: trace.header.name.clone(),
        ..ReplayReport::default()
    };
    let mut segment = 0;
    let mut remaining = trace.header.inputs.first().map_or(0, |s| s.ticks);
    for (tick, sample) in trace.samples.iter().enumerate() {
        if sample.tick != tick as u64
            || [
                sample.x,
                sample.y,
                sample.orientation,
                sample.speed,
                sample.health,
            ]
            .iter()
            .any(|v| !v.is_finite())
            || sample.obstacle_health.len() != world.entities.len()
            || sample
                .obstacle_health
                .iter()
                .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err("invalid or nonconsecutive trace sample".into());
        }
        if tick > 0 {
            let input = trace
                .header
                .inputs
                .get(segment)
                .ok_or("samples exceed input sequence")?;
            if sample.inputs
                != (Inputs {
                    throttle: input.throttle,
                    steer: input.steer,
                })
            {
                return Err(format!("sample inputs disagree with header at tick {tick}"));
            }
            physics::step_vehicle(
                &mut world,
                &mut vehicles,
                0,
                data,
                sample.inputs.throttle,
                sample.inputs.steer,
            );
            remaining -= 1;
            if remaining == 0 {
                segment += 1;
                remaining = trace.header.inputs.get(segment).map_or(0, |s| s.ticks);
            }
        }
        let position = (sample.x - vehicles.x[0]).hypot(sample.y - vehicles.y[0]);
        let orientation =
            (sample.orientation - vehicles.orientation[0] + 0.5).rem_euclid(1.0) - 0.5;
        report.position.add(position);
        report.orientation.add(orientation);
        report.speed.add(sample.speed - vehicles.speed[0]);
        report.health.add(sample.health - vehicles.health[0]);
        for (index, expected) in sample.obstacle_health.iter().enumerate() {
            report.obstacle_health.add(expected - world.health(index));
        }
        if position > 0.05 && report.first_position_error_tick.is_none() {
            report.first_position_error_tick = Some(sample.tick);
        }
        report.samples += 1;
    }
    Ok(report)
}
