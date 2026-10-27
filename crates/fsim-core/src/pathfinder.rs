//! Stand-in for Factorio's `LuaSurface.request_path` as the student's route
//! source (docs/rl.md, "Pathfinder route"). The game's pathfinder avoids every
//! colliding entity and water but knows nothing of nest zones, and returns a
//! fixed list of waypoints that the mod re-requests when the vehicle strays.
//! Here: a target-rooted distance field with trees blocking and no zone cost,
//! its descent from the request point simplified to straight segments, and a
//! new request when the vehicle is more than `REPLAN_DEVIATION` tiles from the
//! waypoints or reaches their end.
use crate::{
    distance_field::{DistanceField, FieldParams, Region},
    prototypes::VehiclePrototype,
    raster::MapRaster,
    rl_config::FieldConfig,
};

/// Tiles from the waypoints that trigger a new request.
pub const REPLAN_DEVIATION: f64 = 16.0;
/// Douglas-Peucker tolerance in tiles (the game's `prefer_straight_paths`).
const SIMPLIFY_TOLERANCE: f64 = 1.0;

pub struct PathRoute {
    field: DistanceField,
    target: [f64; 2],
    waypoints: Vec<[f64; 2]>,
    /// Segment of `waypoints` the vehicle was last projected on.
    segment: usize,
    pub requests: u32,
}

impl PathRoute {
    pub fn new(
        raster: &MapRaster,
        vehicle: &VehiclePrototype,
        config: &FieldConfig,
        start: [f64; 2],
        target: [f64; 2],
    ) -> Self {
        let region = Region::around(&[start, target], config.margin, raster);
        let params = FieldParams {
            trees_block: true,
            ..FieldParams::new(vehicle, config).without_zones()
        };
        let mut field = DistanceField::compute(raster, region, params, target, f64::INFINITY);
        if !field.sample(start[0], start[1]).is_finite() {
            // Trees close the way (a tank in a forest): let them cost instead.
            let params = FieldParams::new(vehicle, config).without_zones();
            field = DistanceField::compute(raster, region, params, target, f64::INFINITY);
        }
        Self {
            field,
            target,
            waypoints: Vec::new(),
            segment: 0,
            requests: 0,
        }
    }

    /// The route from `position` along the current waypoints, requesting new
    /// ones when needed: `position`, its projection, then the later waypoints.
    pub fn route(&mut self, position: [f64; 2]) -> Vec<[f64; 2]> {
        let (segment, point, deviation) = project(&self.waypoints, self.segment, position);
        let at_end = segment + 2 >= self.waypoints.len()
            && distance(point, self.target) > 1.0
            && distance(position, self.target) > REPLAN_DEVIATION;
        if self.waypoints.len() < 2 || deviation > REPLAN_DEVIATION || at_end {
            self.request(position);
            return self.waypoints.clone();
        }
        self.segment = segment;
        let mut route = vec![position, point];
        route.extend_from_slice(&self.waypoints[segment + 1..]);
        route
    }

    fn request(&mut self, position: [f64; 2]) {
        let route = self.field.route(position[0], position[1], self.target);
        self.waypoints = simplify(&route, SIMPLIFY_TOLERANCE);
        self.segment = 0;
        self.requests += 1;
    }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn closest_on_segment(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> [f64; 2] {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    [a[0] + t * d[0], a[1] + t * d[1]]
}

/// Closest point to `p` on segments `from..` of `points` (never going back,
/// so a route that doubles back keeps its order): (segment, point, distance).
fn project(points: &[[f64; 2]], from: usize, p: [f64; 2]) -> (usize, [f64; 2], f64) {
    let mut best = (from, p, f64::INFINITY);
    for (k, w) in points.windows(2).enumerate().skip(from) {
        let q = closest_on_segment(w[0], w[1], p);
        let d = distance(q, p);
        if d < best.2 {
            best = (k, q, d);
        }
    }
    best
}

/// Douglas-Peucker simplification keeping the end points.
fn simplify(points: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut worst = (0.0, a);
        for i in a + 1..b {
            let d = distance(
                closest_on_segment(points[a], points[b], points[i]),
                points[i],
            );
            if d > worst.0 {
                worst = (d, i);
            }
        }
        if worst.0 > tolerance {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_keeps_corners_only() {
        let line: Vec<[f64; 2]> = (0..=10)
            .map(|i| [i as f64, 0.0])
            .chain((1..=10).map(|i| [10.0, i as f64]))
            .collect();
        assert_eq!(
            simplify(&line, 0.5),
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]
        );
    }

    #[test]
    fn project_moves_forward_only() {
        let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let (k, q, d) = project(&pts, 0, [5.0, 1.0]);
        assert_eq!((k, q, d), (0, [5.0, 0.0], 1.0));
        // From segment 2 on, a point near the first segment projects onto the last one.
        let (k, _, _) = project(&pts, 2, [5.0, 1.0]);
        assert_eq!(k, 2);
    }
}
