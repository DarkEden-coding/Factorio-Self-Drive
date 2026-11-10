//! Scenario sampler and the per-episode counters (docs/rl.md, "Scenarios").
use fsim_core::{
    baseline,
    distance_field::{polyline_length, DistanceField, FieldParams, Region},
    map_format,
    policy_obs::{self, COARSE_LEN, LOCAL_LEN, VECTOR_LEN},
    raster::{MapRaster, RasterPatches},
    rl_config::{
        ArrivalScenario, EpisodeRules, FieldConfig, ForestScenario, ImpactScenario, LongScenario,
        Mode, NestScenario, RecoverScenario, ReverseScenario, RlConfig, Scenario, StandardScenario,
        TargetConfig, VehicleChoice, SCENARIOS,
    },
    rl_episode::{self, encode_action, Ctx, Episode, EpisodeSetup, Point, Pose},
    sampler::{self, SampleStats},
    vec_env::{Actions, MapData, ObsBatch, OwnedActions, StepInfo, VecEnv},
    vehicle::Vehicles,
    world::World,
    Prototypes,
};
use std::sync::Arc;

const REAL_MAP: &[u8] = include_bytes!("fixtures/nauvis-origin.fsmap");
const NEST_AT: [f64; 2] = [0.0, 22.0];

fn data() -> Arc<Prototypes> {
    Arc::new(Prototypes::from_json(include_str!("../../../data/prototypes.json")).unwrap())
}

/// The fixture map has almost no trees near its center and no nests, so add
/// a tree lattice in the north half and a spawner in the south.
fn scenario_map(data: &Prototypes) -> Arc<MapData> {
    let mut world = map_format::load_map(REAL_MAP, data).unwrap().0;
    for i in 0..24 {
        for j in 0..12 {
            let (x, y) = (-58.0 + 5.0 * i as f64, -60.0 + 4.5 * j as f64);
            world.spawn_entity(data, "tree-02-red", x + (j % 2) as f64 * 2.5, y);
        }
    }
    world.spawn_entity(data, "biter-spawner", NEST_AT[0], NEST_AT[1]);
    let raster = MapRaster::build(&world, data);
    Arc::new(MapData {
        pool_index: 0,
        pool: 0,
        template: world,
        raster,
    })
}

/// Scenario parameters sized for the 128 x 128 fixture.
fn fixture_scenarios() -> [Scenario; 8] {
    [
        Scenario::Standard(StandardScenario::default()),
        Scenario::Long(LongScenario {
            min_distance: 40.0,
            max_distance: 90.0,
            ..LongScenario::default()
        }),
        Scenario::Forest(ForestScenario {
            start_density: 0.05,
            route_density: 0.05,
            max_distance: 60.0,
            ..ForestScenario::default()
        }),
        Scenario::Reverse(ReverseScenario::default()),
        Scenario::Nest(NestScenario::default()),
        Scenario::Arrival(ArrivalScenario::default()),
        Scenario::Impact(ImpactScenario::default()),
        Scenario::Recover(RecoverScenario::default()),
    ]
}

fn config(vehicle: &str, scenarios: Vec<Scenario>) -> RlConfig {
    RlConfig {
        mode: Mode::Reactive,
        vehicle_mix: vec![VehicleChoice {
            vehicle: vehicle.into(),
            fuel: "coal".into(),
            weight: 1.0,
        }],
        target: TargetConfig {
            min_distance: 12.0,
            max_distance: 40.0,
            max_tortuosity: 1.5,
            radius: 3.0,
        },
        scenarios,
        ..RlConfig::default()
    }
}

struct Buffers {
    local: Vec<u8>,
    coarse: Vec<u8>,
    vector: Vec<f32>,
    finals: (Vec<u8>, Vec<u8>, Vec<f32>),
}

impl Buffers {
    fn new(n: usize) -> Self {
        Self {
            local: vec![0; n * LOCAL_LEN],
            coarse: vec![0; n * COARSE_LEN],
            vector: vec![0.0; n * VECTOR_LEN],
            finals: (
                vec![0; n * LOCAL_LEN],
                vec![0; n * COARSE_LEN],
                vec![0.0; n * VECTOR_LEN],
            ),
        }
    }
    fn reset(&mut self, env: &mut VecEnv) {
        env.reset(ObsBatch {
            local: &mut self.local,
            coarse: &mut self.coarse,
            vector: &mut self.vector,
            student: &mut [],
        })
        .unwrap();
    }
    fn step(&mut self, env: &mut VecEnv, actions: &[i64]) -> Vec<StepInfo> {
        let (l, c, v) = &mut self.finals;
        env.step(
            Actions::Reactive(actions),
            ObsBatch {
                local: &mut self.local,
                coarse: &mut self.coarse,
                vector: &mut self.vector,
                student: &mut [],
            },
            ObsBatch {
                local: l,
                coarse: c,
                vector: v,
                student: &mut [],
            },
        )
        .unwrap()
    }
}

/// Zone tiles on the field's route from `start` to `target`.
fn route_zone_tiles(
    raster: &MapRaster,
    field: &DistanceField,
    start: Pose,
    target: Point,
) -> usize {
    sampler::zone_tiles(raster, &field.route(start.x, start.y, [target.x, target.y]))
}

#[test]
fn every_scenario_kind_samples_valid_episodes_for_car_and_tank() {
    let data = data();
    let map = scenario_map(&data);
    let (world, raster) = (&map.template, &map.raster);
    for vehicle in ["car", "tank"] {
        let index = data.vehicle_index(vehicle).unwrap();
        let tank = data.vehicles[index].tank_driving;
        for scenario in fixture_scenarios() {
            let c = config(vehicle, vec![scenario]);
            let mut env = VecEnv::with_maps(c, data.clone(), vec![map.clone()], 8, 3).unwrap();
            let mut b = Buffers::new(8);
            for _ in 0..2 {
                b.reset(&mut env);
                for i in 0..8 {
                    let what = format!("{vehicle} {} env {i}", scenario.name());
                    assert_eq!(env.scenario(i), scenario.index(), "{what} fell back");
                    let e = env.episode(i);
                    let (start, target) = (e.setup.start, e.setup.target);
                    assert!(
                        rl_episode::pose_is_clear(world, &data, index, start),
                        "{what}"
                    );
                    assert!(e.optimal_distance.is_finite(), "{what}");
                    let straight = (target.x - start.x).hypot(target.y - start.y);
                    assert!(e.optimal_length >= straight - 1.5, "{what}");
                    match scenario {
                        Scenario::Long(p) => assert!(
                            (p.min_distance..=p.max_distance).contains(&e.optimal_distance),
                            "{what}"
                        ),
                        Scenario::Forest(p) => assert!(
                            sampler::tree_density(
                                raster,
                                start.x.floor() as i32,
                                start.y.floor() as i32,
                                p.density_radius
                            ) >= p.start_density,
                            "{what}"
                        ),
                        Scenario::Reverse(p) => {
                            let gap =
                                sampler::forward_gap(world, &data, index, start, p.max_gap, !tank)
                                    .unwrap_or_else(|| panic!("{what}: nothing ahead"));
                            assert!((p.min_gap..=p.max_gap).contains(&gap), "{what} gap {gap}");
                            let ahead = e.field.descend(start.x, start.y, p.route_lookahead);
                            assert!(
                                sampler::route_angle(start, ahead.last()) >= p.min_route_angle,
                                "{what}"
                            );
                        }
                        Scenario::Nest(_) => {
                            assert!(!rl_episode::in_nest_zone(world, raster, start.x, start.y));
                            assert_eq!(route_zone_tiles(raster, &e.field, start, target), 0);
                            let mut rules = EpisodeRules::default();
                            rules.distance_field.nest_zone_cost = 0.0;
                            let free = rl_episode::episode_field(
                                raster,
                                &data,
                                index,
                                &rules,
                                [start.x, start.y],
                                [target.x, target.y],
                            );
                            // Staying out of the zone is longer, so zone-free routes cross it.
                            let skirt = sampler::zone_skirting_field(
                                raster,
                                &data,
                                index,
                                &rules,
                                [start.x, start.y],
                                [target.x, target.y],
                            );
                            assert_eq!(route_zone_tiles(raster, &skirt, start, target), 0);
                            assert!(
                                free.sample(start.x, start.y) + 1.0
                                    < skirt.sample(start.x, start.y),
                                "{what}"
                            );
                            assert!(route_zone_tiles(raster, &free, start, target) > 0, "{what}");
                        }
                        Scenario::Arrival(p) => {
                            assert!(
                                (p.min_distance..=p.max_distance).contains(&e.optimal_distance),
                                "{what}"
                            );
                            assert!(e.setup.start_speed <= p.max_start_speed, "{what}");
                            assert_eq!(env.vehicles(i).speed[0], e.setup.start_speed, "{what}");
                            if e.setup.start_speed > 0.0 {
                                let ahead = e.field.descend(start.x, start.y, 4);
                                assert!(sampler::route_angle(start, ahead.last()) < 20.0, "{what}");
                            }
                        }
                        Scenario::Impact(p) => {
                            // Straight at an obstacle within the gap range, moving.
                            let gap =
                                sampler::forward_gap(world, &data, index, start, p.max_gap, !tank)
                                    .unwrap_or_else(|| panic!("{what}: nothing ahead"));
                            assert!((p.min_gap..=p.max_gap).contains(&gap), "{what} gap {gap}");
                            assert!(e.setup.start_speed > 0.0, "{what}");
                            assert_eq!(env.vehicles(i).speed[0], e.setup.start_speed, "{what}");
                        }
                        Scenario::Recover(_) => {
                            // Touching an obstacle ahead or behind, at rest.
                            let behind = Pose {
                                orientation: (start.orientation + 0.5).rem_euclid(1.0),
                                ..start
                            };
                            let touching = [start, behind].into_iter().any(|pose| {
                                sampler::forward_gap(world, &data, index, pose, 0.2, !tank)
                                    .is_some()
                            });
                            assert!(touching, "{what}");
                            assert_eq!(e.setup.start_speed, 0.0, "{what}");
                        }
                        Scenario::Standard(_) => {}
                    }
                }
            }
            // The episodes run: drive the baseline for a while.
            for _ in 0..100 {
                let OwnedActions::Discrete(actions) = env.baseline_actions() else {
                    unreachable!()
                };
                b.step(&mut env, &actions);
            }
        }
    }
}

#[test]
fn reverse_routes_leave_backward_or_sideways_from_the_nose() {
    let data = data();
    let map = scenario_map(&data);
    let rules = EpisodeRules::default();
    let scenario = Scenario::Reverse(ReverseScenario::default());
    for vehicle in ["car", "tank"] {
        let index = data.vehicle_index(vehicle).unwrap();
        for seed in 0..20 {
            let s = sampler::sample_scenario(
                &map.template,
                &map.raster,
                &data,
                index,
                &rules,
                &scenario,
                &TargetConfig::default(),
                seed,
                &mut SampleStats::default(),
            )
            .unwrap();
            assert!(s.field.sample(s.start.x, s.start.y).is_finite());
            let route = s
                .field
                .route(s.start.x, s.start.y, [s.target.x, s.target.y]);
            assert!(route.len() >= 3);
            assert!(sampler::route_angle(s.start, route.get(8.min(route.len() - 2))) > 90.0);
        }
    }
}

/// A flat grass world with one spawner at the origin.
fn nest_world(data: &Prototypes) -> (World, MapRaster) {
    let mut world = World::empty_rect(192, 192, 1, data.tile_id("grass-1"));
    world.spawn_entity(data, "biter-spawner", 0.0, 0.0);
    let raster = MapRaster::build(&world, data);
    (world, raster)
}

#[test]
fn nest_zone_cost_routes_the_field_and_baseline_around_zones() {
    let data = data();
    let (mut world, raster) = nest_world(&data);
    let (start, target) = ([-50.5, 0.5], [50.5, 0.5]);
    let car = &data.vehicles[0];
    let region = Region::around(&[start, target], 48, &raster);
    let mut config = FieldConfig::default();
    assert!(config.nest_zone_cost > 0.0);
    let avoiding = DistanceField::compute(
        &raster,
        region,
        FieldParams::new(car, &config),
        target,
        f64::INFINITY,
    );
    config.nest_zone_cost = 0.0;
    let free = DistanceField::compute(
        &raster,
        region,
        FieldParams::new(car, &config),
        target,
        f64::INFINITY,
    );
    let free_route = free.route(start[0], start[1], target);
    let route = avoiding.route(start[0], start[1], target);
    assert!(sampler::zone_tiles(&raster, &free_route) > 20);
    assert_eq!(sampler::zone_tiles(&raster, &route), 0);
    assert!(polyline_length(&route) < 1.5 * polyline_length(&free_route));

    // The field value is a cost, `optimal_length` the route's length, and
    // the A* baseline drives around the zone.
    let mut vehicles = Vehicles::default();
    vehicles.push(&data, 0, 0);
    let mut patches = RasterPatches::default();
    let setup = EpisodeSetup {
        map: "nest".into(),
        vehicle: "car".into(),
        fuel: "coal".into(),
        start: Pose {
            x: start[0],
            y: start[1],
            orientation: 0.25,
        },
        target: Point {
            x: target[0],
            y: target[1],
        },
        radius: 3.0,
        start_speed: 0.0,
        seed: None,
    };
    macro_rules! ctx {
        () => {
            Ctx {
                data: &data,
                raster: &raster,
                world: &mut world,
                vehicles: &mut vehicles,
                patches: &mut patches,
            }
        };
    }
    let mut e = Episode::start(ctx!(), setup, EpisodeRules::default()).unwrap();
    assert!((e.optimal_length - polyline_length(&route)).abs() < 1e-9);
    assert!(e.optimal_length > 101.0);
    assert_eq!(
        sampler::zone_tiles(&raster, &baseline::full_path(&e, &vehicles)),
        0
    );
    while !e.status.done() {
        let a = baseline::action(&data, &e, &vehicles);
        e.decide(ctx!(), a).unwrap();
    }
    assert_eq!(e.status, rl_episode::Status::Success);
    assert_eq!(e.nest_zone_ticks, 0);
    assert_eq!(e.nest_zone_decisions, 0);
}

#[test]
fn episodes_count_collisions_by_kind_zone_time_and_reversing() {
    let data = data();
    let (mut world, raster) = nest_world(&data);
    let mut vehicles = Vehicles::default();
    vehicles.push(&data, 0, 0);
    let mut patches = RasterPatches::default();
    // Inside the zone, facing the spawner from 6 tiles south.
    let setup = EpisodeSetup {
        map: "nest".into(),
        vehicle: "car".into(),
        fuel: "coal".into(),
        start: Pose {
            x: 0.5,
            y: 6.0,
            orientation: 0.0,
        },
        target: Point { x: 0.5, y: 60.5 },
        radius: 3.0,
        start_speed: 0.0,
        seed: None,
    };
    macro_rules! ctx {
        () => {
            Ctx {
                data: &data,
                raster: &raster,
                world: &mut world,
                vehicles: &mut vehicles,
                patches: &mut patches,
            }
        };
    }
    let mut e = Episode::start(ctx!(), setup, EpisodeRules::default()).unwrap();
    for _ in 0..20 {
        e.decide(ctx!(), encode_action(1, 0)).unwrap();
    }
    assert!(e.collisions_by_kind[3] > 0, "{:?}", e.collisions_by_kind);
    assert_eq!(e.collisions_by_kind.iter().sum::<u64>(), e.collisions);
    assert_eq!(e.nest_zone_decisions, 20);
    assert_eq!(e.nest_zone_ticks, 80);
    assert!(!e.reversed());
    for _ in 0..20 {
        e.decide(ctx!(), encode_action(-1, 0)).unwrap();
    }
    assert!(e.reversed() && e.reverse_distance > 1.0);
    assert!(e.reverse_distance <= e.path_length + 1e-9);
}

#[test]
fn scenario_mixes_are_deterministic_and_follow_weights() {
    let data = data();
    let map = scenario_map(&data);
    let mut scenarios = fixture_scenarios().to_vec();
    if let Scenario::Standard(p) = &mut scenarios[0] {
        p.weight = 0.0;
    }
    let run = || {
        let c = config("car", scenarios.clone());
        let mut env = VecEnv::with_maps(c, data.clone(), vec![map.clone()], 32, 9).unwrap();
        let mut b = Buffers::new(32);
        b.reset(&mut env);
        let mut kinds = [0; SCENARIOS.len()];
        let mut trace = Vec::new();
        for i in 0..32 {
            kinds[env.scenario(i)] += 1;
        }
        for step in 0..150 {
            let actions: Vec<i64> = (0..32).map(|i| ((step * 7 + i * 3) % 9) as i64).collect();
            for (i, info) in b.step(&mut env, &actions).into_iter().enumerate() {
                if info.terminated || info.truncated {
                    kinds[env.scenario(i)] += 1;
                }
                trace.push(format!("{info:?}"));
            }
        }
        (trace, kinds)
    };
    let (trace, kinds) = run();
    assert_eq!(trace, run().0);
    assert_eq!(kinds[0], 0, "zero weight never picked: {kinds:?}");
    assert!(kinds[1..].iter().all(|k| *k > 0), "{kinds:?}");
}

#[test]
fn scenarios_parse_from_json_with_defaults_and_reject_bad_params() {
    let c = RlConfig::from_json(
        r#"{"scenarios": [{"kind": "standard", "weight": 0.5}, {"kind": "nest", "max_detour": 2.0}]}"#,
    )
    .unwrap();
    assert_eq!(
        c.scenarios[1],
        Scenario::Nest(NestScenario {
            max_detour: 2.0,
            ..NestScenario::default()
        })
    );
    for bad in [
        r#"{"scenarios": []}"#,
        r#"{"scenarios": [{"kind": "warp"}]}"#,
        r#"{"scenarios": [{"kind": "long", "max_distanse": 9}]}"#,
        r#"{"scenarios": [{"kind": "reverse", "min_gap": 2, "max_gap": 1}]}"#,
        r#"{"scenarios": [{"kind": "forest", "weight": 0}]}"#,
        r#"{"scenarios": [{"kind": "arrival", "moving_fraction": 0.8, "backward_fraction": 0.4}]}"#,
        r#"{"arrival": {"mode": "halt"}}"#,
        r#"{"arrival": {"mode": "stop", "stop_radius": 0}}"#,
    ] {
        assert!(RlConfig::from_json(bad).is_err(), "{bad}");
    }
    let spec = serde_json::to_value(policy_obs::spec(&data())).unwrap();
    assert_eq!(spec["scenarios"], serde_json::json!(SCENARIOS));
}

#[test]
fn spec_hash_is_unchanged_by_scenarios() {
    // Checkpoints from before the scenario sampler (reactive-1) keep loading.
    assert_eq!(policy_obs::spec(&data()).spec_hash, "16a3d4353d1658c1");
}

#[test]
fn nest_scenarios_sample_with_a_zone_blind_field() {
    // Evaluating policies on the field that ignored zones needs the same kind of episodes.
    let data = data();
    let map = scenario_map(&data);
    let mut c = config("car", vec![Scenario::Nest(NestScenario::default())]);
    c.distance_field.nest_zone_cost = 0.0;
    let mut env = VecEnv::with_maps(c, data.clone(), vec![map.clone()], 8, 5).unwrap();
    Buffers::new(8).reset(&mut env);
    let mut crossing = 0;
    for i in 0..8 {
        assert_eq!(env.scenario(i), 4);
        let e = env.episode(i);
        crossing +=
            (route_zone_tiles(&map.raster, &e.field, e.setup.start, e.setup.target) > 0) as usize;
    }
    assert!(
        crossing >= 6,
        "the zone-blind field mostly crosses: {crossing}/8"
    );
}

#[test]
fn cached_tree_density_matches_the_window_sum() {
    // The forest sampler reads tree density from a per-map table; it must equal summing the
    // window tile by tile everywhere, including where the window leaves the map.
    let data = data();
    let map = scenario_map(&data);
    let raster = &map.raster;
    let window = |x: i32, y: i32, r: i32| {
        let mut trees = 0;
        for ny in y - r..=y + r {
            for nx in x - r..=x + r {
                trees += (raster.mask_at(nx, ny) & fsim_core::raster::TREE != 0) as u32;
            }
        }
        trees as f64 / ((2 * r + 1) * (2 * r + 1)) as f64
    };
    let r = ForestScenario::default().density_radius;
    assert!(raster.tree_counts(r).is_some());
    let mut dense = 0;
    for y in raster.y_min - 2..raster.y_min + raster.height as i32 + 2 {
        for x in raster.x_min - 2..raster.x_min + raster.width as i32 + 2 {
            let d = sampler::tree_density(raster, x, y, r);
            assert_eq!(d, window(x, y, r), "tile {x},{y}");
            dense += usize::from(d >= 0.2);
        }
    }
    assert!(dense > 100, "fixture has too few dense tiles: {dense}");
    // Another radius is not cached and falls back to the sum.
    assert!(raster.tree_counts(r - 1).is_none());
    assert_eq!(
        sampler::tree_density(raster, 0, -40, r - 1),
        window(0, -40, r - 1)
    );
}

#[test]
fn clearance_cost_routes_keep_room_from_obstacles() {
    // A rock beside a straight route: with clearance cost the route bends away from it.
    let data = data();
    let mut world = World::empty_rect(128, 128, 1, data.tile_id("grass-1"));
    world.spawn_entity(&data, "big-rock", 0.5, 2.5);
    let raster = MapRaster::build(&world, &data);
    let (start, target) = ([-40.5, 0.5], [40.5, 0.5]);
    let car = &data.vehicles[data.vehicle_index("car").unwrap()];
    let region = Region::around(&[start, target], 24, &raster);
    let mut config = FieldConfig::default();
    let route_near = |config: &FieldConfig| {
        let field = DistanceField::compute(
            &raster,
            region,
            FieldParams::new(car, config),
            target,
            f64::INFINITY,
        );
        let route = field.smooth_route(start[0], start[1], target);
        // Closest approach of the route to the rock center.
        route
            .windows(2)
            .flat_map(|s| {
                (0..=20).map(move |k| {
                    let t = k as f64 / 20.0;
                    [
                        s[0][0] + t * (s[1][0] - s[0][0]),
                        s[0][1] + t * (s[1][1] - s[0][1]),
                    ]
                })
            })
            .map(|p| (p[0] - 0.5).hypot(p[1] - 2.5))
            .fold(f64::INFINITY, f64::min)
    };
    let tight = route_near(&config);
    config.clearance_cost = 2.0;
    let roomy = route_near(&config);
    assert!(roomy >= tight + 0.9, "clearance {roomy} vs {tight}");
}
