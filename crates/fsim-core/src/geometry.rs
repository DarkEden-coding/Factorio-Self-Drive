use crate::prototypes::CollisionBox;
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug)]
pub struct Obb {
    pub center: [f64; 2],
    pub half: [f64; 2],
    pub axes: [[f64; 2]; 2],
}

impl Obb {
    pub fn new(x: f64, y: f64, bounds: CollisionBox, orientation: f64) -> Self {
        let (s, c) = (orientation * TAU).sin_cos();
        let local_x = (bounds[0][0] + bounds[1][0]) * 0.5;
        let local_y = (bounds[0][1] + bounds[1][1]) * 0.5;
        Self {
            center: [x + c * local_x - s * local_y, y + s * local_x + c * local_y],
            half: [
                (bounds[1][0] - bounds[0][0]) * 0.5,
                (bounds[1][1] - bounds[0][1]) * 0.5,
            ],
            axes: [[c, s], [-s, c]],
        }
    }

    pub fn aabb(self) -> CollisionBox {
        let dx = self.axes[0][0].abs() * self.half[0] + self.axes[1][0].abs() * self.half[1];
        let dy = self.axes[0][1].abs() * self.half[0] + self.axes[1][1].abs() * self.half[1];
        [
            [self.center[0] - dx, self.center[1] - dy],
            [self.center[0] + dx, self.center[1] + dy],
        ]
    }

    pub fn intersects(self, other: Self) -> bool {
        let delta = [
            other.center[0] - self.center[0],
            other.center[1] - self.center[1],
        ];
        for axis in self.axes.into_iter().chain(other.axes) {
            let radius = |b: Self| {
                b.half[0] * dot(b.axes[0], axis).abs() + b.half[1] * dot(b.axes[1], axis).abs()
            };
            if dot(delta, axis).abs() >= radius(self) + radius(other) - 1e-9 {
                return false;
            }
        }
        true
    }
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
