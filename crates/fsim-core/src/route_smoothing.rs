//! Cost-aware string pulling over the vehicle-inflated field grid. Shortcuts
//! use continuous headings and supercover traversal, including corner touches.
use crate::distance_field::{FieldParams, Region, F_BLOCKED, F_NEAR, F_TREE, F_ZONE};

const EPS: f64 = 1e-10;
pub const VERSION: &str = "field-smooth-v1";

#[derive(Clone, Debug)]
pub struct RouteGrid {
    region: Region,
    flags: Vec<u8>,
    /// Slow-ground cost per tile, same layout as `flags`; `None` when off.
    friction: Option<Vec<f32>>,
    tree_cost: f64,
    zone_cost: f64,
    clearance_cost: f64,
}

impl RouteGrid {
    pub(crate) fn new(
        region: Region,
        flags: Vec<u8>,
        friction: Option<Vec<f32>>,
        params: FieldParams,
    ) -> Self {
        Self {
            region,
            flags,
            friction,
            tree_cost: params.tree_cost,
            zone_cost: params.zone_cost,
            clearance_cost: params.clearance_cost,
        }
    }

    fn cost(&self, x: i32, y: i32) -> f64 {
        if !self.region.contains(x, y) {
            return f64::INFINITY;
        }
        let stride = self.region.width() + 2;
        let i = (y - self.region.y0 + 1) as usize * stride + (x - self.region.x0 + 1) as usize;
        let f = self.flags[i];
        if f & F_BLOCKED != 0 {
            return f64::INFINITY;
        }
        1.0 + if f & F_TREE != 0 { self.tree_cost } else { 0.0 }
            + if f & F_ZONE != 0 { self.zone_cost } else { 0.0 }
            + self.friction.as_ref().map_or(0.0, |c| c[i] as f64)
            + if f & F_NEAR != 0 {
                self.clearance_cost
            } else {
                0.0
            }
    }

    pub fn smooth(&self, points: &[[f64; 2]]) -> Vec<[f64; 2]> {
        smooth(points, |x, y| self.cost(x, y))
    }
}

fn point_open(p: [f64; 2], cost: &impl Fn(i32, i32) -> f64) -> bool {
    let (x, y) = (p[0].floor() as i32, p[1].floor() as i32);
    let on_x = (p[0] - x as f64).abs() < EPS;
    let on_y = (p[1] - y as f64).abs() < EPS;
    cost(x, y).is_finite()
        && (!on_x || cost(x - 1, y).is_finite())
        && (!on_y || cost(x, y - 1).is_finite())
        && (!(on_x && on_y) || cost(x - 1, y - 1).is_finite())
}

/// Integrate terrain cost along a segment. Touching a blocked corner/border
/// rejects the shortcut; a zero-length touch adds no terrain cost.
fn line_cost(a: [f64; 2], b: [f64; 2], cost: &impl Fn(i32, i32) -> f64, budget: f64) -> f64 {
    if !point_open(a, cost) || !point_open(b, cost) {
        return f64::INFINITY;
    }
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = (dx * dx + dy * dy).sqrt();
    if length < EPS {
        return 0.0;
    }
    let (mut x, mut y) = (a[0].floor() as i32, a[1].floor() as i32);
    let (sx, sy) = (if dx > 0.0 { 1 } else { -1 }, if dy > 0.0 { 1 } else { -1 });
    let (dt_x, dt_y) = (
        if dx.abs() > EPS {
            1.0 / dx.abs()
        } else {
            f64::INFINITY
        },
        if dy.abs() > EPS {
            1.0 / dy.abs()
        } else {
            f64::INFINITY
        },
    );
    let mut tx = if dx > 0.0 {
        x as f64 + 1.0 - a[0]
    } else {
        a[0] - x as f64
    };
    let mut ty = if dy > 0.0 {
        y as f64 + 1.0 - a[1]
    } else {
        a[1] - y as f64
    };
    tx = if dt_x.is_finite() {
        tx * dt_x
    } else {
        f64::INFINITY
    };
    ty = if dt_y.is_finite() {
        ty * dt_y
    } else {
        f64::INFINITY
    };
    let along_x = dx.abs() < EPS && (a[0] - a[0].floor()).abs() < EPS;
    let along_y = dy.abs() < EPS && (a[1] - a[1].floor()).abs() < EPS;
    let (mut t, mut total) = (0.0, 0.0);
    while t < 1.0 {
        let cell = cost(x, y);
        if !cell.is_finite()
            || (along_x && !cost(x - 1, y).is_finite())
            || (along_y && !cost(x, y - 1).is_finite())
        {
            return f64::INFINITY;
        }
        let next = tx.min(ty).min(1.0);
        total += (next - t) * length * cell;
        if total > budget + 1e-8 {
            return f64::INFINITY;
        }
        if next >= 1.0 {
            break;
        }
        if (tx - ty).abs() < EPS {
            if !cost(x + sx, y).is_finite() || !cost(x, y + sy).is_finite() {
                return f64::INFINITY;
            }
            x += sx;
            y += sy;
            tx += dt_x;
            ty += dt_y;
        } else if tx < ty {
            x += sx;
            tx += dt_x;
        } else {
            y += sy;
            ty += dt_y;
        }
        t = next;
    }
    total
}

fn smooth(points: &[[f64; 2]], cost: impl Fn(i32, i32) -> f64) -> Vec<[f64; 2]> {
    if points.len() < 3 {
        return points.to_vec();
    }
    // Prefix costs let every proposed segment compare against exactly the raw
    // span it replaces. Invalid raw endpoint spans force a fallback, not a cut.
    let mut prefix = vec![0.0; points.len()];
    let mut invalid = vec![0usize; points.len()];
    for i in 1..points.len() {
        let c = line_cost(points[i - 1], points[i], &cost, f64::INFINITY);
        prefix[i] = prefix[i - 1] + if c.is_finite() { c } else { 0.0 };
        invalid[i] = invalid[i - 1] + usize::from(!c.is_finite());
    }
    let mut out = vec![points[0]];
    let mut anchor = 0;
    while anchor + 1 < points.len() {
        let mut candidate = points.len() - 1;
        while candidate > anchor + 1 {
            let budget = prefix[candidate] - prefix[anchor];
            if invalid[candidate] == invalid[anchor]
                && line_cost(points[anchor], points[candidate], &cost, budget).is_finite()
            {
                break;
            }
            candidate = anchor + (candidate - anchor) / 2;
        }
        out.push(points[candidate]);
        anchor = candidate;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn length(p: &[[f64; 2]]) -> f64 {
        p.windows(2)
            .map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt())
            .sum()
    }
    fn staircase() -> Vec<[f64; 2]> {
        let mut p = vec![[0.5, 0.5]];
        p.extend((1..=10).map(|i| [i as f64 + 0.5, i as f64 + 0.5]));
        p.extend((11..=30).map(|i| [i as f64 + 0.5, 10.5]));
        p
    }
    #[test]
    fn open_ground_uses_one_arbitrary_angle_segment() {
        let raw = staircase();
        let out = smooth(&raw, |_, _| 1.0);
        assert_eq!(out, vec![raw[0], *raw.last().unwrap()]);
        assert!(length(&out) < length(&raw) * 0.95);
        let heading = (10.0f64 / 30.0).atan().to_degrees();
        assert!(heading > 18.0 && heading < 19.0);
    }
    #[test]
    fn shortcuts_retain_clearance_and_expensive_zone_detours() {
        let mut raw = vec![[0.5, 0.5]];
        raw.extend((1..=8).map(|y| [0.5, y as f64 + 0.5]));
        raw.extend((1..=20).map(|x| [x as f64 + 0.5, 8.5]));
        raw.extend((0..8).rev().map(|y| [20.5, y as f64 + 0.5]));
        for penalty in [f64::INFINITY, 10.0] {
            let cost = |x: i32, y: i32| {
                if (8..=12).contains(&x) && y <= 5 {
                    penalty
                } else {
                    1.0
                }
            };
            let out = smooth(&raw, cost);
            assert!(out.len() > 2);
            let a: f64 = raw
                .windows(2)
                .map(|w| line_cost(w[0], w[1], &cost, f64::INFINITY))
                .sum();
            let b: f64 = out
                .windows(2)
                .map(|w| line_cost(w[0], w[1], &cost, f64::INFINITY))
                .sum();
            assert!(b.is_finite() && b <= a + 1e-8);
            assert!(length(&out) <= length(&raw));
        }
    }
    #[test]
    fn blocked_corner_and_border_touches_are_rejected() {
        let cost = |x, y| if x == 1 && y == 0 { f64::INFINITY } else { 1.0 };
        assert!(!line_cost([0.5, 0.5], [1.5, 1.5], &cost, f64::INFINITY).is_finite());
        assert!(!line_cost([0.5, 1.0], [2.5, 1.0], &cost, f64::INFINITY).is_finite());
    }
}
