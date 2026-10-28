//! Policy observations (docs/rl.md, "Observation"): u8 grids cropped from the
//! per-map raster, snapped to the vehicle's tile, plus a named f32 vector.
use crate::{
    observation::CHANNELS,
    physics,
    prototypes::Prototypes,
    raster::{MapRaster, RasterPatches, OFF_MAP},
    rl_episode::{Episode, ACTIONS},
    vehicle::Vehicles,
    world::World,
};
use serde::Serialize;
use std::f64::consts::TAU;

pub const LOCAL_SIZE: usize = 64;
pub const COARSE_SIZE: usize = 32;
pub const COARSE_CELL_TILES: usize = 8;
const COARSE_TILES: usize = COARSE_SIZE * COARSE_CELL_TILES;
pub const LOCAL_LEN: usize = CHANNELS.len() * LOCAL_SIZE * LOCAL_SIZE;
pub const COARSE_LEN: usize = CHANNELS.len() * COARSE_SIZE * COARSE_SIZE;
const FRICTION: usize = 6;
const TARGET: usize = 7;

pub const VECTOR_FEATURES: [&str; 39] = [
    "speed",
    "heading_sin",
    "heading_cos",
    "target_right",
    "target_forward",
    "target_dir_right",
    "target_dir_forward",
    "straight_distance",
    "path_distance",
    "path_dir_right",
    "path_dir_forward",
    "target_radius",
    "health",
    "subtile_x",
    "subtile_y",
    "vehicle_tank",
    "vehicle_weight",
    "vehicle_max_health",
    "vehicle_rotation_speed",
    "vehicle_braking_power",
    "vehicle_power_to_weight",
    "vehicle_top_speed",
    "vehicle_half_width",
    "vehicle_half_length",
    "fuel_acceleration",
    "fuel_top_speed",
    "tile_friction",
    "prev_action_0",
    "prev_action_1",
    "prev_action_2",
    "prev_action_3",
    "prev_action_4",
    "prev_action_5",
    "prev_action_6",
    "prev_action_7",
    "prev_action_8",
    "time_fraction",
    "stall_fraction",
    "in_nest_zone",
];
pub const VECTOR_LEN: usize = VECTOR_FEATURES.len();
/// Tiles per unit for distances and offsets in the vector.
const DISTANCE_SCALE: f64 = 64.0;
/// Field-descent steps used for `path_dir_*`.
const PATH_LOOKAHEAD: usize = 4;

/// Output slices for one env. Empty slices are skipped, so an env can write
/// the teacher's observation, the student's, or both.
pub struct ObsMut<'a> {
    pub local: &'a mut [u8],
    pub coarse: &'a mut [u8],
    pub vector: &'a mut [f32],
    pub student: &'a mut [f32],
}

/// Reusable crop buffers.
#[derive(Default)]
pub struct ObsScratch {
    mask: Vec<u8>,
    friction: Vec<u8>,
}

#[derive(Serialize)]
struct ActionSpec {
    count: usize,
    encoding: &'static str,
    throttle: [i8; 3],
    steer: [i8; 3],
}

#[derive(Serialize)]
struct HashedSpec {
    local_shape: [usize; 3],
    coarse_shape: [usize; 3],
    coarse_cell_tiles: usize,
    channels: [&'static str; 8],
    vector_features: &'static [&'static str],
    actions: ActionSpec,
}

#[derive(Serialize)]
pub struct PolicySpec {
    #[serde(flatten)]
    hashed: HashedSpec,
    pub vector_size: usize,
    pub reward_terms: [&'static str; crate::reward::TERMS.len()],
    pub modes: [&'static str; 3],
    /// Scenario kinds; `info["scenario"]` indexes this list.
    pub scenarios: [&'static str; 6],
    /// `info["collisions_<kind>"]` kinds.
    pub collision_kinds: [&'static str; 5],
    /// Vehicle prototype names; `info["vehicle_index"]` indexes this list.
    pub vehicles: Vec<String>,
    /// Fuel names; `info["fuel_index"]` indexes this list.
    pub fuels: Vec<String>,
    pub spec_hash: String,
    /// The compact student observation, with its own hash.
    pub student: crate::student_obs::StudentSpec,
}

/// FNV-1a 64 of a string.
pub fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

/// Shapes, names and action layout. `spec_hash` is FNV-1a 64 of the JSON of
/// everything the policy network depends on (not the vehicle and fuel lists).
pub fn spec(data: &Prototypes) -> PolicySpec {
    let hashed = HashedSpec {
        local_shape: [CHANNELS.len(), LOCAL_SIZE, LOCAL_SIZE],
        coarse_shape: [CHANNELS.len(), COARSE_SIZE, COARSE_SIZE],
        coarse_cell_tiles: COARSE_CELL_TILES,
        channels: CHANNELS,
        vector_features: &VECTOR_FEATURES,
        actions: ActionSpec {
            count: ACTIONS,
            encoding: "(throttle + 1) * 3 + (steer + 1)",
            throttle: [-1, 0, 1],
            steer: [-1, 0, 1],
        },
    };
    let hash = fnv1a(&serde_json::to_string(&hashed).expect("static spec"));
    PolicySpec {
        hashed,
        vector_size: VECTOR_LEN,
        reward_terms: crate::reward::TERMS,
        modes: ["reactive", "direct_plan", "path_planner"],
        scenarios: crate::rl_config::SCENARIOS,
        collision_kinds: crate::rl_episode::COLLISION_KINDS,
        vehicles: data.vehicles.iter().map(|v| v.name.clone()).collect(),
        fuels: data.fuels.iter().map(|f| f.name.clone()).collect(),
        spec_hash: format!("{hash:016x}"),
        student: crate::student_obs::spec(),
    }
}

/// Effective masks and friction for tiles `[x0, x0+size) x [y0, y0+size)`.
fn window(
    raster: &MapRaster,
    patches: &RasterPatches,
    x0: i32,
    y0: i32,
    size: usize,
    scratch: &mut ObsScratch,
) {
    scratch.mask.clear();
    scratch.mask.resize(size * size, OFF_MAP);
    scratch.friction.clear();
    scratch.friction.resize(size * size, 0);
    let col_lo = (raster.x_min - x0).clamp(0, size as i32) as usize;
    let col_hi = (raster.x_min + raster.width as i32 - x0).clamp(0, size as i32) as usize;
    for row in 0..size {
        let y = y0 + row as i32;
        if col_lo >= col_hi || y < raster.y_min || y >= raster.y_min + raster.height as i32 {
            continue;
        }
        let start = raster.index(x0 + col_lo as i32, y).expect("clipped to map");
        let len = col_hi - col_lo;
        scratch.mask[row * size + col_lo..row * size + col_hi]
            .copy_from_slice(&raster.mask[start..start + len]);
        scratch.friction[row * size + col_lo..row * size + col_hi]
            .copy_from_slice(&raster.friction[start..start + len]);
    }
    for (&index, &mask) in &patches.tiles {
        let x = raster.x_min + (index % raster.width) as i32 - x0;
        let y = raster.y_min + (index / raster.width) as i32 - y0;
        if x >= 0 && y >= 0 && (x as usize) < size && (y as usize) < size {
            scratch.mask[y as usize * size + x as usize] = mask;
        }
    }
}

/// Calls `f(row, col)` for each cell of the window whose center is within the target circle.
fn target_cells(
    x0: i32,
    y0: i32,
    size: usize,
    target: [f64; 2],
    radius: f64,
    mut f: impl FnMut(usize, usize),
) {
    let lo_x = ((target[0] - radius).floor() as i32 - x0).max(0);
    let hi_x = ((target[0] + radius).ceil() as i32 - x0 + 1).min(size as i32);
    let lo_y = ((target[1] - radius).floor() as i32 - y0).max(0);
    let hi_y = ((target[1] + radius).ceil() as i32 - y0 + 1).min(size as i32);
    for row in lo_y..hi_y {
        for col in lo_x..hi_x {
            let px = (x0 + col) as f64 + 0.5;
            let py = (y0 + row) as f64 + 0.5;
            if (px - target[0]).hypot(py - target[1]) <= radius {
                f(row as usize, col as usize);
            }
        }
    }
}

/// `u8 [8, 64, 64]` centered on tile `(tx, ty)`: cell `(r, c)` is tile `(tx-32+c, ty-32+r)`.
pub fn write_local(
    raster: &MapRaster,
    patches: &RasterPatches,
    tile: [i32; 2],
    target: Option<([f64; 2], f64)>,
    scratch: &mut ObsScratch,
    out: &mut [u8],
) {
    let half = LOCAL_SIZE as i32 / 2;
    let (x0, y0) = (tile[0] - half, tile[1] - half);
    window(raster, patches, x0, y0, LOCAL_SIZE, scratch);
    let plane = LOCAL_SIZE * LOCAL_SIZE;
    out.fill(0);
    for (cell, &mask) in scratch.mask.iter().enumerate() {
        for channel in 0..FRICTION {
            if mask & (1 << channel) != 0 {
                out[channel * plane + cell] = 255;
            }
        }
    }
    out[FRICTION * plane..(FRICTION + 1) * plane].copy_from_slice(&scratch.friction);
    if let Some((point, radius)) = target {
        target_cells(x0, y0, LOCAL_SIZE, point, radius, |row, col| {
            out[TARGET * plane + row * LOCAL_SIZE + col] = 255;
        });
    }
}

/// Spreads the six binary mask bits into separate bytes of a u64, so 64
/// cells can be summed with one add each without overflowing a byte.
const SPREAD: [u64; 256] = {
    let mut table = [0u64; 256];
    let mut m = 0;
    while m < 256 {
        let mut b = 0;
        while b < FRICTION {
            if m & (1 << b) != 0 {
                table[m] |= 1 << (8 * b);
            }
            b += 1;
        }
        m += 1;
    }
    table
};

fn mean_byte(count: u64) -> u8 {
    ((count * 255 + 32) / 64) as u8
}

/// `u8 [8, 32, 32]`, each cell the rounded mean of 8x8 local-resolution cells,
/// covering tiles `[tx-128, tx+128) x [ty-128, ty+128)`.
pub fn write_coarse(
    raster: &MapRaster,
    patches: &RasterPatches,
    tile: [i32; 2],
    target: Option<([f64; 2], f64)>,
    scratch: &mut ObsScratch,
    out: &mut [u8],
) {
    let half = COARSE_TILES as i32 / 2;
    let (x0, y0) = (tile[0] - half, tile[1] - half);
    window(raster, patches, x0, y0, COARSE_TILES, scratch);
    let plane = COARSE_SIZE * COARSE_SIZE;
    let n = COARSE_CELL_TILES;
    for cy in 0..COARSE_SIZE {
        let mut counts = [0u64; COARSE_SIZE];
        let mut friction = [0u32; COARSE_SIZE];
        for dy in 0..n {
            let row = (cy * n + dy) * COARSE_TILES;
            let masks = &scratch.mask[row..row + COARSE_TILES];
            let frictions = &scratch.friction[row..row + COARSE_TILES];
            for cx in 0..COARSE_SIZE {
                for dx in 0..n {
                    counts[cx] += SPREAD[masks[cx * n + dx] as usize];
                    friction[cx] += frictions[cx * n + dx] as u32;
                }
            }
        }
        for cx in 0..COARSE_SIZE {
            let cell = cy * COARSE_SIZE + cx;
            for channel in 0..FRICTION {
                out[channel * plane + cell] = mean_byte((counts[cx] >> (8 * channel)) & 0xff);
            }
            out[FRICTION * plane + cell] = ((friction[cx] + 32) / 64) as u8;
        }
    }
    let target_plane = &mut out[TARGET * plane..];
    let mut counts = [0u8; COARSE_SIZE * COARSE_SIZE];
    if let Some((point, radius)) = target {
        target_cells(x0, y0, COARSE_TILES, point, radius, |row, col| {
            counts[(row / n) * COARSE_SIZE + col / n] += 1;
        });
    }
    for (value, count) in target_plane[..plane].iter_mut().zip(counts) {
        *value = mean_byte(count as u64);
    }
}

/// Read-only state an observation is computed from.
#[derive(Clone, Copy)]
pub struct ObsSource<'a> {
    pub data: &'a Prototypes,
    pub world: &'a World,
    pub raster: &'a MapRaster,
    pub patches: &'a RasterPatches,
    pub vehicles: &'a Vehicles,
    pub episode: &'a Episode,
    /// The student's route from the vehicle when it doesn't come from the
    /// episode field (`RouteSource::Pathfinder`).
    pub route: Option<&'a [[f64; 2]]>,
}

/// Vector features in `VECTOR_FEATURES` order.
pub fn write_vector(src: ObsSource<'_>, out: &mut [f32]) {
    let ObsSource {
        data,
        world,
        raster,
        vehicles,
        episode,
        ..
    } = src;
    let p = &data.vehicles[vehicles.prototype[0]];
    let fuel = &data.fuels[vehicles.fuel[0]];
    let (x, y) = (vehicles.x[0], vehicles.y[0]);
    let (sin, cos) = (vehicles.orientation[0] * TAU).sin_cos();
    // Vehicle frame: right = (cos, sin), forward = (sin, -cos) in world x/y.
    let to_vehicle = |dx: f64, dy: f64| [dx * cos + dy * sin, dx * sin - dy * cos];
    let target = episode.setup.target;
    let offset = to_vehicle(target.x - x, target.y - y);
    let straight = (target.x - x).hypot(target.y - y);
    let unit = |v: [f64; 2]| {
        let n = v[0].hypot(v[1]);
        if n > 1e-9 {
            [v[0] / n, v[1] / n]
        } else {
            [0.0, 0.0]
        }
    };
    let target_dir = unit(offset);
    let path_dir = episode
        .field
        .descend(x, y, PATH_LOOKAHEAD)
        .last()
        .map_or([0.0, 0.0], |p| unit(to_vehicle(p[0] - x, p[1] - y)));
    let grass = data.tile_id("grass-1");
    let top_speed =
        physics::terminal_speed(data, vehicles.prototype[0], vehicles.fuel[0], grass, false);
    let power = p.consumption
        * p.effectivity
        * fuel.fuel_acceleration_multiplier
        * fuel.fuel_top_speed_multiplier.powi(2);
    let tile_friction = world
        .tile(x.floor() as i32, y.floor() as i32)
        .map_or(1.0, |id| data.tiles[id as usize].vehicle_friction_modifier);
    let b = p.collision_box;
    let mut prev = [0.0; ACTIONS];
    if let Some(a) = episode.prev_action() {
        prev[a as usize] = 1.0;
    }
    let kmh = |speed: f64| speed * 216.0 / 100.0;
    let distance = |d: f64| (d / DISTANCE_SCALE).clamp(0.0, 8.0);
    let values: [f64; VECTOR_LEN] = [
        kmh(vehicles.speed[0]),
        sin,
        cos,
        (offset[0] / DISTANCE_SCALE).clamp(-4.0, 4.0),
        (offset[1] / DISTANCE_SCALE).clamp(-4.0, 4.0),
        target_dir[0],
        target_dir[1],
        distance(straight),
        distance(episode.path_distance),
        path_dir[0],
        path_dir[1],
        episode.setup.radius / 8.0,
        vehicles.health[0] / p.max_health,
        x - x.floor(),
        y - y.floor(),
        if p.tank_driving { 1.0 } else { 0.0 },
        p.weight / 20000.0,
        p.max_health / 2000.0,
        p.rotation_speed / 0.015,
        p.braking_power / 1e6,
        power / p.weight / 100.0,
        kmh(top_speed),
        b[0][0].abs().max(b[1][0].abs()),
        b[0][1].abs().max(b[1][1].abs()),
        fuel.fuel_acceleration_multiplier,
        fuel.fuel_top_speed_multiplier,
        tile_friction - 1.0,
        prev[0],
        prev[1],
        prev[2],
        prev[3],
        prev[4],
        prev[5],
        prev[6],
        prev[7],
        prev[8],
        episode.ticks as f64 / episode.time_limit_ticks as f64,
        episode.stall_ticks() as f64 / episode.rules.time_limit.stall_ticks as f64,
        if crate::rl_episode::in_nest_zone(world, raster, x, y) {
            1.0
        } else {
            0.0
        },
    ];
    for (o, v) in out.iter_mut().zip(values) {
        *o = v as f32;
    }
}

/// Writes the requested (non-empty) policy observations for the episode's current state.
pub fn write_obs(src: ObsSource<'_>, scratch: &mut ObsScratch, out: ObsMut<'_>) {
    let v = src.vehicles;
    let tile = [v.x[0].floor() as i32, v.y[0].floor() as i32];
    let setup = &src.episode.setup;
    let target = Some(([setup.target.x, setup.target.y], setup.radius));
    if !out.local.is_empty() {
        write_local(src.raster, src.patches, tile, target, scratch, out.local);
        write_coarse(src.raster, src.patches, tile, target, scratch, out.coarse);
        write_vector(src, out.vector);
    }
    if !out.student.is_empty() {
        crate::student_obs::write_student(src, out.student);
    }
}
