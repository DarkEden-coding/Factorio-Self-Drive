//! Path-distance field on the tile grid (docs/rl.md, "Distance field").
//!
//! 8-connected Dijkstra over a rectangular region. Water, cliffs, nests and
//! rocks block; trees block the car and cost extra for the tank. Obstacles are
//! dilated by the vehicle's half-width rounded up. Tiles inside a nest penalty
//! radius, dilated by a margin, cost extra so routes keep clear of zones.
//! Diagonal steps need both orthogonal neighbors free, so paths never cut
//! obstacle corners.
//!
//! Every edge costs at least 1, so a bucket queue with unit-width buckets pops
//! nodes in exact Dijkstra order without a heap.
use crate::{
    prototypes::VehiclePrototype,
    raster::{MapRaster, CLIFF, NEST, NEST_ZONE, ROCK, TREE, WATER},
    rl_config::FieldConfig,
};
use serde::Serialize;
use std::f64::consts::SQRT_2;

const HARD: u8 = CLIFF | NEST | ROCK | WATER;
const F_BLOCKED: u8 = 1;
const F_TREE: u8 = 2;
const F_ZONE: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldParams {
    /// Obstacle dilation in tiles.
    pub dilation: i32,
    pub trees_block: bool,
    /// Extra cost per tile for trees when they don't block.
    pub tree_cost: f64,
    /// Extra cost per tile inside a nest penalty radius plus `zone_margin`.
    pub zone_cost: f64,
    pub zone_margin: i32,
}

impl FieldParams {
    pub fn new(vehicle: &VehiclePrototype, config: &FieldConfig) -> Self {
        let b = vehicle.collision_box;
        let half_width = b[0][0].abs().max(b[1][0].abs());
        Self {
            dilation: half_width.ceil() as i32,
            trees_block: !vehicle.tank_driving,
            tree_cost: config.tank_tree_cost,
            zone_cost: config.nest_zone_cost,
            zone_margin: config.nest_zone_margin,
        }
    }

    /// The same parameters with nest zones at no extra cost.
    pub fn without_zones(self) -> Self {
        Self {
            zone_cost: 0.0,
            ..self
        }
    }
}

/// Tile rectangle `[x0, x1) x [y0, y1)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Region {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Region {
    /// Bounding box of the points' tiles plus `margin`, clipped to the map.
    pub fn around(points: &[[f64; 2]], margin: i32, raster: &MapRaster) -> Self {
        let mut r = Region {
            x0: i32::MAX,
            y0: i32::MAX,
            x1: i32::MIN,
            y1: i32::MIN,
        };
        for p in points {
            let (x, y) = (p[0].floor() as i32, p[1].floor() as i32);
            r.x0 = r.x0.min(x - margin);
            r.y0 = r.y0.min(y - margin);
            r.x1 = r.x1.max(x + margin + 1);
            r.y1 = r.y1.max(y + margin + 1);
        }
        r.x0 = r.x0.max(raster.x_min);
        r.y0 = r.y0.max(raster.y_min);
        r.x1 = r.x1.min(raster.x_min + raster.width as i32);
        r.y1 = r.y1.min(raster.y_min + raster.height as i32);
        r
    }
    pub fn width(&self) -> usize {
        (self.x1 - self.x0).max(0) as usize
    }
    pub fn height(&self) -> usize {
        (self.y1 - self.y0).max(0) as usize
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

#[derive(Clone, Debug)]
pub struct DistanceField {
    pub region: Region,
    /// Row-major path distance at each tile center; infinity = blocked or unreachable.
    pub values: Vec<f32>,
}

/// Neighbor steps as (dx, dy, length).
const STEPS: [(i32, i32, f64); 8] = [
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, SQRT_2),
    (-1, 1, SQRT_2),
    (1, -1, SQRT_2),
    (-1, -1, SQRT_2),
];

/// Per-tile cost flags for `region`, dilated, with a one-tile blocked border.
fn cost_flags(raster: &MapRaster, region: Region, params: FieldParams) -> Vec<u8> {
    let obstacles = dilate(raster, region, params.dilation, |mask| {
        let tree = mask & TREE != 0;
        if mask & HARD != 0 || (tree && params.trees_block) {
            F_BLOCKED
        } else if tree {
            F_TREE
        } else {
            0
        }
    });
    let zones = (params.zone_cost > 0.0).then(|| {
        dilate(raster, region, params.zone_margin, |mask| {
            if mask & NEST_ZONE != 0 {
                F_ZONE
            } else {
                0
            }
        })
    });
    let (w, h) = (region.width(), region.height());
    let stride = w + 2;
    let mut flags = vec![F_BLOCKED; stride * (h + 2)];
    for y in 0..h {
        for x in 0..w {
            let v = obstacles[y * w + x];
            let zone = zones.as_ref().map_or(0, |z| z[y * w + x]);
            flags[(y + 1) * stride + x + 1] = if v & F_BLOCKED != 0 {
                F_BLOCKED
            } else {
                v | zone
            };
        }
    }
    flags
}

/// `bits(mask)` per tile of `region`, OR-ed over the square of half-size `d`
/// around each tile. Row-major `width x height`.
fn dilate(raster: &MapRaster, region: Region, d: i32, bits: impl Fn(u8) -> u8) -> Vec<u8> {
    let d = d.max(0);
    let (w, h) = (region.width(), region.height());
    let ew = w + 2 * d as usize;
    let eh = h + 2 * d as usize;
    let mut raw = vec![0u8; ew * eh];
    for ey in 0..eh {
        let y = region.y0 - d + ey as i32;
        for ex in 0..ew {
            raw[ey * ew + ex] = bits(raster.mask_at(region.x0 - d + ex as i32, y));
        }
    }
    // Separable: OR over a (2d+1) window horizontally, then vertically.
    let d = d as usize;
    let mut horizontal = vec![0u8; w * eh];
    for ey in 0..eh {
        let row = &raw[ey * ew..(ey + 1) * ew];
        for x in 0..w {
            horizontal[ey * w + x] = row[x..=x + 2 * d].iter().fold(0, |a, b| a | b);
        }
    }
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = (0..=2 * d).fold(0, |v, k| v | horizontal[(y + k) * w + x]);
        }
    }
    out
}

impl DistanceField {
    /// Path distance from `source` to every tile center of `region`. The
    /// source tile starts at the straight distance from `source` to its
    /// center, so it may lie in a blocked tile. Nodes beyond `max_value`
    /// stay infinite.
    pub fn compute(
        raster: &MapRaster,
        region: Region,
        params: FieldParams,
        source: [f64; 2],
        max_value: f64,
    ) -> Self {
        let (w, h) = (region.width(), region.height());
        let stride = w + 2;
        let flags = cost_flags(raster, region, params);
        let mut dist = vec![f64::INFINITY; flags.len()];
        let (sx, sy) = (source[0].floor() as i32, source[1].floor() as i32);
        let max_edge = SQRT_2 * (1.0 + params.tree_cost + params.zone_cost);
        let ring = max_edge.ceil() as usize + 2;
        let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); ring];
        let mut pending = 0usize;
        if region.contains(sx, sy) {
            let p = (sy - region.y0 + 1) as usize * stride + (sx - region.x0 + 1) as usize;
            let d0 = (sx as f64 + 0.5 - source[0]).hypot(sy as f64 + 0.5 - source[1]);
            dist[p] = d0;
            buckets[d0 as usize % ring].push(p as u32);
            pending = 1;
        }
        let offsets: [isize; 8] =
            STEPS.map(|(dx, dy, _)| dy as isize * stride as isize + dx as isize);
        let extra = |f: u8| {
            let tree = if f & F_TREE != 0 {
                params.tree_cost
            } else {
                0.0
            };
            let zone = if f & F_ZONE != 0 {
                params.zone_cost
            } else {
                0.0
            };
            tree + zone
        };
        let mut done = vec![false; flags.len()];
        let mut current = 0usize;
        let mut scratch = Vec::new();
        while pending > 0 && current as f64 <= max_value {
            std::mem::swap(&mut scratch, &mut buckets[current % ring]);
            pending -= scratch.len();
            for &node in &scratch {
                let u = node as usize;
                if done[u] || dist[u] as usize != current {
                    continue;
                }
                done[u] = true;
                let du = dist[u];
                let eu = extra(flags[u]);
                for (k, &(_, _, length)) in STEPS.iter().enumerate() {
                    let v = (u as isize + offsets[k]) as usize;
                    if flags[v] & F_BLOCKED != 0 || done[v] {
                        continue;
                    }
                    if k >= 4 {
                        // No corner cutting: both orthogonal cells must be open.
                        let ox = (u as isize + STEPS[k].0 as isize) as usize;
                        let oy = (u as isize + STEPS[k].1 as isize * stride as isize) as usize;
                        if flags[ox] & F_BLOCKED != 0 || flags[oy] & F_BLOCKED != 0 {
                            continue;
                        }
                    }
                    let dv = du + length * (1.0 + 0.5 * (eu + extra(flags[v])));
                    if dv < dist[v] {
                        dist[v] = dv;
                        buckets[dv as usize % ring].push(v as u32);
                        pending += 1;
                    }
                }
            }
            scratch.clear();
            current += 1;
        }
        let mut values = vec![f32::INFINITY; w * h];
        for y in 0..h {
            for x in 0..w {
                let d = dist[(y + 1) * stride + x + 1];
                if d <= max_value {
                    values[y * w + x] = d as f32;
                }
            }
        }
        Self { region, values }
    }

    /// Value at tile `(x, y)`; infinity outside the region.
    pub fn tile(&self, x: i32, y: i32) -> f64 {
        if !self.region.contains(x, y) {
            return f64::INFINITY;
        }
        let i = (y - self.region.y0) as usize * self.region.width() + (x - self.region.x0) as usize;
        self.values[i] as f64
    }

    /// Bilinear interpolation between tile centers, over finite corners only.
    /// If none of the four corners is usable, falls back to the nearest
    /// finite tile within 3 tiles plus the straight distance to it.
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let fx = x - 0.5;
        let fy = y - 0.5;
        let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
        let (tx, ty) = (fx - ix as f64, fy - iy as f64);
        let mut sum = 0.0;
        let mut weight = 0.0;
        for (dx, dy, w) in [
            (0, 0, (1.0 - tx) * (1.0 - ty)),
            (1, 0, tx * (1.0 - ty)),
            (0, 1, (1.0 - tx) * ty),
            (1, 1, tx * ty),
        ] {
            let v = self.tile(ix + dx, iy + dy);
            if v.is_finite() && w > 0.0 {
                sum += v * w;
                weight += w;
            }
        }
        if weight > 1e-9 {
            return sum / weight;
        }
        let (cx, cy) = (x.floor() as i32, y.floor() as i32);
        let mut best = f64::INFINITY;
        for ny in cy - 3..=cy + 3 {
            for nx in cx - 3..=cx + 3 {
                let v = self.tile(nx, ny);
                if v.is_finite() {
                    best = best.min(v + (nx as f64 + 0.5 - x).hypot(ny as f64 + 0.5 - y));
                }
            }
        }
        best
    }

    /// Greedy steepest descent from the tile containing `(x, y)` (or the best
    /// finite tile near it) to the source. Returns tile centers, excluding
    /// the start tile, at most `max_steps` of them.
    pub fn descend(&self, x: f64, y: f64, max_steps: usize) -> Vec<[f64; 2]> {
        let (mut cx, mut cy) = (x.floor() as i32, y.floor() as i32);
        let mut path = Vec::new();
        if !self.tile(cx, cy).is_finite() {
            let mut best = (f64::INFINITY, cx, cy);
            for ny in cy - 3..=cy + 3 {
                for nx in cx - 3..=cx + 3 {
                    let v = self.tile(nx, ny);
                    let cost = v + (nx as f64 + 0.5 - x).hypot(ny as f64 + 0.5 - y);
                    if cost < best.0 {
                        best = (cost, nx, ny);
                    }
                }
            }
            if !best.0.is_finite() {
                return path;
            }
            (cx, cy) = (best.1, best.2);
            path.push([cx as f64 + 0.5, cy as f64 + 0.5]);
        }
        while path.len() < max_steps {
            let current = self.tile(cx, cy);
            let mut best = (current, cx, cy);
            for (k, &(dx, dy, _)) in STEPS.iter().enumerate() {
                let (nx, ny) = (cx + dx, cy + dy);
                if k >= 4
                    && !(self.tile(cx + dx, cy).is_finite() && self.tile(cx, cy + dy).is_finite())
                {
                    continue;
                }
                let v = self.tile(nx, ny);
                if v < best.0 {
                    best = (v, nx, ny);
                }
            }
            if best.1 == cx && best.2 == cy {
                break;
            }
            (cx, cy) = (best.1, best.2);
            path.push([cx as f64 + 0.5, cy as f64 + 0.5]);
        }
        path
    }

    /// The field's route from `(x, y)` to `source` as world points: the start,
    /// the tile centers of the steepest descent, then `source`.
    pub fn route(&self, x: f64, y: f64, source: [f64; 2]) -> Vec<[f64; 2]> {
        let mut points = vec![[x, y]];
        points.extend(self.descend(x, y, self.region.width() * self.region.height()));
        points.push(source);
        points
    }
}

/// Length of a polyline in tiles.
pub fn polyline_length(points: &[[f64; 2]]) -> f64 {
    points
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum()
}
