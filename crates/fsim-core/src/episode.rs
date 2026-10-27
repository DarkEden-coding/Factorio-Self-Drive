use crate::{
    geometry::Obb,
    observation, physics,
    prototypes::Prototypes,
    target::{Target, TargetState},
    vehicle::{VehicleState, Vehicles},
    world::World,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub vehicle: String,
    pub fuel: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            vehicle: "car".into(),
            fuel: "coal".into(),
        }
    }
}

impl Config {
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }
    fn validate(&self, data: &Prototypes) -> Result<(usize, usize), String> {
        if self.vehicle != "car" && self.vehicle != "tank" {
            return Err("vehicle must be car or tank".into());
        }
        Ok((
            data.vehicle_index(&self.vehicle)?,
            data.fuel_index(&self.fuel)?,
        ))
    }
}

pub struct Simulator {
    pub data: Prototypes,
    pub config: Config,
    pub world: World,
    pub vehicles: Vehicles,
    pub target: Option<Target>,
}

impl Simulator {
    pub fn new(data: Prototypes, config: Config, world: World) -> Result<Self, String> {
        let (kind, fuel) = config.validate(&data)?;
        let mut vehicles = Vehicles::default();
        vehicles.push(&data, kind, fuel);
        let mut sim = Self {
            data,
            config,
            world,
            vehicles,
            target: None,
        };
        sim.reset()?;
        Ok(sim)
    }

    pub fn load_map(&mut self, bytes: &[u8]) -> Result<crate::map_import::MapLoadReport, String> {
        let (mut world, report) = crate::map_format::load_map(bytes, &self.data)?;
        world.revision = self.world.revision.wrapping_add(1);
        world.tile_revision = self.world.tile_revision.wrapping_add(1);
        let mut candidate = Self {
            data: self.data.clone(),
            config: self.config.clone(),
            world,
            vehicles: self.vehicles.clone(),
            target: None,
        };
        candidate.reset()?;
        self.world = candidate.world;
        self.vehicles = candidate.vehicles;
        self.target = None;
        Ok(report)
    }

    pub fn reset(&mut self) -> Result<(), String> {
        let p = &self.data.vehicles[self.vehicles.prototype[0]];
        let [ox, oy] = self.world.spawn;
        let valid = |x, y| {
            !self
                .world
                .blocked(Obb::new(x, y, p.collision_box, 0.0), &self.data)
        };
        let (x, y) = if valid(ox, oy) {
            (ox, oy)
        } else {
            // Search the source-spawn lattice, clamping the search center to
            // the map first so exports far from source origin stay bounded.
            let xmin = (self.world.info.x_min as f64 - ox).ceil() as i64;
            let ymin = (self.world.info.y_min as f64 - oy).ceil() as i64;
            let xmax = (self.world.info.x_min as f64 + self.world.info.width as f64 - 1.0 - ox)
                .floor() as i64;
            let ymax = (self.world.info.y_min as f64 + self.world.info.height as f64 - 1.0 - oy)
                .floor() as i64;
            let cx = 0_i64.clamp(xmin, xmax);
            let cy = 0_i64.clamp(ymin, ymax);
            let mut best = None;
            let mut distance = f64::INFINITY;
            let squared = |x: i64, y: i64| (x as f64).powi(2) + (y as f64).powi(2);
            let extent = self.world.info.width.max(self.world.info.height) as i64;
            for radius in 0..=extent {
                for dy in (cy - radius).max(ymin)..=(cy + radius).min(ymax) {
                    let left = (cx - radius).max(xmin);
                    let right = (cx + radius).min(xmax);
                    let xs: Vec<i64> = if (dy - cy).abs() == radius {
                        (left..=right).collect()
                    } else {
                        [cx - radius, cx + radius]
                            .into_iter()
                            .filter(|x| *x >= xmin && *x <= xmax)
                            .collect()
                    };
                    for dx in xs {
                        let d = squared(dx, dy);
                        if d < distance && valid(ox + dx as f64, oy + dy as f64) {
                            distance = d;
                            best = Some((ox + dx as f64, oy + dy as f64));
                        }
                    }
                }
                // Any unvisited position lies past one of these four sides.
                let mut lower = f64::INFINITY;
                for (x, inside) in [
                    (cx - radius - 1, cx - radius > xmin),
                    (cx + radius + 1, cx + radius < xmax),
                ] {
                    if inside {
                        lower = lower.min(squared(x, cy));
                    }
                }
                for (y, inside) in [
                    (cy - radius - 1, cy - radius > ymin),
                    (cy + radius + 1, cy + radius < ymax),
                ] {
                    if inside {
                        lower = lower.min(squared(cx, y));
                    }
                }
                if lower >= distance {
                    break;
                }
            }
            best.ok_or("no valid spawn for this vehicle")?
        };
        self.vehicles.x[0] = x;
        self.vehicles.y[0] = y;
        self.vehicles.orientation[0] = 0.0;
        self.vehicles.speed[0] = 0.0;
        self.vehicles.health[0] = p.max_health;
        self.vehicles.tick[0] = 0;
        self.vehicles.last_collision[0] = None;
        self.target = None;
        Ok(())
    }

    pub fn set_vehicle(&mut self, kind: &str, fuel: &str) -> Result<(), String> {
        if kind != "car" && kind != "tank" {
            return Err("vehicle must be car or tank".into());
        }
        let prototype = self.data.vehicle_index(kind)?;
        let fuel_index = self.data.fuel_index(fuel)?;
        // The contract preserves position even if a larger replacement overlaps an obstacle.
        self.vehicles.prototype[0] = prototype;
        self.vehicles.fuel[0] = fuel_index;
        self.vehicles.health[0] = self.data.vehicles[prototype].max_health;
        self.vehicles.speed[0] = 0.0;
        self.vehicles.last_collision[0] = None;
        self.config.vehicle = kind.into();
        self.config.fuel = fuel.into();
        Ok(())
    }

    pub fn step(&mut self, throttle: i32, steer: i32, ticks: u32) -> Result<(), String> {
        if !(-1..=1).contains(&throttle) || !(-1..=1).contains(&steer) || ticks == 0 {
            return Err("throttle and steer must be -1, 0, or 1; ticks must be positive".into());
        }
        for _ in 0..ticks {
            if self.vehicles.health[0] <= 0.0 {
                break;
            }
            physics::step_vehicle(
                &mut self.world,
                &mut self.vehicles,
                0,
                &self.data,
                throttle as i8,
                steer as i8,
            );
        }
        Ok(())
    }

    pub fn vehicle_state(&self) -> VehicleState {
        self.vehicles.state(0, &self.data)
    }
    pub fn target_state(&self) -> Option<TargetState> {
        self.target
            .map(|t| t.state(self.vehicles.x[0], self.vehicles.y[0]))
    }
    pub fn set_target(&mut self, x: f64, y: f64) -> Result<(), String> {
        if !x.is_finite() || !y.is_finite() {
            return Err("target coordinates must be finite".into());
        }
        self.target = Some(Target {
            x,
            y,
            radius: self.data.physics.target_radius,
        });
        Ok(())
    }
    pub fn observation_local(&self) -> Vec<f32> {
        observation::local(
            &self.world,
            &self.data,
            self.vehicles.x[0],
            self.vehicles.y[0],
            self.target,
        )
    }
    pub fn observation_coarse(&self) -> Vec<f32> {
        observation::coarse(
            &self.world,
            &self.data,
            self.vehicles.x[0],
            self.vehicles.y[0],
            self.target,
        )
    }
}
