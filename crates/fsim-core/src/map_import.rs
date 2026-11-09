//! Name-based interchange with the export mod. Unknown prototypes use semantic fallbacks.
use crate::{
    prototypes::Prototypes,
    world::{Entity, World},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapExport {
    pub schema_version: u32,
    pub area: [[i32; 2]; 2],
    #[serde(default)]
    pub seed: u32,
    pub chunks: Vec<MapChunk>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapChunk {
    pub position: [i32; 2],
    /// Exactly 1024 tile names, row-major within this chunk.
    pub tiles: Vec<String>,
    pub resources: Vec<MapResource>,
    pub entities: Vec<MapEntity>,
    /// Demolisher territory (Vulcanus).
    #[serde(default)]
    pub territory: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapResource {
    pub name: String,
    pub position: [i32; 2],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapEntity {
    pub name: String,
    pub kind: String,
    pub position: [f64; 2],
    #[serde(default)]
    pub orientation: f64,
    pub cliff_orientation: Option<String>,
    pub health: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct MapLoadReport {
    pub name: String,
    /// Factorio position = simulator position + offset.
    pub offset: [i32; 2],
    pub warnings: Vec<String>,
}

pub fn dimensions(area: [[i32; 2]; 2]) -> Result<[u32; 2], String> {
    let width = area[1][0] as i64 - area[0][0] as i64;
    let height = area[1][1] as i64 - area[0][1] as i64;
    if !(2..=8192).contains(&width)
        || !(2..=8192).contains(&height)
        || width * height > 16_777_216
        || width % 2 != 0
        || height % 2 != 0
    {
        return Err("area must have even dimensions in 2..8192 and at most 16777216 tiles".into());
    }
    Ok([width as u32, height as u32])
}

pub fn load_map(json: &str, data: &Prototypes) -> Result<(World, MapLoadReport), String> {
    let map: MapExport = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if map.schema_version != 1 {
        return Err("unsupported map schema version".into());
    }
    if map.area.iter().flatten().any(|v| v % 32 != 0) {
        return Err("map area must align to chunk boundaries".into());
    }
    let [width, height] = dimensions(map.area)?;
    let offset = [
        map.area[0][0] + width as i32 / 2,
        map.area[0][1] + height as i32 / 2,
    ];
    let mut world = World::empty_rect(width, height, map.seed, data.tile_id("deepwater"));
    world.info.source_offset = Some(offset);
    world.spawn = [-offset[0] as f64, -offset[1] as f64];
    let mut warnings = BTreeSet::new();
    let mut seen = HashSet::new();
    let territory: Vec<[i32; 2]> = map
        .chunks
        .iter()
        .filter(|c| c.territory)
        .map(|c| c.position)
        .collect();
    for chunk in map.chunks {
        let [cx, cy] = chunk.position;
        let x0 = i64::from(cx) * 32;
        let y0 = i64::from(cy) * 32;
        if x0 < i64::from(map.area[0][0])
            || y0 < i64::from(map.area[0][1])
            || x0 + 32 > i64::from(map.area[1][0])
            || y0 + 32 > i64::from(map.area[1][1])
            || !seen.insert(chunk.position)
            || chunk.tiles.len() != 1024
        {
            return Err("duplicate, out-of-area, or incomplete chunk".into());
        }
        let x0 = x0 as i32 - offset[0];
        let y0 = y0 as i32 - offset[1];
        for (i, name) in chunk.tiles.iter().enumerate() {
            let id = tile_id(name, data, &mut warnings);
            let index = world.index(x0 + i as i32 % 32, y0 + i as i32 / 32).unwrap();
            std::sync::Arc::make_mut(&mut world.tiles)[index] = id;
        }
        for resource in chunk.resources {
            let [x, y] = resource.position;
            if x.div_euclid(32) != cx || y.div_euclid(32) != cy {
                return Err("resource position is outside its chunk".into());
            }
            let id = resource_id(&resource.name, data, &mut warnings);
            let index = world.index(x - offset[0], y - offset[1]).unwrap();
            std::sync::Arc::make_mut(&mut world.resources)[index] = id;
        }
        for mut entity in chunk.entities {
            validate_entity(&entity)?;
            if (entity.position[0] / 32.0).floor() != cx as f64
                || (entity.position[1] / 32.0).floor() != cy as f64
            {
                return Err("entity position is outside its chunk".into());
            }
            entity.position[0] -= offset[0] as f64;
            entity.position[1] -= offset[1] as f64;
            add_entity(&mut world, data, &entity, &mut warnings)?;
        }
    }
    if seen.len() < (width as usize / 32) * (height as usize / 32) {
        warnings.insert("ungenerated chunks are blocked with deepwater".into());
    }
    world.set_territory_chunks(&territory);
    let world_name = world.info.name.clone();
    Ok((
        world,
        MapLoadReport {
            name: world_name,
            offset,
            warnings: warnings.into_iter().collect(),
        },
    ))
}

pub(crate) fn tile_id(name: &str, data: &Prototypes, warnings: &mut BTreeSet<String>) -> u8 {
    if let Some(tile) = data.tiles.iter().find(|t| t.name == name) {
        return tile.id;
    }
    let fallback = if name.contains("deepwater") || name == "out-of-map" || name.contains("lava") {
        "deepwater"
    } else if name.contains("water") || name.contains("ocean") {
        "water"
    } else if name.contains("sand") {
        "sand-1"
    } else if name.contains("red-desert") {
        "red-desert-0"
    } else if name.contains("dirt") || name.contains("mud") {
        "dirt-1"
    } else {
        "grass-1"
    };
    warnings.insert(format!("unknown tile {name} -> {fallback}"));
    data.tile_id(fallback)
}

pub(crate) fn validate_entity(entity: &MapEntity) -> Result<(), String> {
    if entity.position.iter().any(|v| !v.is_finite())
        || !entity.orientation.is_finite()
        || entity.health.is_some_and(|v| !v.is_finite() || v < 0.0)
    {
        return Err("invalid entity state".into());
    }
    Ok(())
}

pub(crate) fn add_entity(
    world: &mut World,
    data: &Prototypes,
    entity: &MapEntity,
    warnings: &mut BTreeSet<String>,
) -> Result<usize, String> {
    let [x, y] = entity.position;
    let index = if entity.kind == "cliff" {
        let cliff = data.cliff(&entity.name).unwrap_or_else(|| {
            warnings.insert(format!(
                "unknown cliff {} -> {}",
                entity.name, data.cliffs.name
            ));
            &data.cliffs
        });
        let orientation = entity
            .cliff_orientation
            .as_deref()
            .unwrap_or("west-to-east");
        let orientation = if cliff.orientations.contains_key(orientation) {
            orientation
        } else {
            warnings.insert(format!(
                "unknown cliff orientation {orientation} -> west-to-east"
            ));
            "west-to-east"
        };
        if !cliff.orientations.contains_key(orientation) {
            return Err("prototype snapshot has no fallback cliff orientation".into());
        }
        world.spawn_cliff_of(cliff, orientation, x, y)
    } else {
        let prototype = data
            .entities
            .iter()
            .find(|p| p.name == entity.name)
            .or_else(|| {
                let fallback = match entity.kind.as_str() {
                    "tree" if entity.name.contains("dead") || entity.name.contains("dry") => {
                        "dry-tree"
                    }
                    "tree" => "tree-02",
                    "nest" => "biter-spawner",
                    "rock" if entity.name.contains("huge") => "huge-rock",
                    _ => "big-rock",
                };
                warnings.insert(format!(
                    "unknown entity {} ({}) -> {fallback}",
                    entity.name, entity.kind
                ));
                data.entities.iter().find(|p| p.name == fallback)
            })
            .ok_or("prototype snapshot has no entity fallback")?;
        world.add_entity(Entity {
            id: 0,
            kind: prototype.kind.clone(),
            name: entity.name.clone(),
            x,
            y,
            collision_box: prototype.collision_box,
            orientation: entity.orientation.rem_euclid(1.0),
            cliff_orientation: None,
            initial_health: entity
                .health
                .unwrap_or(prototype.max_health)
                .min(prototype.max_health),
            max_health: prototype.max_health,
            penalty_radius: (prototype.kind == "nest").then_some(data.physics.nest_penalty_radius),
            prototype: Some(data.entity_index(&prototype.name)),
        })
    };
    Ok(index)
}

pub(crate) fn resource_id(name: &str, data: &Prototypes, warnings: &mut BTreeSet<String>) -> u8 {
    if let Some(p) = data.resources.iter().find(|p| p.name == name) {
        return p.id;
    }
    let fallback = data
        .resources
        .iter()
        .find(|p| name.contains(&p.name))
        .or_else(|| data.resources.iter().find(|p| p.name == "stone"));
    warnings.insert(format!(
        "unknown resource {name} -> {}",
        fallback.map_or("none", |p| p.name.as_str())
    ));
    fallback.map_or(0, |p| p.id)
}
