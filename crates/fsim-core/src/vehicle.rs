use crate::prototypes::{CollisionBox, Prototypes};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CollisionEvent {
    pub tick: u64,
    pub entity_id: u32,
    pub kind: String,
    pub damage_taken: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct VehicleState {
    pub kind: String,
    pub fuel: String,
    pub x: f64,
    pub y: f64,
    pub orientation: f64,
    pub speed: f64,
    pub health: f64,
    pub max_health: f64,
    pub destroyed: bool,
    pub tick: u64,
    pub collision_box: CollisionBox,
    pub last_collision: Option<CollisionEvent>,
}

#[derive(Clone, Debug, Default)]
pub struct Vehicles {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub orientation: Vec<f64>,
    pub speed: Vec<f64>,
    pub health: Vec<f64>,
    pub prototype: Vec<usize>,
    pub fuel: Vec<usize>,
    pub tick: Vec<u64>,
    pub last_collision: Vec<Option<CollisionEvent>>,
}

impl Vehicles {
    pub fn push(&mut self, data: &Prototypes, kind: usize, fuel: usize) -> usize {
        let index = self.x.len();
        self.x.push(0.0);
        self.y.push(0.0);
        self.orientation.push(0.0);
        self.speed.push(0.0);
        self.health.push(data.vehicles[kind].max_health);
        self.prototype.push(kind);
        self.fuel.push(fuel);
        self.tick.push(0);
        self.last_collision.push(None);
        index
    }

    pub fn state(&self, i: usize, data: &Prototypes) -> VehicleState {
        let p = &data.vehicles[self.prototype[i]];
        VehicleState {
            kind: p.name.clone(),
            fuel: data.fuels[self.fuel[i]].name.clone(),
            x: self.x[i],
            y: self.y[i],
            orientation: self.orientation[i],
            speed: self.speed[i],
            health: self.health[i],
            max_health: p.max_health,
            destroyed: self.health[i] <= 0.0,
            tick: self.tick[i],
            collision_box: p.collision_box,
            last_collision: self.last_collision[i].clone(),
        }
    }
}
