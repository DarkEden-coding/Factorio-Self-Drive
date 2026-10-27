use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TargetState {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    pub distance: f64,
    pub reached: bool,
}

impl Target {
    pub fn state(self, x: f64, y: f64) -> TargetState {
        let distance = (x - self.x).hypot(y - self.y);
        TargetState {
            x: self.x,
            y: self.y,
            radius: self.radius,
            distance,
            reached: distance <= self.radius,
        }
    }
}
