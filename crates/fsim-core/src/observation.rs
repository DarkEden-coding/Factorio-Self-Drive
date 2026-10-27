use crate::{geometry::Obb, prototypes::Prototypes, target::Target, world::World};
use serde::Serialize;

pub const CHANNELS: [&str; 8] = [
    "blocked",
    "cliff",
    "tree",
    "rock",
    "water",
    "nest_zone",
    "friction",
    "target",
];
pub const LOCAL_SIZE: usize = 64;
pub const COARSE_SIZE: usize = 32;
pub const COARSE_CELL_TILES: usize = 8;

#[derive(Serialize)]
pub struct ObservationSpec {
    pub local_size: usize,
    pub coarse_size: usize,
    pub coarse_cell_tiles: usize,
    pub channels: [&'static str; 8],
}

pub fn spec() -> ObservationSpec {
    ObservationSpec {
        local_size: LOCAL_SIZE,
        coarse_size: COARSE_SIZE,
        coarse_cell_tiles: COARSE_CELL_TILES,
        channels: CHANNELS,
    }
}

pub fn local(world: &World, data: &Prototypes, x: f64, y: f64, target: Option<Target>) -> Vec<f32> {
    raster(world, data, x, y, target, LOCAL_SIZE)
}

pub fn coarse(
    world: &World,
    data: &Prototypes,
    x: f64,
    y: f64,
    target: Option<Target>,
) -> Vec<f32> {
    let size = COARSE_SIZE * COARSE_CELL_TILES;
    let fine = raster(world, data, x, y, target, size);
    let mut result = vec![0.0; CHANNELS.len() * COARSE_SIZE * COARSE_SIZE];
    for channel in 0..CHANNELS.len() {
        for cy in 0..COARSE_SIZE {
            for cx in 0..COARSE_SIZE {
                let mut sum = 0.0;
                for dy in 0..COARSE_CELL_TILES {
                    for dx in 0..COARSE_CELL_TILES {
                        sum += fine[channel * size * size
                            + (cy * COARSE_CELL_TILES + dy) * size
                            + cx * COARSE_CELL_TILES
                            + dx];
                    }
                }
                result[channel * COARSE_SIZE * COARSE_SIZE + cy * COARSE_SIZE + cx] =
                    sum / (COARSE_CELL_TILES * COARSE_CELL_TILES) as f32;
            }
        }
    }
    result
}

fn raster(
    world: &World,
    data: &Prototypes,
    x: f64,
    y: f64,
    target: Option<Target>,
    size: usize,
) -> Vec<f32> {
    let plane = size * size;
    let mut result = vec![0.0; CHANNELS.len() * plane];
    let x0 = x - size as f64 * 0.5;
    let y0 = y - size as f64 * 0.5;
    for row in 0..size {
        for col in 0..size {
            let px = x0 + col as f64 + 0.5;
            let py = y0 + row as f64 + 0.5;
            let cell = row * size + col;
            let tile = world
                .tile(px.floor() as i32, py.floor() as i32)
                .map(|id| &data.tiles[id as usize]);
            if tile.is_none_or(|t| t.is_water) {
                result[cell] = 1.0;
                result[4 * plane + cell] = 1.0;
            }
            result[6 * plane + cell] = tile.map_or(1.0, |t| t.vehicle_friction_modifier as f32);
            if let Some(t) = target {
                if (px - t.x).hypot(py - t.y) <= t.radius {
                    result[7 * plane + cell] = 1.0;
                }
            }
        }
    }
    let padding = data.physics.nest_penalty_radius;
    let bounds = [
        [x0 - padding, y0 - padding],
        [x0 + size as f64 + padding, y0 + size as f64 + padding],
    ];
    for index in world.candidates(bounds) {
        let e = &world.entities[index];
        let shape = e.shape();
        let b = shape.aabb();
        let [lo, hi] = raster_bounds(b, x0, y0, size);
        let channel = match e.kind.as_str() {
            "cliff" => 1,
            "tree" => 2,
            "rock" => 3,
            _ => 0,
        };
        for row in lo[1]..hi[1] {
            for col in lo[0]..hi[0] {
                let cell_shape = Obb::new(
                    x0 + col as f64,
                    y0 + row as f64,
                    [[0.0, 0.0], [1.0, 1.0]],
                    0.0,
                );
                if cell_shape.intersects(shape) {
                    let cell = row * size + col;
                    result[cell] = 1.0;
                    result[channel * plane + cell] = 1.0;
                }
            }
        }
        if let Some(radius) = e.penalty_radius {
            let [lo, hi] = raster_bounds(
                [[e.x - radius, e.y - radius], [e.x + radius, e.y + radius]],
                x0,
                y0,
                size,
            );
            for row in lo[1]..hi[1] {
                for col in lo[0]..hi[0] {
                    let distance = (x0 + col as f64 + 0.5 - e.x).hypot(y0 + row as f64 + 0.5 - e.y);
                    if distance <= radius {
                        result[5 * plane + row * size + col] = 1.0;
                    }
                }
            }
        }
    }
    result
}

fn raster_bounds(b: [[f64; 2]; 2], x0: f64, y0: f64, size: usize) -> [[usize; 2]; 2] {
    [
        [
            (b[0][0] - x0).floor().clamp(0.0, size as f64) as usize,
            (b[0][1] - y0).floor().clamp(0.0, size as f64) as usize,
        ],
        [
            (b[1][0] - x0).ceil().clamp(0.0, size as f64) as usize,
            (b[1][1] - y0).ceil().clamp(0.0, size as f64) as usize,
        ],
    ]
}
