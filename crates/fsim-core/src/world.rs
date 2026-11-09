use crate::{
    geometry::Obb,
    prototypes::{CollisionBox, Prototypes},
};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};

const CELL_TILES: f64 = 8.0;

#[derive(Clone, Debug, Serialize)]
pub struct Entity {
    pub id: u32,
    pub kind: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub collision_box: CollisionBox,
    pub orientation: f64,
    pub cliff_orientation: Option<String>,
    /// Health when the map was loaded. Live health is `World::health`, so
    /// worlds sharing this entity list keep separate damage.
    #[serde(skip)]
    pub initial_health: f64,
    pub max_health: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penalty_radius: Option<f64>,
    #[serde(skip)]
    pub prototype: Option<usize>,
}

/// An entity with its live health, as serialized for the app.
#[derive(Serialize)]
pub struct EntityState<'a> {
    #[serde(flatten)]
    pub entity: &'a Entity,
    pub health: f64,
}

impl Entity {
    pub fn shape(&self) -> Obb {
        Obb::new(self.x, self.y, self.collision_box, self.orientation)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MapInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x_min: i32,
    pub y_min: i32,
    pub seed: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_offset: Option<[i32; 2]>,
}

/// Map data and live obstacle state. Tiles, entities and the spatial hash are
/// shared copy-on-write, so cloning a loaded world copies only entity health.
#[derive(Clone, Debug)]
pub struct World {
    pub info: MapInfo,
    /// Preferred spawn in simulator coordinates, translated from Factorio origin.
    pub spawn: [f64; 2],
    pub tiles: Arc<Vec<u8>>,
    pub resources: Arc<Vec<u8>>,
    pub entities: Arc<Vec<Entity>>,
    health: Vec<f64>,
    /// Entity indices destroyed since load or `restore_entities`, in order.
    destroyed: Vec<usize>,
    /// Bumped when the entity set or tiles change.
    pub revision: u32,
    /// Bumped only when tiles change.
    pub tile_revision: u32,
    spatial: Arc<HashMap<(i32, i32), Vec<usize>>>,
    /// Row-major per-tile flag for zone tiles that aren't nest radii: Vulcanus
    /// demolisher territory. Empty when the map has none.
    pub territory: Arc<Vec<bool>>,
}

impl World {
    pub fn empty(chunks: u32, seed: u32, land: u8) -> Self {
        Self::empty_rect(chunks * 32, chunks * 32, seed, land)
    }

    pub fn empty_rect(width: u32, height: u32, seed: u32, land: u8) -> Self {
        Self {
            spawn: [0.0, 0.0],
            info: MapInfo {
                name: "unnamed".into(),
                width,
                height,
                x_min: -(width as i32 / 2),
                y_min: -(height as i32 / 2),
                seed,
                source_offset: None,
            },
            tiles: Arc::new(vec![land; width as usize * height as usize]),
            resources: Arc::new(vec![0; width as usize * height as usize]),
            entities: Arc::default(),
            health: Vec::new(),
            destroyed: Vec::new(),
            revision: 1,
            tile_revision: 1,
            spatial: Arc::default(),
            territory: Arc::default(),
        }
    }

    pub fn index(&self, x: i32, y: i32) -> Option<usize> {
        let ix = x - self.info.x_min;
        let iy = y - self.info.y_min;
        (ix >= 0 && iy >= 0 && ix < self.info.width as i32 && iy < self.info.height as i32)
            .then_some((iy as i64 * self.info.width as i64 + ix as i64) as usize)
    }

    pub fn tile(&self, x: i32, y: i32) -> Option<u8> {
        self.index(x, y).map(|i| self.tiles[i])
    }

    /// Whether tile `(x, y)` is demolisher territory.
    pub fn in_territory(&self, x: i32, y: i32) -> bool {
        !self.territory.is_empty() && self.index(x, y).is_some_and(|i| self.territory[i])
    }

    /// Marks the 32x32 source chunks in `chunks` (Factorio chunk positions) as
    /// territory. Needs `info.source_offset`.
    pub fn set_territory_chunks(&mut self, chunks: &[[i32; 2]]) {
        let offset = self.info.source_offset.unwrap_or([0, 0]);
        let mut flags = vec![false; self.tiles.len()];
        for &[cx, cy] in chunks {
            for dy in 0..32 {
                for dx in 0..32 {
                    let (x, y) = (cx * 32 + dx - offset[0], cy * 32 + dy - offset[1]);
                    if let Some(i) = self.index(x, y) {
                        flags[i] = true;
                    }
                }
            }
        }
        self.territory = Arc::new(if flags.contains(&true) {
            flags
        } else {
            Vec::new()
        });
    }

    pub fn water(&self, x: i32, y: i32, data: &Prototypes) -> bool {
        self.tile(x, y)
            .is_none_or(|id| data.tiles[id as usize].is_water)
    }

    pub fn add_entity(&mut self, mut entity: Entity) -> usize {
        let index = self.entities.len();
        entity.id = index as u32 + 1;
        let cells = cell_bounds(entity.shape().aabb());
        let spatial = Arc::make_mut(&mut self.spatial);
        for cy in cells[0][1]..=cells[1][1] {
            for cx in cells[0][0]..=cells[1][0] {
                spatial.entry((cx, cy)).or_default().push(index);
            }
        }
        self.health.push(entity.initial_health);
        Arc::make_mut(&mut self.entities).push(entity);
        index
    }

    pub fn health(&self, index: usize) -> f64 {
        self.health[index]
    }

    pub fn set_health(&mut self, index: usize, health: f64) {
        let was_alive = self.alive(index);
        self.health[index] = health;
        if was_alive && !self.alive(index) {
            self.destroyed.push(index);
        }
    }

    pub fn alive(&self, index: usize) -> bool {
        self.entities[index].kind == "cliff" || self.health[index] > 0.0
    }

    /// Entities destroyed since the map was loaded or restored, in destruction order.
    pub fn destroyed(&self) -> &[usize] {
        &self.destroyed
    }

    /// Restores every entity to its loaded health, undoing all damage.
    pub fn restore_entities(&mut self) {
        for (health, entity) in self.health.iter_mut().zip(self.entities.iter()) {
            *health = entity.initial_health;
        }
        if !self.destroyed.is_empty() {
            self.destroyed.clear();
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Makes this world's live state equal `template`'s, assuming both come from
    /// the same loaded map. Reuses allocations; shares the template's map data.
    pub fn restore_from(&mut self, template: &World) {
        self.info.clone_from(&template.info);
        self.spawn = template.spawn;
        self.tiles = template.tiles.clone();
        self.resources = template.resources.clone();
        self.entities = template.entities.clone();
        self.spatial = template.spatial.clone();
        self.territory = template.territory.clone();
        self.health.clone_from(&template.health);
        self.destroyed.clone_from(&template.destroyed);
        self.revision = template.revision;
        self.tile_revision = template.tile_revision;
    }

    pub fn spawn_entity(&mut self, data: &Prototypes, name: &str, x: f64, y: f64) -> usize {
        let index = data.entity_index(name);
        let p = &data.entities[index];
        self.add_entity(Entity {
            id: 0,
            kind: p.kind.clone(),
            name: p.name.clone(),
            x,
            y,
            collision_box: p.collision_box,
            orientation: 0.0,
            cliff_orientation: None,
            initial_health: p.max_health,
            max_health: p.max_health,
            penalty_radius: (p.kind == "nest").then_some(data.physics.nest_penalty_radius),
            prototype: Some(index),
        })
    }

    pub fn spawn_cliff(&mut self, data: &Prototypes, name: &str, x: f64, y: f64) -> usize {
        self.spawn_cliff_of(&data.cliffs, name, x, y)
    }

    /// A cliff piece of prototype `cliff` with orientation `name`.
    pub fn spawn_cliff_of(
        &mut self,
        cliff: &crate::prototypes::CliffPrototype,
        name: &str,
        x: f64,
        y: f64,
    ) -> usize {
        let p = &cliff.orientations[name];
        self.add_entity(Entity {
            id: 0,
            kind: "cliff".into(),
            name: cliff.name.clone(),
            x,
            y,
            collision_box: p.collision_box,
            orientation: p.orientation,
            cliff_orientation: Some(name.into()),
            initial_health: 0.0,
            max_health: 0.0,
            penalty_radius: None,
            prototype: None,
        })
    }

    pub fn candidates(&self, bounds: CollisionBox) -> Vec<usize> {
        let cells = cell_bounds(bounds);
        let mut result = Vec::new();
        for cy in cells[0][1]..=cells[1][1] {
            for cx in cells[0][0]..=cells[1][0] {
                if let Some(indices) = self.spatial.get(&(cx, cy)) {
                    result.extend(indices.iter().copied().filter(|i| self.alive(*i)));
                }
            }
        }
        // No HashMap iteration: stable id order also resolves simultaneous contacts.
        result.sort_unstable();
        result.dedup();
        result
    }

    pub fn entities_in_rect(&self, bounds: CollisionBox) -> Vec<EntityState<'_>> {
        let rect = Obb::new(0.0, 0.0, bounds, 0.0);
        self.candidates(bounds)
            .into_iter()
            .filter(|i| self.entities[*i].shape().intersects(rect))
            .map(|i| EntityState {
                entity: &self.entities[i],
                health: self.health[i],
            })
            .collect()
    }

    pub fn blocked(&self, shape: Obb, data: &Prototypes) -> bool {
        if self.touches_water(shape, data) {
            return true;
        }
        self.candidates(shape.aabb())
            .iter()
            .any(|i| shape.intersects(self.entities[*i].shape()))
    }

    pub fn touches_water(&self, shape: Obb, data: &Prototypes) -> bool {
        let b = shape.aabb();
        for y in b[0][1].floor() as i32..b[1][1].ceil() as i32 {
            for x in b[0][0].floor() as i32..b[1][0].ceil() as i32 {
                if self.water(x, y, data)
                    && shape.intersects(Obb::new(x as f64, y as f64, [[0.0, 0.0], [1.0, 1.0]], 0.0))
                {
                    return true;
                }
            }
        }
        false
    }

    pub fn set_tile(&mut self, x: i32, y: i32, id: u8) {
        if let Some(i) = self.index(x, y) {
            if self.tiles[i] != id {
                Arc::make_mut(&mut self.tiles)[i] = id;
                self.revision = self.revision.wrapping_add(1);
                self.tile_revision = self.tile_revision.wrapping_add(1);
            }
        }
    }
}

fn cell_bounds(b: CollisionBox) -> [[i32; 2]; 2] {
    b.map(|p| p.map(|v| (v / CELL_TILES).floor() as i32))
}
