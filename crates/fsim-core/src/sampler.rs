//! Episode sampling (docs/rl.md, "Episodes" and "Scenarios"): a valid start
//! pose and a target for one scenario kind. Every kind draws starts and
//! targets at random and rejects the ones that miss its conditions.
use crate::{
    distance_field::{polyline_length, DistanceField, FieldParams, Region},
    geometry::Obb,
    physics,
    prototypes::Prototypes,
    raster::{MapRaster, CLIFF, NEST, NEST_ZONE, ROCK, TREE, WATER},
    rl_config::{
        ArrivalScenario, EpisodeRules, FieldConfig, ForestScenario, LongScenario, NestScenario,
        ReverseScenario, Scenario, TargetConfig,
    },
    rl_episode::{episode_field, in_nest_zone, pose_is_clear, Point, Pose},
    world::World,
};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg32;
use std::f64::consts::TAU;

/// Starts stay this many tiles inside the map so the local grid is on-map.
pub const START_MARGIN: i32 = 32;
const START_ATTEMPTS: usize = 64;
const TARGET_ATTEMPTS: usize = 4;
/// Long routes cost a map-sized field per start, so they try fewer starts.
const LONG_START_ATTEMPTS: usize = 8;
/// Draws for starts with a rare condition (forest, reverse, nest), checked cheaply first.
const START_DRAWS: usize = 4000;
/// Valid starts tried for the rare-start kinds.
const RARE_START_ATTEMPTS: usize = 16;
/// Candidate targets checked on the start's field before confirming one.
const PRECHECKS: usize = 64;
/// Hull sweep step when measuring the free distance ahead, in tiles.
const GAP_STEP: f64 = 0.05;
/// In a nest episode, staying out of the zones (without the field's margin)
/// costs at least this much more than the zone-free route, so every
/// zone-free shortest route enters a zone.
const NEST_MIN_DETOUR: f64 = 2.0;
/// Zone cost for `zone_skirting_field`: high enough that no route enters.
const SKIRT_ZONE_COST: f64 = 100.0;
/// Nest starts lie this far outside a zone edge, in tiles.
const NEST_START_OFFSET: (f64, f64) = (4.0, 40.0);
/// Arrival starts aim at the route point this many tiles ahead.
const ARRIVAL_LOOKAHEAD: usize = 4;

pub struct Sampled {
    pub start: Pose,
    /// Signed speed along the start heading, tiles/tick.
    pub start_speed: f64,
    pub target: Point,
    pub field: DistanceField,
}

/// Rejection counters, for the benchmark.
#[derive(Clone, Copy, Debug, Default)]
pub struct SampleStats {
    /// Start poses drawn and valid starts among them.
    pub start_draws: u64,
    pub starts: u64,
    /// Targets confirmed with an episode field and those accepted.
    pub target_checks: u64,
    pub targets: u64,
}

/// Samples a standard start and target on `world` for `vehicle`, with the
/// path distance range in `target`. Deterministic in `seed`.
pub fn sample(
    world: &World,
    raster: &MapRaster,
    data: &Prototypes,
    vehicle: usize,
    rules: &EpisodeRules,
    target: &TargetConfig,
    seed: u64,
) -> Result<Sampled, String> {
    let standard = Scenario::Standard(Default::default());
    let mut stats = SampleStats::default();
    sample_scenario(
        world, raster, data, vehicle, rules, &standard, target, seed, &mut stats,
    )
}

/// Samples a start and target of the given scenario kind. `target` is the
/// curriculum range, used by `standard` only. Deterministic in `seed`.
#[allow(clippy::too_many_arguments)]
pub fn sample_scenario(
    world: &World,
    raster: &MapRaster,
    data: &Prototypes,
    vehicle: usize,
    rules: &EpisodeRules,
    scenario: &Scenario,
    target: &TargetConfig,
    seed: u64,
    stats: &mut SampleStats,
) -> Result<Sampled, String> {
    crate::rl_config::validate_target(target)?;
    let mut rng = Pcg32::seed_from_u64(seed);
    let search = Search {
        world,
        raster,
        data,
        vehicle,
        rules,
        params: FieldParams::new(&data.vehicles[vehicle], &rules.distance_field),
        area: StartArea::new(raster),
    };
    let found = match scenario {
        Scenario::Standard(_) => search.ranged(&mut rng, target, START_ATTEMPTS, false, stats),
        Scenario::Long(p) => search.long(&mut rng, p, stats),
        Scenario::Forest(p) => search.forest(&mut rng, p, stats),
        Scenario::Reverse(p) => search.reverse(&mut rng, p, stats),
        Scenario::Nest(p) => search.nest(&mut rng, p, stats),
        Scenario::Arrival(p) => search.arrival(&mut rng, p, stats),
    };
    found.ok_or_else(|| {
        format!(
            "no {} start/target pair found on {}",
            scenario.name(),
            world.info.name
        )
    })
}

/// A random valid start pose for `vehicle` (same rules as `sample`), without a
/// target. Deterministic in `seed`. Fails only if `max_attempts` draws miss.
pub fn sample_start(
    world: &World,
    raster: &MapRaster,
    data: &Prototypes,
    vehicle: usize,
    field: &FieldConfig,
    seed: u64,
    max_attempts: usize,
) -> Result<Pose, String> {
    let mut rng = Pcg32::seed_from_u64(seed);
    let params = FieldParams::new(&data.vehicles[vehicle], field);
    let area = StartArea::new(raster);
    (0..max_attempts)
        .find_map(|_| {
            let (tx, ty, pose) = area.draw(&mut rng);
            valid_start(world, raster, data, vehicle, params, tx, ty, pose).then_some(pose)
        })
        .ok_or_else(|| {
            format!(
                "no valid start pose on {} after {max_attempts} attempts",
                world.info.name
            )
        })
}

struct Search<'a> {
    world: &'a World,
    raster: &'a MapRaster,
    data: &'a Prototypes,
    vehicle: usize,
    rules: &'a EpisodeRules,
    params: FieldParams,
    area: StartArea,
}

impl Search<'_> {
    /// A random valid start whose tile passes `cheap` first.
    fn start(
        &self,
        rng: &mut Pcg32,
        draws: usize,
        stats: &mut SampleStats,
        cheap: impl Fn(i32, i32) -> bool,
    ) -> Option<Pose> {
        for _ in 0..draws {
            stats.start_draws += 1;
            let (tx, ty, pose) = self.area.draw(rng);
            if cheap(tx, ty)
                && valid_start(
                    self.world,
                    self.raster,
                    self.data,
                    self.vehicle,
                    self.params,
                    tx,
                    ty,
                    pose,
                )
            {
                stats.starts += 1;
                return Some(pose);
            }
        }
        None
    }

    /// Path distances from `start` with `params`, out to `max_distance`.
    fn start_field(&self, start: Pose, max_distance: f64, params: FieldParams) -> DistanceField {
        let reach = max_distance.ceil() as i32 + 1;
        let region = Region::around(&[[start.x, start.y]], reach, self.raster);
        DistanceField::compute(
            self.raster,
            region,
            params,
            [start.x, start.y],
            max_distance,
        )
    }

    /// The episode toward `point`, if the start's path distance is finite and,
    /// with a `range`, fits it.
    fn confirm(
        &self,
        start: Pose,
        point: Point,
        range: Option<&TargetConfig>,
        stats: &mut SampleStats,
    ) -> Option<Sampled> {
        stats.target_checks += 1;
        let field = episode_field(
            self.raster,
            self.data,
            self.vehicle,
            self.rules,
            [start.x, start.y],
            [point.x, point.y],
        );
        let optimal = field.sample(start.x, start.y);
        let fits = optimal.is_finite()
            && range.is_none_or(|r| {
                let straight = (point.x - start.x).hypot(point.y - start.y);
                (r.min_distance..=r.max_distance).contains(&optimal)
                    && optimal <= r.max_tortuosity * straight
            });
        fits.then_some(Sampled {
            start,
            start_speed: 0.0,
            target: point,
            field,
        })
    }

    /// Standard and long episodes: targets whose path distance fits `range`.
    /// With `uniform_distance`, far tiles are thinned so the path distance is
    /// about uniform over the range instead of growing with the ring area.
    fn ranged(
        &self,
        rng: &mut Pcg32,
        range: &TargetConfig,
        attempts: usize,
        uniform_distance: bool,
        stats: &mut SampleStats,
    ) -> Option<Sampled> {
        for _ in 0..attempts {
            let Some(start) = self.start(rng, 1, stats, |_, _| true) else {
                continue;
            };
            let field = self.start_field(start, range.max_distance, self.params);
            let mut candidates = candidates(&field, start, range, |_, _| true);
            let mut tries = 0;
            for i in 0..candidates.len() {
                if tries == TARGET_ATTEMPTS {
                    break;
                }
                let (x, y, value) = shuffle_step(rng, &mut candidates, i);
                if uniform_distance && value > 0.0 && rng.gen::<f64>() > range.min_distance / value
                {
                    continue;
                }
                tries += 1;
                if let Some(found) = self.confirm(start, tile_center(x, y), Some(range), stats) {
                    stats.targets += 1;
                    return Some(found);
                }
            }
        }
        None
    }

    fn long(&self, rng: &mut Pcg32, p: &LongScenario, stats: &mut SampleStats) -> Option<Sampled> {
        let range = TargetConfig {
            min_distance: p.min_distance,
            max_distance: p.max_distance,
            max_tortuosity: p.max_tortuosity,
            ..TargetConfig::default()
        };
        // Retry a few times: a start on a small island or in a corner has no far tiles.
        self.ranged(rng, &range, LONG_START_ATTEMPTS, true, stats)
    }

    /// A short ranged episode. Some start moving along the route, some at
    /// rest facing away from it; if the turned hull doesn't fit, the start
    /// keeps its random heading at rest.
    fn arrival(
        &self,
        rng: &mut Pcg32,
        p: &ArrivalScenario,
        stats: &mut SampleStats,
    ) -> Option<Sampled> {
        let range = TargetConfig {
            min_distance: p.min_distance,
            max_distance: p.max_distance,
            max_tortuosity: p.max_tortuosity,
            ..TargetConfig::default()
        };
        let mut found = self.ranged(rng, &range, START_ATTEMPTS, false, stats)?;
        let roll = rng.gen::<f64>();
        if roll >= p.moving_fraction + p.backward_fraction {
            return Some(found);
        }
        let backward = roll >= p.moving_fraction;
        let start = found.start;
        let ahead = found.field.descend(start.x, start.y, ARRIVAL_LOOKAHEAD);
        let point = ahead
            .last()
            .copied()
            .unwrap_or([found.target.x, found.target.y]);
        // Forward is (sin, -cos) of 2*pi*orientation; jitter about +-7 degrees.
        let toward = (point[0] - start.x).atan2(start.y - point[1]) / TAU
            + rng.gen_range(-0.02..0.02)
            + if backward { 0.5 } else { 0.0 };
        let speed = rng.gen::<f64>() * p.max_start_speed;
        let pose = Pose {
            orientation: physics::quantize_orientation(toward.rem_euclid(1.0)),
            ..start
        };
        if pose_is_clear(self.world, self.data, self.vehicle, pose) {
            found.start = pose;
            found.start_speed = if backward { 0.0 } else { speed };
        }
        Some(found)
    }

    fn forest(
        &self,
        rng: &mut Pcg32,
        p: &ForestScenario,
        stats: &mut SampleStats,
    ) -> Option<Sampled> {
        let range = TargetConfig {
            min_distance: p.min_distance,
            max_distance: p.max_distance,
            max_tortuosity: p.max_tortuosity,
            ..TargetConfig::default()
        };
        let density = |x, y| tree_density(self.raster, x, y, p.density_radius);
        let dense_route = |route: &[[f64; 2]]| {
            let dense = route
                .iter()
                .filter(|t| density(t[0].floor() as i32, t[1].floor() as i32) >= p.route_density)
                .count();
            !route.is_empty() && dense as f64 >= p.route_fraction * route.len() as f64
        };
        for _ in 0..RARE_START_ATTEMPTS {
            let start = self.start(rng, START_DRAWS, stats, |x, y| {
                density(x, y) >= p.start_density
            })?;
            let field = self.start_field(start, range.max_distance, self.params);
            // Dense targets make dense routes likely; the route checks decide.
            let mut candidates = candidates(&field, start, &range, |x, y| {
                density(x, y) >= p.route_density
            });
            let mut tries = 0;
            for i in 0..candidates.len().min(PRECHECKS) {
                if tries == TARGET_ATTEMPTS {
                    break;
                }
                let (x, y, _) = shuffle_step(rng, &mut candidates, i);
                let point = tile_center(x, y);
                if !dense_route(&field.descend(point.x, point.y, usize::MAX)) {
                    continue;
                }
                tries += 1;
                let Some(found) = self.confirm(start, point, Some(&range), stats) else {
                    continue;
                };
                if dense_route(&found.field.descend(start.x, start.y, usize::MAX)) {
                    stats.targets += 1;
                    return Some(found);
                }
            }
        }
        None
    }

    fn reverse(
        &self,
        rng: &mut Pcg32,
        p: &ReverseScenario,
        stats: &mut SampleStats,
    ) -> Option<Sampled> {
        let range = TargetConfig {
            min_distance: p.min_distance,
            max_distance: p.max_distance,
            max_tortuosity: p.max_tortuosity,
            ..TargetConfig::default()
        };
        let lookahead = p.route_lookahead;
        for _ in 0..RARE_START_ATTEMPTS {
            let start = self.reverse_start(rng, p, stats)?;
            let field = self.start_field(start, range.max_distance, self.params);
            let mut candidates = candidates(&field, start, &range, |_, _| true);
            let mut tries = 0;
            for i in 0..candidates.len().min(PRECHECKS) {
                if tries == TARGET_ATTEMPTS {
                    break;
                }
                let (x, y, _) = shuffle_step(rng, &mut candidates, i);
                let point = tile_center(x, y);
                // The start field's route from the target ends at the start;
                // its last tiles are where the route leaves the start.
                let route = field.descend(point.x, point.y, usize::MAX);
                let leave = route.len().saturating_sub(lookahead + 1);
                if route_angle(start, route.get(leave)) < p.min_route_angle {
                    continue;
                }
                tries += 1;
                let Some(found) = self.confirm(start, point, Some(&range), stats) else {
                    continue;
                };
                let ahead = found.field.descend(start.x, start.y, lookahead);
                if route_angle(start, ahead.last()) >= p.min_route_angle {
                    stats.targets += 1;
                    return Some(found);
                }
            }
        }
        None
    }

    /// A pose facing a blocking obstacle with the hull front `min_gap` to
    /// `max_gap` tiles from it. The tank drives through trees, so for it trees
    /// don't count as obstacles. The start may lie in a nest zone.
    fn reverse_start(
        &self,
        rng: &mut Pcg32,
        p: &ReverseScenario,
        stats: &mut SampleStats,
    ) -> Option<Pose> {
        let prototype = &self.data.vehicles[self.vehicle];
        let b = prototype.collision_box;
        let half_length = b[0][1].abs().max(b[1][1].abs());
        let reach = (half_length + p.max_gap + 1.5).ceil() as i32;
        let blocking = CLIFF | NEST | ROCK | WATER | if self.params.trees_block { TREE } else { 0 };
        let mut obstacles = Vec::new();
        for _ in 0..START_DRAWS {
            stats.start_draws += 1;
            let (tx, ty, pose) = self.area.draw(rng);
            if self.raster.mask_at(tx, ty) & blocking != 0 {
                continue;
            }
            obstacles.clear();
            for y in ty - reach..=ty + reach {
                for x in tx - reach..=tx + reach {
                    if self.raster.mask_at(x, y) & blocking != 0 {
                        obstacles.push((x, y));
                    }
                }
            }
            if obstacles.is_empty() {
                continue;
            }
            let (ox, oy) = obstacles[rng.gen_range(0..obstacles.len())];
            let (dx, dy) = (ox as f64 + 0.5 - pose.x, oy as f64 + 0.5 - pose.y);
            // Forward is (sin, -cos) of 2*pi*orientation; jitter about +-7 degrees.
            let toward = dx.atan2(-dy) / TAU + rng.gen_range(-0.02..0.02);
            let pose = Pose {
                orientation: physics::quantize_orientation(toward.rem_euclid(1.0)),
                ..pose
            };
            if !pose_is_clear(self.world, self.data, self.vehicle, pose) {
                continue;
            }
            let gap = forward_gap(
                self.world,
                self.data,
                self.vehicle,
                pose,
                p.max_gap,
                self.params.trees_block,
            );
            if gap.is_some_and(|g| g >= p.min_gap) {
                stats.starts += 1;
                return Some(pose);
            }
        }
        None
    }

    fn nest(&self, rng: &mut Pcg32, p: &NestScenario, stats: &mut SampleStats) -> Option<Sampled> {
        let nests = &self.raster.nests;
        if nests.is_empty() {
            return None;
        }
        let free = self.params.without_zones();
        let range = TargetConfig {
            min_distance: p.min_distance,
            max_distance: p.max_distance,
            max_tortuosity: f64::INFINITY,
            ..TargetConfig::default()
        };
        for _ in 0..RARE_START_ATTEMPTS {
            let (start, nest) = self.nest_start(rng, stats)?;
            let e = &self.world.entities[nest];
            let radius = e.penalty_radius.unwrap_or(0.0);
            let field = self.start_field(start, p.max_distance, free);
            // The straight line to the target crosses this nest's zone.
            let mut candidates = candidates(&field, start, &range, |x, y| {
                let point = tile_center(x, y);
                self.raster.mask_at(x, y) & NEST_ZONE == 0
                    && segment_distance([start.x, start.y], [point.x, point.y], [e.x, e.y]) < radius
            });
            let mut tries = 0;
            for i in 0..candidates.len().min(PRECHECKS) {
                if tries == TARGET_ATTEMPTS {
                    break;
                }
                let (x, y, free_cost) = shuffle_step(rng, &mut candidates, i);
                let point = tile_center(x, y);
                let free_route = field.route(point.x, point.y, [start.x, start.y]);
                if zone_tiles(self.raster, &free_route) < p.min_zone_tiles {
                    continue;
                }
                tries += 1;
                let Some(found) = self.confirm(start, point, None, stats) else {
                    continue;
                };
                let skirt = zone_skirting_field(
                    self.raster,
                    self.data,
                    self.vehicle,
                    self.rules,
                    [start.x, start.y],
                    [point.x, point.y],
                );
                if skirt.sample(start.x, start.y) < free_cost + NEST_MIN_DETOUR {
                    continue;
                }
                // The episode's own field must lead around the zone. With
                // `nest_zone_cost` 0 (to evaluate zone-blind fields) it can't,
                // so the skirting route stands in for the detour bound.
                let avoiding = if self.params.zone_cost > 0.0 {
                    &found.field
                } else {
                    &skirt
                };
                let route = avoiding.route(start.x, start.y, [point.x, point.y]);
                if zone_tiles(self.raster, &route) == 0
                    && polyline_length(&route) <= p.max_detour * polyline_length(&free_route)
                {
                    stats.targets += 1;
                    return Some(found);
                }
            }
        }
        None
    }

    /// A valid start a little outside a random nest's zone, and that nest.
    fn nest_start(&self, rng: &mut Pcg32, stats: &mut SampleStats) -> Option<(Pose, usize)> {
        let nests = &self.raster.nests;
        for _ in 0..START_DRAWS {
            stats.start_draws += 1;
            let nest = nests[rng.gen_range(0..nests.len())];
            let e = &self.world.entities[nest];
            let distance = e.penalty_radius.unwrap_or(0.0)
                + rng.gen_range(NEST_START_OFFSET.0..NEST_START_OFFSET.1);
            let angle = rng.gen::<f64>() * TAU;
            let grid = |v: f64| (v * 256.0).floor() / 256.0;
            let pose = Pose {
                x: grid(e.x + distance * angle.cos()),
                y: grid(e.y + distance * angle.sin()),
                orientation: physics::quantize_orientation(rng.gen::<f64>()),
            };
            let (tx, ty) = (pose.x.floor() as i32, pose.y.floor() as i32);
            if self.area.contains(tx, ty)
                && valid_start(
                    self.world,
                    self.raster,
                    self.data,
                    self.vehicle,
                    self.params,
                    tx,
                    ty,
                    pose,
                )
            {
                stats.starts += 1;
                return Some((pose, nest));
            }
        }
        None
    }
}

/// The episode field toward `target` with nest zones (no margin) priced so
/// that routes keep out of them when they can.
pub fn zone_skirting_field(
    raster: &MapRaster,
    data: &Prototypes,
    vehicle: usize,
    rules: &EpisodeRules,
    start: [f64; 2],
    target: [f64; 2],
) -> DistanceField {
    let params = FieldParams {
        zone_cost: SKIRT_ZONE_COST,
        zone_margin: 0,
        ..FieldParams::new(&data.vehicles[vehicle], &rules.distance_field)
    };
    let region = Region::around(&[start, target], rules.distance_field.margin, raster);
    DistanceField::compute(raster, region, params, target, f64::INFINITY)
}

/// Tiles whose value in `field` fits the range and tortuosity and pass
/// `keep`: `(x, y, value)`.
fn candidates(
    field: &DistanceField,
    start: Pose,
    range: &TargetConfig,
    keep: impl Fn(i32, i32) -> bool,
) -> Vec<(i32, i32, f64)> {
    let region = field.region;
    let mut out = Vec::new();
    for (i, &value) in field.values.iter().enumerate() {
        let value = value as f64;
        if !(range.min_distance..=range.max_distance).contains(&value) {
            continue;
        }
        let x = region.x0 + (i % region.width()) as i32;
        let y = region.y0 + (i / region.width()) as i32;
        let straight = (x as f64 + 0.5 - start.x).hypot(y as f64 + 0.5 - start.y);
        if straight > 0.0 && value <= range.max_tortuosity * straight && keep(x, y) {
            out.push((x, y, value));
        }
    }
    out
}

/// Step `i` of a Fisher-Yates shuffle: entry `i` becomes a uniform pick
/// among the entries not yet visited.
fn shuffle_step<T: Copy>(rng: &mut Pcg32, items: &mut [T], i: usize) -> T {
    let j = rng.gen_range(i..items.len());
    items.swap(i, j);
    items[i]
}

fn tile_center(x: i32, y: i32) -> Point {
    Point {
        x: x as f64 + 0.5,
        y: y as f64 + 0.5,
    }
}

/// Fraction of tree tiles in the square of half-size `radius` around a tile.
pub fn tree_density(raster: &MapRaster, x: i32, y: i32, radius: i32) -> f64 {
    let side = (2 * radius + 1) as f64;
    if let (Some(counts), Some(i)) = (raster.tree_counts(radius), raster.index(x, y)) {
        return f64::from(counts[i]) / (side * side);
    }
    let mut trees = 0;
    for ny in y - radius..=y + radius {
        for nx in x - radius..=x + radius {
            trees += (raster.mask_at(nx, ny) & TREE != 0) as u32;
        }
    }
    let side = (2 * radius + 1) as f64;
    trees as f64 / (side * side)
}

/// Route points whose tile lies in a nest zone.
pub fn zone_tiles(raster: &MapRaster, route: &[[f64; 2]]) -> usize {
    route
        .iter()
        .filter(|p| raster.mask_at(p[0].floor() as i32, p[1].floor() as i32) & NEST_ZONE != 0)
        .count()
}

/// Angle in degrees between the heading of `pose` and the direction to
/// `point`; 0 without a point.
pub fn route_angle(pose: Pose, point: Option<&[f64; 2]>) -> f64 {
    let Some(p) = point else {
        return 0.0;
    };
    let (sin, cos) = (pose.orientation * TAU).sin_cos();
    let (dx, dy) = (p[0] - pose.x, p[1] - pose.y);
    let length = dx.hypot(dy);
    if length < 1e-9 {
        return 0.0;
    }
    ((dx * sin - dy * cos) / length)
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

/// Free distance in tiles the hull can move straight ahead before touching
/// a blocking obstacle or water (to `GAP_STEP` resolution); `None` if
/// nothing blocks within `limit`. Trees block only when `trees_block`.
pub fn forward_gap(
    world: &World,
    data: &Prototypes,
    vehicle: usize,
    pose: Pose,
    limit: f64,
    trees_block: bool,
) -> Option<f64> {
    let collision_box = data.vehicles[vehicle].collision_box;
    let (sin, cos) = (pose.orientation * TAU).sin_cos();
    let steps = (limit / GAP_STEP).ceil() as usize + 1;
    (1..=steps).find_map(|k| {
        let s = k as f64 * GAP_STEP;
        let shape = Obb::new(
            pose.x + s * sin,
            pose.y - s * cos,
            collision_box,
            pose.orientation,
        );
        let blocked = world.touches_water(shape, data)
            || world.candidates(shape.aabb()).into_iter().any(|i| {
                let e = &world.entities[i];
                (trees_block || e.kind != "tree") && shape.intersects(e.shape())
            });
        blocked.then_some(s - GAP_STEP)
    })
}

/// Distance from `p` to the segment `a`-`b`.
fn segment_distance(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length2 = dx * dx + dy * dy;
    let t = if length2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a[0] + t * dx - p[0]).hypot(a[1] + t * dy - p[1])
}

/// The standard start rules: open tile and dilation neighborhood,
/// collision-free hull, outside nest zones.
#[allow(clippy::too_many_arguments)]
fn valid_start(
    world: &World,
    raster: &MapRaster,
    data: &Prototypes,
    vehicle: usize,
    params: FieldParams,
    tx: i32,
    ty: i32,
    pose: Pose,
) -> bool {
    tile_is_open(raster, params, tx, ty)
        && pose_is_clear(world, data, vehicle, pose)
        && !in_nest_zone(world, raster, pose.x, pose.y)
}

/// Tiles a start may lie on: the map minus `START_MARGIN`.
struct StartArea {
    x: (i32, i32),
    y: (i32, i32),
}

impl StartArea {
    fn new(raster: &MapRaster) -> Self {
        let margin_x = START_MARGIN.min(raster.width as i32 / 2 - 1);
        let margin_y = START_MARGIN.min(raster.height as i32 / 2 - 1);
        Self {
            x: (
                raster.x_min + margin_x,
                raster.x_min + raster.width as i32 - margin_x,
            ),
            y: (
                raster.y_min + margin_y,
                raster.y_min + raster.height as i32 - margin_y,
            ),
        }
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        (self.x.0..self.x.1).contains(&x) && (self.y.0..self.y.1).contains(&y)
    }

    /// A random pose on Factorio's 1/256-tile grid with a quantized heading,
    /// and its tile.
    fn draw(&self, rng: &mut Pcg32) -> (i32, i32, Pose) {
        let tx = rng.gen_range(self.x.0..self.x.1);
        let ty = rng.gen_range(self.y.0..self.y.1);
        let pose = Pose {
            x: tx as f64 + rng.gen_range(0..256) as f64 / 256.0,
            y: ty as f64 + rng.gen_range(0..256) as f64 / 256.0,
            orientation: physics::quantize_orientation(rng.gen::<f64>()),
        };
        (tx, ty, pose)
    }
}

/// Whether the tile and its dilation neighborhood are free for this vehicle.
fn tile_is_open(raster: &MapRaster, params: FieldParams, x: i32, y: i32) -> bool {
    let blocking = CLIFF | NEST | ROCK | WATER | if params.trees_block { TREE } else { 0 };
    let d = params.dilation;
    (y - d..=y + d).all(|ny| (x - d..=x + d).all(|nx| raster.mask_at(nx, ny) & blocking == 0))
}
