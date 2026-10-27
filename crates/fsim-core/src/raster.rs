//! Per-map static raster of the observation semantics, built once per loaded
//! map and shared by every env on that map. Each world keeps its own small
//! patch set for entities it destroyed, so shared rasters never change.
use crate::{geometry::Obb, prototypes::Prototypes, world::World};
use std::collections::HashMap;

/// Bits of `MapRaster::mask`. Channel `c` of the policy grids is bit `c` for
/// the six binary channels. `NEST` marks nest collision boxes (part of
/// `BLOCKED`) for the distance field; it is not a policy channel.
pub const BLOCKED: u8 = 1;
pub const CLIFF: u8 = 2;
pub const TREE: u8 = 4;
pub const ROCK: u8 = 8;
pub const WATER: u8 = 16;
pub const NEST_ZONE: u8 = 32;
pub const NEST: u8 = 64;
/// Off-map cells are blocked water with friction byte 0.
pub const OFF_MAP: u8 = BLOCKED | WATER;

pub struct MapRaster {
    pub x_min: i32,
    pub y_min: i32,
    pub width: usize,
    pub height: usize,
    /// Row-major semantic bits per tile.
    pub mask: Vec<u8>,
    /// `round(clamp(vehicle_friction_modifier - 1, 0, 1) * 255)` per tile.
    pub friction: Vec<u8>,
    /// Entity indices of nests, for the nest-zone reward.
    pub nests: Vec<usize>,
}

pub fn friction_byte(modifier: f64) -> u8 {
    ((modifier - 1.0).clamp(0.0, 1.0) * 255.0).round() as u8
}

fn kind_bits(kind: &str) -> u8 {
    BLOCKED
        | match kind {
            "cliff" => CLIFF,
            "tree" => TREE,
            "rock" => ROCK,
            "nest" => NEST,
            _ => 0,
        }
}

fn unit_cell(x: i32, y: i32) -> Obb {
    Obb::new(x as f64, y as f64, [[0.0, 0.0], [1.0, 1.0]], 0.0)
}

/// Tile ranges `[lo, hi)` covering an axis-aligned box.
fn tile_range(b: [[f64; 2]; 2]) -> [[i32; 2]; 2] {
    [
        [b[0][0].floor() as i32, b[0][1].floor() as i32],
        [b[1][0].ceil() as i32, b[1][1].ceil() as i32],
    ]
}

impl MapRaster {
    /// Rasterizes the map as loaded (entity health from the map file, not
    /// live damage; worlds track their own destruction in `RasterPatches`).
    /// Uses the same cell tests as `observation::local`: a tile carries an
    /// entity bit when the entity's oriented box intersects the tile square,
    /// and a nest-zone bit when the tile center is within a nest's penalty radius.
    pub fn build(world: &World, data: &Prototypes) -> Self {
        let info = &world.info;
        let (width, height) = (info.width as usize, info.height as usize);
        let mut mask = Vec::with_capacity(width * height);
        let mut friction = Vec::with_capacity(width * height);
        for id in world.tiles.iter() {
            let tile = &data.tiles[*id as usize];
            mask.push(if tile.is_water { BLOCKED | WATER } else { 0 });
            friction.push(friction_byte(tile.vehicle_friction_modifier));
        }
        let mut raster = Self {
            x_min: info.x_min,
            y_min: info.y_min,
            width,
            height,
            mask,
            friction,
            nests: Vec::new(),
        };
        for (index, e) in world.entities.iter().enumerate() {
            if e.kind == "nest" {
                raster.nests.push(index);
            }
            if e.kind != "cliff" && e.initial_health <= 0.0 {
                continue;
            }
            let shape = e.shape();
            let bits = kind_bits(&e.kind);
            let [lo, hi] = tile_range(shape.aabb());
            for y in lo[1]..hi[1] {
                for x in lo[0]..hi[0] {
                    if let Some(i) = raster.index(x, y) {
                        if unit_cell(x, y).intersects(shape) {
                            raster.mask[i] |= bits;
                        }
                    }
                }
            }
            if let Some(radius) = e.penalty_radius {
                let [lo, hi] =
                    tile_range([[e.x - radius, e.y - radius], [e.x + radius, e.y + radius]]);
                for y in lo[1]..hi[1] {
                    for x in lo[0]..hi[0] {
                        if let Some(i) = raster.index(x, y) {
                            if (x as f64 + 0.5 - e.x).hypot(y as f64 + 0.5 - e.y) <= radius {
                                raster.mask[i] |= NEST_ZONE;
                            }
                        }
                    }
                }
            }
        }
        raster
    }

    pub fn index(&self, x: i32, y: i32) -> Option<usize> {
        let ix = x - self.x_min;
        let iy = y - self.y_min;
        (ix >= 0 && iy >= 0 && (ix as usize) < self.width && (iy as usize) < self.height)
            .then(|| iy as usize * self.width + ix as usize)
    }

    pub fn mask_at(&self, x: i32, y: i32) -> u8 {
        self.index(x, y).map_or(OFF_MAP, |i| self.mask[i])
    }

    pub fn friction_at(&self, x: i32, y: i32) -> u8 {
        self.index(x, y).map_or(0, |i| self.friction[i])
    }

    pub fn memory_bytes(&self) -> usize {
        self.mask.len() + self.friction.len() + self.nests.len() * 8
    }
}

/// Replacement masks for tiles touched by entities this world destroyed.
#[derive(Clone, Debug, Default)]
pub struct RasterPatches {
    /// How many of `world.destroyed()` have been applied.
    applied: usize,
    /// Raster tile index -> mask.
    pub tiles: HashMap<usize, u8>,
}

impl RasterPatches {
    pub fn clear(&mut self) {
        self.applied = 0;
        self.tiles.clear();
    }

    /// Applies entities destroyed since the last call. Call after every tick
    /// batch, before reading observations.
    pub fn sync(&mut self, world: &World, raster: &MapRaster, data: &Prototypes) {
        let destroyed = world.destroyed();
        if destroyed.len() < self.applied {
            // The world was restored; patches from earlier damage are stale.
            self.clear();
        }
        for &index in &destroyed[self.applied..] {
            let e = &world.entities[index];
            let mut bounds = e.shape().aabb();
            if let Some(radius) = e.penalty_radius {
                bounds = [[e.x - radius, e.y - radius], [e.x + radius, e.y + radius]];
            }
            let [lo, hi] = tile_range(bounds);
            for y in lo[1]..hi[1] {
                for x in lo[0]..hi[0] {
                    if let Some(i) = raster.index(x, y) {
                        self.tiles.insert(i, tile_mask(world, raster, data, x, y));
                    }
                }
            }
        }
        self.applied = destroyed.len();
    }
}

/// Recomputes one tile's mask from the world's live entities.
fn tile_mask(world: &World, raster: &MapRaster, data: &Prototypes, x: i32, y: i32) -> u8 {
    let mut mask = raster.mask_at(x, y) & WATER;
    if mask != 0 {
        mask |= BLOCKED;
    }
    let cell = unit_cell(x, y);
    for index in world.candidates(cell.aabb()) {
        let e = &world.entities[index];
        if cell.intersects(e.shape()) {
            mask |= kind_bits(&e.kind);
        }
    }
    let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
    let reach = data.physics.nest_penalty_radius;
    for index in world.candidates([[cx - reach, cy - reach], [cx + reach, cy + reach]]) {
        let e = &world.entities[index];
        if let Some(radius) = e.penalty_radius {
            if (cx - e.x).hypot(cy - e.y) <= radius {
                mask |= NEST_ZONE;
            }
        }
    }
    mask
}
