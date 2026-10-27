//! Reward terms from docs/rl.md. Each term is reported separately; the reward is their sum.
use serde::{Deserialize, Serialize};

pub const TERMS: [&str; 9] = [
    "progress",
    "time",
    "damage",
    "collision",
    "nest_zone",
    "success",
    "destroyed",
    "arrival",
    "target_speed",
];

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct RewardTerms {
    pub progress: f64,
    pub time: f64,
    pub damage: f64,
    pub collision: f64,
    pub nest_zone: f64,
    pub success: f64,
    pub destroyed: f64,
    pub arrival: f64,
    pub target_speed: f64,
}

impl RewardTerms {
    /// Values in `TERMS` order.
    pub fn values(&self) -> [f64; 9] {
        [
            self.progress,
            self.time,
            self.damage,
            self.collision,
            self.nest_zone,
            self.success,
            self.destroyed,
            self.arrival,
            self.target_speed,
        ]
    }

    pub fn total(&self) -> f64 {
        self.values().iter().sum()
    }

    /// `self += scale * other`, used for totals and discounted macro-step sums.
    pub fn add_scaled(&mut self, other: &RewardTerms, scale: f64) {
        self.progress += scale * other.progress;
        self.time += scale * other.time;
        self.damage += scale * other.damage;
        self.collision += scale * other.collision;
        self.nest_zone += scale * other.nest_zone;
        self.success += scale * other.success;
        self.destroyed += scale * other.destroyed;
        self.arrival += scale * other.arrival;
        self.target_speed += scale * other.target_speed;
    }
}
