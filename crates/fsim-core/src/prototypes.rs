use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type CollisionBox = [[f64; 2]; 2];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Source {
    pub game_version: String,
    #[serde(default)]
    pub active_mods: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Resistance {
    #[serde(rename = "type")]
    pub damage_type: String,
    pub decrease: f64,
    pub percent: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VehiclePrototype {
    pub name: String,
    pub weight: f64,
    pub braking_power: f64,
    pub consumption: f64,
    pub effectivity: f64,
    pub friction: f64,
    pub rotation_speed: f64,
    pub rotation_snap_angle: f64,
    pub max_health: f64,
    pub collision_box: CollisionBox,
    pub terrain_friction_modifier: f64,
    pub energy_per_hit_point: f64,
    pub resistances: Vec<Resistance>,
    pub tank_driving: bool,
    pub immune_to_all_impacts: bool,
    pub immune_to_tree_impacts: bool,
    pub immune_to_rock_impacts: bool,
    pub immune_to_cliff_impacts: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FuelPrototype {
    pub name: String,
    pub fuel_value: f64,
    pub fuel_acceleration_multiplier: f64,
    pub fuel_top_speed_multiplier: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TilePrototype {
    pub id: u8,
    pub name: String,
    pub color: [u8; 3],
    pub vehicle_friction_modifier: f64,
    pub is_water: bool,
    pub collision_mask: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityPrototype {
    pub name: String,
    pub kind: String,
    pub max_health: f64,
    pub collision_box: CollisionBox,
    pub resistances: Vec<Resistance>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CliffOrientation {
    pub collision_box: CollisionBox,
    pub orientation: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CliffPrototype {
    pub name: String,
    pub max_health: Option<f64>,
    pub grid_size: [f64; 2],
    pub grid_offset: [f64; 2],
    pub orientations: BTreeMap<String, CliffOrientation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResourcePrototype {
    pub id: u8,
    pub name: String,
    pub color: [u8; 3],
}

// Simulator assumptions live separately from exported Factorio prototype values.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PhysicsTuning {
    pub reverse_power_multiplier: f64,
    pub stop_speed: f64,
    pub nest_penalty_radius: f64,
    pub target_radius: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Prototypes {
    pub schema_version: u32,
    pub source: Source,
    pub vehicles: Vec<VehiclePrototype>,
    pub fuels: Vec<FuelPrototype>,
    pub tiles: Vec<TilePrototype>,
    pub entities: Vec<EntityPrototype>,
    pub cliffs: CliffPrototype,
    pub resources: Vec<ResourcePrototype>,
    pub physics: PhysicsTuning,
}

impl Prototypes {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let data: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if data.schema_version != 2 {
            return Err("unsupported prototype schema version".into());
        }
        if data.tiles.len() > 256 || data.tiles.is_empty() {
            return Err("tile palette must contain 1..=256 entries".into());
        }
        for (i, tile) in data.tiles.iter().enumerate() {
            if tile.id as usize != i || tile.vehicle_friction_modifier <= 0.0 {
                return Err("tiles must have contiguous ids and positive friction".into());
            }
        }
        for v in &data.vehicles {
            if v.weight <= 0.0
                || v.energy_per_hit_point <= 0.0
                || v.max_health <= 0.0
                || !(0.0..1.0).contains(&v.friction)
                || !valid_box(v.collision_box)
            {
                return Err(format!("invalid vehicle prototype: {}", v.name));
            }
        }
        for e in &data.entities {
            if !valid_box(e.collision_box) || e.max_health <= 0.0 {
                return Err(format!("invalid entity prototype: {}", e.name));
            }
        }
        for (i, r) in data.resources.iter().enumerate() {
            if r.id as usize != i + 1 {
                return Err("resource ids must start at 1 and be contiguous".into());
            }
        }
        let p = &data.physics;
        if !p.stop_speed.is_finite()
            || p.stop_speed < 0.0
            || !p.reverse_power_multiplier.is_finite()
            || p.reverse_power_multiplier <= 0.0
        {
            return Err("invalid physics tuning".into());
        }
        for name in ["car", "tank"] {
            data.vehicle_index(name)?;
        }
        for name in ["grass-1", "water", "deepwater"] {
            if !data.tiles.iter().any(|t| t.name == name) {
                return Err(format!("missing tile {name}"));
            }
        }
        Ok(data)
    }

    pub fn vehicle_index(&self, name: &str) -> Result<usize, String> {
        self.vehicles
            .iter()
            .position(|v| v.name == name)
            .ok_or_else(|| format!("unknown vehicle: {name}"))
    }

    pub fn fuel_index(&self, name: &str) -> Result<usize, String> {
        self.fuels
            .iter()
            .position(|v| v.name == name)
            .ok_or_else(|| format!("unknown fuel: {name}"))
    }

    pub fn tile_id(&self, name: &str) -> u8 {
        self.tiles
            .iter()
            .find(|v| v.name == name)
            .expect("required fallback tile")
            .id
    }

    pub fn entity_index(&self, name: &str) -> usize {
        self.entities
            .iter()
            .position(|v| v.name == name)
            .expect("required fallback entity")
    }
}

fn valid_box(b: CollisionBox) -> bool {
    b[0][0] < b[1][0] && b[0][1] < b[1][1]
}
