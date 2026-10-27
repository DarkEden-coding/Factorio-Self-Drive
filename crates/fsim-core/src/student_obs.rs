//! Compact `student` observation (docs/rl.md, "Student observation"): ray
//! casts over the tile raster, route guidance along the field's descent path
//! and vehicle state, all in the vehicle frame. Small enough to compute in a
//! Factorio mod in Lua from a cached obstacle grid around the vehicle.
//!
//! With `student_map`, a coarse vehicle-frame map follows the features: a
//! route-free alternative to the route guidance for planning past the rays.
use crate::{
    baseline,
    distance_field::polyline_length,
    policy_obs::{write_vector, ObsSource, VECTOR_FEATURES, VECTOR_LEN},
    raster::{MapRaster, RasterPatches, BLOCKED, CLIFF, NEST, NEST_ZONE, ROCK, TREE, WATER},
};
use serde::Serialize;
use std::f64::consts::TAU;

/// Ray directions in degrees from the vehicle's forward axis, positive to
/// the right, in circular order (denser ahead).
pub const RAY_ANGLES: [f64; 32] = [
    -164.0, -144.0, -124.0, -106.0, -90.0, -76.0, -64.0, -53.0, -43.0, -34.0, -26.0, -19.0, -13.0,
    -8.0, -4.0, 0.0, 4.0, 8.0, 13.0, 19.0, 26.0, 34.0, 43.0, 53.0, 64.0, 76.0, 90.0, 106.0, 124.0,
    144.0, 164.0, 180.0,
];
pub const RAY_COUNT: usize = RAY_ANGLES.len();
/// Clearance range in tiles beyond the hull.
pub const RAY_RANGE: f64 = 32.0;
/// Per-ray channels, each a block of `RAY_COUNT` features.
pub const RAY_CHANNELS: [&str; 3] = ["hard", "tree", "zone"];
/// Arc lengths (tiles) along the route for the `route<s>_*` features.
pub const ROUTE_LOOKAHEADS: [f64; 4] = [4.0, 8.0, 16.0, 32.0];
const DISTANCE_SCALE: f64 = 64.0;
/// Coarse map: `MAP_CELLS` x `MAP_CELLS` cells of `MAP_CELL_TILES` tiles
/// centered on the vehicle, row 0 ahead, column 0 left. Each cell holds, per
/// ray channel, the fraction of its `MAP_SAMPLES`^2 sample points that hit.
pub const MAP_CELLS: usize = 16;
pub const MAP_CELL_TILES: f64 = 8.0;
pub const MAP_SAMPLES: usize = 4;
pub const MAP_LEN: usize = RAY_CHANNELS.len() * MAP_CELLS * MAP_CELLS;

/// Features copied unchanged from the teacher's vector (`policy_obs::VECTOR_FEATURES`).
const SHARED: [&str; 30] = [
    "target_dir_right",
    "target_dir_forward",
    "straight_distance",
    "target_radius",
    "speed",
    "health",
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

const RAYS_LEN: usize = RAY_CHANNELS.len() * RAY_COUNT;
const ROUTE_LEN: usize = 2 * ROUTE_LOOKAHEADS.len() + 1;
const SHARED_LEN: usize = SHARED.len();
pub const STUDENT_LEN: usize = RAYS_LEN + ROUTE_LEN + SHARED_LEN;

const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn teacher_index(name: &str) -> usize {
    let mut i = 0;
    while i < VECTOR_LEN {
        if str_eq(VECTOR_FEATURES[i], name) {
            return i;
        }
        i += 1;
    }
    panic!("not a teacher vector feature")
}

/// Teacher vector index of each shared feature, in student order.
const SHARED_INDEX: [usize; SHARED_LEN] = {
    let mut out = [0; SHARED_LEN];
    let mut i = 0;
    while i < SHARED_LEN {
        out[i] = teacher_index(SHARED[i]);
        i += 1;
    }
    out
};

/// Feature names in order.
pub fn feature_names() -> Vec<String> {
    let mut names = Vec::with_capacity(STUDENT_LEN);
    for channel in RAY_CHANNELS {
        names.extend((0..RAY_COUNT).map(|i| format!("{channel}_{i}")));
    }
    for s in ROUTE_LOOKAHEADS {
        names.push(format!("route{s}_right"));
        names.push(format!("route{s}_forward"));
    }
    names.push("route_length".into());
    names.extend(SHARED.iter().map(|s| s.to_string()));
    names
}

#[derive(Serialize)]
struct HashedStudentSpec {
    features: Vec<String>,
    ray_angles_deg: [f64; RAY_COUNT],
    ray_range: f64,
    ray_channels: [&'static str; 3],
    route_lookaheads: [f64; 4],
}

#[derive(Serialize)]
struct HashedMapSpec {
    student_spec_hash: String,
    cells: usize,
    cell_tiles: f64,
    samples: usize,
    channels: [&'static str; 3],
}

#[derive(Serialize)]
pub struct MapSpec {
    #[serde(flatten)]
    hashed: HashedMapSpec,
    pub size: usize,
    /// Hash of the student layout with the map appended.
    pub spec_hash: String,
    /// The same with route features from the pathfinder stand-in.
    pub pathfinder_spec_hash: String,
}

#[derive(Serialize)]
pub struct StudentSpec {
    #[serde(flatten)]
    hashed: HashedStudentSpec,
    pub size: usize,
    /// FNV-1a 64 of the JSON of the layout; independent of the teacher's `spec_hash`.
    pub spec_hash: String,
    pub map: MapSpec,
}

pub fn spec() -> StudentSpec {
    let hashed = HashedStudentSpec {
        features: feature_names(),
        ray_angles_deg: RAY_ANGLES,
        ray_range: RAY_RANGE,
        ray_channels: RAY_CHANNELS,
        route_lookaheads: ROUTE_LOOKAHEADS,
    };
    let hash = |json: String| format!("{:016x}", crate::policy_obs::fnv1a(&json));
    let spec_hash = hash(serde_json::to_string(&hashed).expect("static spec"));
    let map = HashedMapSpec {
        student_spec_hash: spec_hash.clone(),
        cells: MAP_CELLS,
        cell_tiles: MAP_CELL_TILES,
        samples: MAP_SAMPLES,
        channels: RAY_CHANNELS,
    };
    let map_hash = hash(serde_json::to_string(&map).expect("static spec"));
    let pathfinder = hash(format!(
        "{map_hash}:pathfinder:{}",
        crate::pathfinder::REPLAN_DEVIATION
    ));
    StudentSpec {
        hashed,
        size: STUDENT_LEN,
        spec_hash,
        map: MapSpec {
            hashed: map,
            size: STUDENT_LEN + MAP_LEN,
            spec_hash: map_hash,
            pathfinder_spec_hash: pathfinder,
        },
    }
}

/// Live mask of a tile: the shared raster with this env's patches applied.
fn tile_mask(raster: &MapRaster, patches: &RasterPatches, x: i32, y: i32) -> u8 {
    match raster.index(x, y) {
        Some(i) => patches.tiles.get(&i).copied().unwrap_or(raster.mask[i]),
        None => raster.mask_at(x, y),
    }
}

/// Blocked by anything but a tree: rock, cliff, nest, water or off-map.
fn is_hard(mask: u8) -> bool {
    mask & (CLIFF | ROCK | WATER | NEST) != 0 || mask & (BLOCKED | TREE) == BLOCKED
}

/// Distance from the center to the edge of a `half_width` x `half_length`
/// rectangle along a direction `angle` radians off its long axis.
fn hull_extent(angle: f64, half_width: f64, half_length: f64) -> f64 {
    let (s, c) = (angle.sin().abs(), angle.cos().abs());
    let across = if s > 1e-9 {
        half_width / s
    } else {
        f64::INFINITY
    };
    let along = if c > 1e-9 {
        half_length / c
    } else {
        f64::INFINITY
    };
    across.min(along)
}

/// Ray distances along world direction `(dx, dy)` from `(x, y)` to the first
/// hard, tree and zone tile, up to `max_t`. Tree and zone hits count only
/// before the hard hit. Grid traversal (Amanatides-Woo): a tile's distance is
/// where the ray enters it, 0 for the start tile. Returns the distances
/// (`max_t` when nothing was hit) and the number of tiles visited.
fn cast(
    raster: &MapRaster,
    patches: &RasterPatches,
    [x, y]: [f64; 2],
    [dx, dy]: [f64; 2],
    max_t: f64,
) -> ([f64; 3], usize) {
    let (mut ix, mut iy) = (x.floor() as i32, y.floor() as i32);
    let step_x = if dx > 0.0 { 1 } else { -1 };
    let step_y = if dy > 0.0 { 1 } else { -1 };
    let delta_x = if dx.abs() > 1e-12 {
        1.0 / dx.abs()
    } else {
        f64::INFINITY
    };
    let delta_y = if dy.abs() > 1e-12 {
        1.0 / dy.abs()
    } else {
        f64::INFINITY
    };
    let mut next_x = if dx > 0.0 {
        ix as f64 + 1.0 - x
    } else {
        x - ix as f64
    } * delta_x;
    let mut next_y = if dy > 0.0 {
        iy as f64 + 1.0 - y
    } else {
        y - iy as f64
    } * delta_y;
    let (mut tree, mut zone) = (max_t, max_t);
    let mut t = 0.0;
    let mut visited = 0;
    while t < max_t {
        let mask = tile_mask(raster, patches, ix, iy);
        visited += 1;
        if is_hard(mask) {
            return ([t, tree.min(t), zone.min(t)], visited);
        }
        if mask & TREE != 0 && tree == max_t {
            tree = t;
        }
        if mask & NEST_ZONE != 0 && zone == max_t {
            zone = t;
        }
        if next_x < next_y {
            t = next_x;
            next_x += delta_x;
            ix += step_x;
        } else {
            t = next_y;
            next_y += delta_y;
            iy += step_y;
        }
    }
    ([max_t, tree, zone], visited)
}

/// Writes the coarse map (`MAP_LEN` values, channel-major) around `(x, y)`
/// facing `heading` radians.
fn write_map(
    raster: &MapRaster,
    patches: &RasterPatches,
    [x, y]: [f64; 2],
    heading: f64,
    out: &mut [f32],
) {
    let (sin, cos) = heading.sin_cos();
    let half = MAP_CELLS as f64 / 2.0;
    let step = MAP_CELL_TILES / MAP_SAMPLES as f64;
    let weight = 1.0 / (MAP_SAMPLES * MAP_SAMPLES) as f32;
    let plane = MAP_CELLS * MAP_CELLS;
    out.fill(0.0);
    for row in 0..MAP_CELLS {
        for col in 0..MAP_CELLS {
            let cell = row * MAP_CELLS + col;
            for sy in 0..MAP_SAMPLES {
                for sx in 0..MAP_SAMPLES {
                    let forward = (half - row as f64) * MAP_CELL_TILES - (sy as f64 + 0.5) * step;
                    let right = (col as f64 - half) * MAP_CELL_TILES + (sx as f64 + 0.5) * step;
                    // Vehicle frame to world: right = (cos, sin), forward = (sin, -cos).
                    let wx = x + right * cos + forward * sin;
                    let wy = y + right * sin - forward * cos;
                    let mask = tile_mask(raster, patches, wx.floor() as i32, wy.floor() as i32);
                    if is_hard(mask) {
                        out[cell] += weight;
                    }
                    if mask & TREE != 0 {
                        out[plane + cell] += weight;
                    }
                    if mask & NEST_ZONE != 0 {
                        out[2 * plane + cell] += weight;
                    }
                }
            }
        }
    }
}

/// Writes the student features in `feature_names()` order, then the coarse
/// map when `out` has room for it. Returns the number of tiles the rays
/// visited (a cost measure for the Lua port).
pub fn write_student(src: ObsSource<'_>, out: &mut [f32]) -> usize {
    let mut teacher = [0.0f32; VECTOR_LEN];
    write_vector(src, &mut teacher);
    let v = src.vehicles;
    let p = &src.data.vehicles[v.prototype[0]];
    let b = p.collision_box;
    let half_width = b[0][0].abs().max(b[1][0].abs());
    let half_length = b[0][1].abs().max(b[1][1].abs());
    let (x, y, heading) = (v.x[0], v.y[0], v.orientation[0] * TAU);

    let mut visited = 0;
    for (i, angle) in RAY_ANGLES.iter().enumerate() {
        let a = angle.to_radians();
        let extent = hull_extent(a, half_width, half_length);
        // World direction of heading + a (orientation 0 = north, clockwise).
        let dir = [(heading + a).sin(), -(heading + a).cos()];
        let (hits, n) = cast(src.raster, src.patches, [x, y], dir, extent + RAY_RANGE);
        visited += n;
        for (c, hit) in hits.iter().enumerate() {
            out[c * RAY_COUNT + i] = ((hit - extent).clamp(0.0, RAY_RANGE) / RAY_RANGE) as f32;
        }
    }

    // The whole route: its length is a feature, and descent is cheap per step.
    let route = match src.route {
        Some(route) => route.to_vec(),
        None => baseline::full_path(src.episode, v),
    };
    let (sin, cos) = heading.sin_cos();
    let to_vehicle = |p: [f64; 2]| {
        [
            (p[0] - x) * cos + (p[1] - y) * sin,
            (p[0] - x) * sin - (p[1] - y) * cos,
        ]
    };
    let mut k = RAYS_LEN;
    for s in ROUTE_LOOKAHEADS {
        let local = to_vehicle(point_along(&route, s));
        out[k] = (local[0] / s) as f32;
        out[k + 1] = (local[1] / s) as f32;
        k += 2;
    }
    out[k] = (polyline_length(&route) / DISTANCE_SCALE).clamp(0.0, 8.0) as f32;
    k += 1;
    for (o, &index) in out[k..STUDENT_LEN].iter_mut().zip(&SHARED_INDEX) {
        *o = teacher[index];
    }
    if out.len() == STUDENT_LEN + MAP_LEN {
        write_map(
            src.raster,
            src.patches,
            [x, y],
            heading,
            &mut out[STUDENT_LEN..],
        );
    }
    visited
}

/// The point at arc length `s` along a polyline, or its end if shorter.
fn point_along(points: &[[f64; 2]], s: f64) -> [f64; 2] {
    let mut left = s;
    for w in points.windows(2) {
        let [a, b] = [w[0], w[1]];
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        if len >= left && len > 0.0 {
            let t = left / len;
            return [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
        }
        left -= len;
    }
    *points.last().expect("route starts at the vehicle")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_consistent() {
        assert_eq!(feature_names().len(), STUDENT_LEN);
        assert_eq!(STUDENT_LEN, 135);
        assert!(RAY_ANGLES.windows(2).all(|w| w[0] < w[1]));
        for (k, name) in SHARED.iter().enumerate() {
            assert_eq!(VECTOR_FEATURES[SHARED_INDEX[k]], *name);
        }
    }

    #[test]
    fn hull_extent_matches_the_box_sides() {
        assert!((hull_extent(0.0, 0.7, 1.0) - 1.0).abs() < 1e-12);
        assert!((hull_extent(TAU / 4.0, 0.7, 1.0) - 0.7).abs() < 1e-9);
        assert!((hull_extent(TAU / 2.0, 0.7, 1.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn point_along_interpolates_and_clamps() {
        let route = [[0.0, 0.0], [3.0, 0.0], [3.0, 4.0]];
        assert_eq!(point_along(&route, 2.0), [2.0, 0.0]);
        assert_eq!(point_along(&route, 5.0), [3.0, 2.0]);
        assert_eq!(point_along(&route, 50.0), [3.0, 4.0]);
    }
}
