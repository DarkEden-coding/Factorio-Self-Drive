use fsim_core::{
    map_format, observation,
    policy_obs::{self, ObsScratch, COARSE_LEN, LOCAL_LEN, VECTOR_LEN},
    raster::{friction_byte, MapRaster, RasterPatches},
    rl_config::{
        ArrivalMode, EpisodeRules, Mode, Observation, RlConfig, TargetConfig, VehicleChoice,
    },
    rl_episode::{self, encode_action, Ctx, Episode, EpisodeSetup, Point, Pose, Status},
    sampler,
    student_obs::{self, STUDENT_LEN},
    target::Target,
    vec_env::{Actions, MapData, ObsBatch, OwnedActions, StepInfo, VecEnv},
    vehicle::Vehicles,
    world::World,
    Prototypes,
};
use std::sync::Arc;

const REAL_MAP: &[u8] = include_bytes!("fixtures/nauvis-origin.fsmap");

fn data() -> Arc<Prototypes> {
    Arc::new(Prototypes::from_json(include_str!("../../../data/prototypes.json")).unwrap())
}

fn fixture_map(data: &Prototypes) -> Arc<MapData> {
    let world = map_format::load_map(REAL_MAP, data).unwrap().0;
    let raster = MapRaster::build(&world, data);
    Arc::new(MapData {
        pool_index: 0,
        pool: 0,
        template: world,
        raster,
    })
}

fn config(mode: Mode, vehicle: &str) -> RlConfig {
    RlConfig {
        mode,
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
        ..RlConfig::default()
    }
}

fn env(config: RlConfig, n: usize, seed: u64) -> VecEnv {
    let data = data();
    let map = fixture_map(&data);
    VecEnv::with_maps(config, data, vec![map], n, seed).unwrap()
}

struct Buffers {
    obs: [Vec<u8>; 2],
    vector: Vec<f32>,
    finals: [Vec<u8>; 2],
    final_vector: Vec<f32>,
}

impl Buffers {
    fn new(n: usize) -> Self {
        Self {
            obs: [vec![0; n * LOCAL_LEN], vec![0; n * COARSE_LEN]],
            vector: vec![0.0; n * VECTOR_LEN],
            finals: [vec![0; n * LOCAL_LEN], vec![0; n * COARSE_LEN]],
            final_vector: vec![0.0; n * VECTOR_LEN],
        }
    }
    fn reset(&mut self, env: &mut VecEnv) {
        let [local, coarse] = &mut self.obs;
        env.reset(ObsBatch {
            local,
            coarse,
            vector: &mut self.vector,
            student: &mut [],
        })
        .unwrap();
    }
    fn step(&mut self, env: &mut VecEnv, actions: Actions<'_>) -> Vec<StepInfo> {
        let [local, coarse] = &mut self.obs;
        let [final_local, final_coarse] = &mut self.finals;
        env.step(
            actions,
            ObsBatch {
                local,
                coarse,
                vector: &mut self.vector,
                student: &mut [],
            },
            ObsBatch {
                local: final_local,
                coarse: final_coarse,
                vector: &mut self.final_vector,
                student: &mut [],
            },
        )
        .unwrap()
    }
    fn step_baseline(&mut self, env: &mut VecEnv) -> Vec<StepInfo> {
        match env.baseline_actions() {
            OwnedActions::Discrete(d) => {
                let actions = if env.mode() == Mode::Reactive {
                    Actions::Reactive(&d)
                } else {
                    Actions::DirectPlan(&d)
                };
                self.step(env, actions)
            }
            OwnedActions::Waypoints(w) => self.step(env, Actions::PathPlanner(&w)),
        }
    }
}

/// Deterministic pseudo-random action stream.
fn scripted(step: usize, i: usize) -> i64 {
    ((step * 7 + i * 3 + step / 5) % 9) as i64
}

#[test]
fn episodes_are_deterministic_given_seed() {
    let run = || {
        let mut env = env(config(Mode::Reactive, "car"), 4, 99);
        let mut b = Buffers::new(4);
        b.reset(&mut env);
        let mut trace = Vec::new();
        for step in 0..400 {
            let actions: Vec<i64> = (0..4).map(|i| scripted(step, i)).collect();
            for info in b.step(&mut env, Actions::Reactive(&actions)) {
                trace.push(format!("{info:?}"));
            }
        }
        (trace, b.obs, b.vector)
    };
    assert_eq!(run(), run());
}

#[test]
fn sampled_targets_are_reachable_within_distance_and_tortuosity() {
    for vehicle in ["car", "tank"] {
        let mut c = config(Mode::Reactive, vehicle);
        c.target.max_tortuosity = 1.2;
        let mut env = env(c.clone(), 16, 5);
        let mut b = Buffers::new(16);
        for _ in 0..4 {
            b.reset(&mut env);
            for i in 0..16 {
                let e = env.episode(i);
                let s = &e.setup;
                let straight = (s.target.x - s.start.x).hypot(s.target.y - s.start.y);
                assert!(e.optimal_distance.is_finite());
                assert!(
                    (c.target.min_distance..=c.target.max_distance).contains(&e.optimal_distance)
                );
                assert!(e.optimal_distance <= 1.2 * straight + 1e-9);
                assert!(e.optimal_distance >= straight - 1.0);
            }
        }
    }
}

#[test]
fn path_length_reward_charges_actual_forward_and_reverse_travel() {
    let mut c = config(Mode::Reactive, "car");
    c.reward.path_length = -0.02;
    let term = fsim_core::reward::TERMS
        .iter()
        .position(|t| *t == "path_length")
        .unwrap();
    for direction in [-1, 1] {
        let mut e = env(c.clone(), 1, 17);
        let mut b = Buffers::new(1);
        b.reset(&mut e);
        let mut previous = 0.0;
        let mut moved = false;
        for action in [4, encode_action(direction, 0), encode_action(direction, 0)] {
            let info = b.step(&mut e, Actions::Reactive(&[action as i64]));
            let traveled = info[0].path_length as f64 - previous;
            assert!(traveled >= 0.0);
            assert!((info[0].terms[term] as f64 + 0.02 * traveled).abs() < 1e-6);
            moved |= if direction < 0 {
                info[0].reverse_distance > 0.0
            } else {
                info[0].speed > 0.0
            };
            previous = info[0].path_length as f64;
            assert!(!info[0].terminated && !info[0].truncated);
        }
        assert!(previous > 0.0 && moved);
    }
}

#[test]
fn steer_change_reward_charges_steps_between_consecutive_decisions() {
    let mut c = config(Mode::Reactive, "car");
    c.reward.steer_change = -0.1;
    let term = fsim_core::reward::TERMS
        .iter()
        .position(|t| *t == "steer_change")
        .unwrap();
    let mut e = env(c, 1, 17);
    let mut b = Buffers::new(1);
    b.reset(&mut e);
    // The first decision has no predecessor; then left->left, left->right, right->straight.
    let steps = [(-1, 0.0), (-1, 0.0), (1, -0.2), (0, -0.1)];
    for (steer, expected) in steps {
        let info = b.step(&mut e, Actions::Reactive(&[encode_action(1, steer) as i64]));
        assert!((info[0].terms[term] - expected).abs() < 1e-6);
    }
}

#[test]
fn partial_steering_turns_in_fine_steps_and_full_strength_matches_classic_steering() {
    // Heading change of the last decision, after four identical straight accelerations.
    let turn = |levels: u32, steer: i32| {
        let mut c = config(Mode::Reactive, "car");
        c.steer_levels = levels;
        let mut e = env(c, 1, 17);
        let mut b = Buffers::new(1);
        b.reset(&mut e);
        let straight = fsim_core::rl_episode::widen_action(encode_action(1, 0), levels);
        let mut last = 0.0;
        for _ in 0..8 {
            last = b.step(&mut e, Actions::Reactive(&[straight as i64]))[0].orientation as f64;
        }
        let action = (2 * levels as i32 + 1) * 2 + steer + levels as i32;
        let after = b.step(&mut e, Actions::Reactive(&[action as i64]))[0].orientation as f64;
        (after - last + 0.5).rem_euclid(1.0) - 0.5
    };
    let classic = turn(1, 1);
    let (full, half, quarter) = (turn(4, 4), turn(4, 2), turn(4, 1));
    assert!(classic > 0.0 && (full - classic).abs() < 1e-6);
    assert!(0.0 < quarter && quarter < half && half < full);
    assert!((half / full - 0.5).abs() < 0.1 && (quarter / full - 0.25).abs() < 0.1);
    assert!(turn(4, -2) < 0.0);
}

#[test]
fn heading_error_reward_is_a_bounded_cost_only_while_driving_forward() {
    let mut c = config(Mode::Reactive, "car");
    c.reward.heading_error = -1.0;
    let term = fsim_core::reward::TERMS
        .iter()
        .position(|t| *t == "heading_error")
        .unwrap();
    let mut e = env(c, 1, 17);
    let mut b = Buffers::new(1);
    b.reset(&mut e);
    let mut costs = Vec::new();
    for _ in 0..24 {
        let info = b.step(&mut e, Actions::Reactive(&[encode_action(1, 0) as i64]));
        costs.push((info[0].speed, info[0].terms[term]));
    }
    assert!(costs
        .iter()
        .all(|(_, c)| (-std::f32::consts::PI..=0.0).contains(c)));
    assert!(costs.iter().any(|(speed, c)| *speed > 0.02 && *c < 0.0));
}

#[test]
fn reward_terms_sum_to_reward_and_macro_steps_discount() {
    let mut c = config(Mode::DirectPlan, "car");
    c.gamma = 0.9;
    c.reward.path_length = -0.02;
    let mut plan_env = env(c.clone(), 2, 17);
    c.mode = Mode::Reactive;
    let mut reactive_env = env(c, 2, 17);
    let (mut pb, mut rb) = (Buffers::new(2), Buffers::new(2));
    pb.reset(&mut plan_env);
    rb.reset(&mut reactive_env);
    assert_eq!(pb.vector, rb.vector);
    let horizon = plan_env.rules().direct_plan.horizon;
    for step in 0..40 {
        let plan: Vec<i64> = (0..2 * horizon).map(|k| scripted(step, k)).collect();
        let infos = pb.step(&mut plan_env, Actions::DirectPlan(&plan));
        let mut expected = [0.0f64; 2];
        let mut scale = [1.0f64; 2];
        let mut done = [false; 2];
        let mut executed = [0; 2];
        for k in 0..4 {
            let actions: Vec<i64> = (0..2)
                .map(|i| if done[i] { 4 } else { plan[i * horizon + k] })
                .collect();
            let r = rb.step(&mut reactive_env, Actions::Reactive(&actions));
            for i in 0..2 {
                if !done[i] {
                    let sum: f32 = r[i].terms.iter().sum();
                    assert!((sum - r[i].reward).abs() < 1e-5);
                    assert_eq!(r[i].discount, 0.9);
                    expected[i] += scale[i] * r[i].reward as f64;
                    scale[i] *= 0.9;
                    executed[i] += 1;
                    done[i] = r[i].terminated || r[i].truncated;
                }
            }
        }
        for i in 0..2 {
            let info = &infos[i];
            let sum: f32 = info.terms.iter().sum();
            assert!((sum - info.reward).abs() < 1e-5);
            assert_eq!(info.decisions, executed[i]);
            assert!((info.reward as f64 - expected[i]).abs() < 1e-4);
            assert!((info.discount as f64 - 0.9f64.powi(executed[i] as i32)).abs() < 1e-6);
        }
        if done.iter().any(|d| *d) {
            // Paired envs desynchronize once one finishes inside a macro step.
            break;
        }
    }
}

/// A flat grass world with one rock at the origin.
fn rock_world(data: &Prototypes) -> (World, MapRaster) {
    let mut world = World::empty_rect(128, 128, 1, data.tile_id("grass-1"));
    world.spawn_entity(data, "big-rock", 0.0, 0.0);
    let raster = MapRaster::build(&world, data);
    (world, raster)
}

fn start_episode(
    data: &Prototypes,
    world: &mut World,
    raster: &MapRaster,
    vehicles: &mut Vehicles,
    patches: &mut RasterPatches,
    rules: fsim_core::rl_config::EpisodeRules,
) -> Episode {
    let setup = EpisodeSetup {
        map: "rock".into(),
        vehicle: "car".into(),
        fuel: "coal".into(),
        start: Pose {
            x: 0.5,
            y: 12.0,
            orientation: 0.0,
        },
        target: Point { x: 0.5, y: -20.0 },
        radius: 3.0,
        start_speed: 0.0,
        seed: None,
    };
    Episode::start(
        Ctx {
            data,
            raster,
            world,
            vehicles,
            patches,
        },
        setup,
        rules,
    )
    .unwrap()
}

#[test]
fn episodes_end_by_destruction_timeout_and_stall() {
    let data = data();
    let (mut world, raster) = rock_world(&data);
    let mut vehicles = Vehicles::default();
    vehicles.push(&data, 0, 0);
    let mut patches = RasterPatches::default();
    let base = config(Mode::Reactive, "car").rules();
    let forward = encode_action(1, 0);
    let idle = encode_action(0, 0);

    // Destroyed: a nearly dead car drives into the rock.
    let mut e = start_episode(
        &data,
        &mut world,
        &raster,
        &mut vehicles,
        &mut patches,
        base.clone(),
    );
    assert!(e.optimal_distance > 32.0 && e.optimal_distance < 40.0);
    vehicles.health[0] = 0.5;
    let mut last = None;
    while !e.status.done() {
        let ctx = Ctx {
            data: &data,
            raster: &raster,
            world: &mut world,
            vehicles: &mut vehicles,
            patches: &mut patches,
        };
        last = Some(e.decide(ctx, forward).unwrap());
    }
    assert_eq!(e.status, Status::Destroyed);
    assert!(e.status.terminated() && !e.status.truncated());
    let last = last.unwrap();
    assert_eq!(last.terms.destroyed, base.reward.destroyed);
    assert_eq!(last.terms.collision, base.reward.collision);
    assert!(last.terms.damage < 0.0);
    assert!(e.collisions > 0);

    // Timeout: the limit is base_ticks when per-tile ticks are zero.
    let mut rules = base.clone();
    rules.time_limit.base_ticks = 10.0;
    rules.time_limit.per_tile_ticks = 0.0;
    let mut e = start_episode(
        &data,
        &mut world,
        &raster,
        &mut vehicles,
        &mut patches,
        rules,
    );
    let mut decisions = 0;
    while !e.status.done() {
        let ctx = Ctx {
            data: &data,
            raster: &raster,
            world: &mut world,
            vehicles: &mut vehicles,
            patches: &mut patches,
        };
        let d = e.decide(ctx, idle).unwrap();
        assert_eq!(d.terms.progress, 0.0);
        decisions += 1;
    }
    assert_eq!((e.status, e.ticks, decisions), (Status::Timeout, 10, 3));
    assert!(e.status.truncated());

    // Stalled: no path-distance improvement for stall_ticks.
    let mut rules = base;
    rules.time_limit.stall_ticks = 20;
    let mut e = start_episode(
        &data,
        &mut world,
        &raster,
        &mut vehicles,
        &mut patches,
        rules,
    );
    while !e.status.done() {
        let ctx = Ctx {
            data: &data,
            raster: &raster,
            world: &mut world,
            vehicles: &mut vehicles,
            patches: &mut patches,
        };
        e.decide(ctx, idle).unwrap();
    }
    assert_eq!((e.status, e.ticks), (Status::Stalled, 20));
    assert!(e.status.truncated());
    assert!((e.totals.total() - e.totals.time).abs() < 1e-12);
}

#[test]
fn stop_arrival_needs_a_stop_near_the_center() {
    let data = data();
    let mut world = World::empty_rect(128, 128, 1, data.tile_id("grass-1"));
    let raster = MapRaster::build(&world, &data);
    let mut vehicles = Vehicles::default();
    vehicles.push(&data, 0, 0);
    let mut patches = RasterPatches::default();
    let target = Point { x: 0.5, y: 0.5 };
    let mut stop = EpisodeRules::default();
    stop.arrival.mode = ArrivalMode::Stop;
    let weights = stop.reward;
    macro_rules! start {
        ($y:expr, $speed:expr, $rules:expr) => {
            Episode::start(
                Ctx {
                    data: &data,
                    raster: &raster,
                    world: &mut world,
                    vehicles: &mut vehicles,
                    patches: &mut patches,
                },
                EpisodeSetup {
                    map: "empty".into(),
                    vehicle: "car".into(),
                    fuel: "coal".into(),
                    start: Pose {
                        x: 0.5,
                        y: $y,
                        orientation: 0.0,
                    },
                    target,
                    radius: 3.0,
                    start_speed: $speed,
                    seed: None,
                },
                $rules,
            )
            .unwrap()
        };
    }
    macro_rules! decide {
        ($e:expr, $action:expr) => {
            $e.decide(
                Ctx {
                    data: &data,
                    raster: &raster,
                    world: &mut world,
                    vehicles: &mut vehicles,
                    patches: &mut patches,
                },
                $action,
            )
            .unwrap()
        };
    }

    // `enter` (the default) succeeds on entering the radius, without the stop terms.
    let mut e = start!(30.5, 0.0, EpisodeRules::default());
    while !e.status.done() {
        decide!(e, encode_action(1, 0));
    }
    assert_eq!(e.status, Status::Success);
    assert!(vehicles.speed[0] > 0.1);
    assert_eq!((e.totals.arrival, e.totals.target_speed), (0.0, 0.0));

    // `stop`: driving through pays `arrival` once and the speed penalty, and doesn't end the episode.
    let mut e = start!(30.5, 0.0, stop.clone());
    while vehicles.y[0] > target.y - 6.0 {
        decide!(e, encode_action(1, 0));
        assert_eq!(e.status, Status::Running);
    }
    assert!(e.reached);
    assert_eq!(e.totals.arrival, weights.arrival);
    assert!(e.totals.target_speed < 0.0);
    // Stopped, but too far from the center: still running.
    while vehicles.speed[0] != 0.0 {
        decide!(e, encode_action(-1, 0));
    }
    assert_eq!(e.status, Status::Running);
    assert!(e.target_distance(&vehicles) > stop.arrival.stop_radius);

    // Moving through the center is not a stop; stopped there is.
    let mut e = start!(0.5, 0.05, stop.clone());
    let d = decide!(e, encode_action(0, 0));
    assert_eq!(e.status, Status::Running);
    assert_eq!(d.terms.arrival, weights.arrival);
    assert!(d.terms.target_speed < 0.0);
    while !e.status.done() {
        decide!(e, encode_action(-1, 0));
    }
    assert_eq!(e.status, Status::Success);
    assert!(vehicles.speed[0].abs() <= stop.arrival.stop_speed);
    assert!(e.target_distance(&vehicles) <= stop.arrival.stop_radius);
    assert_eq!(e.totals.success, weights.success);
    assert_eq!(e.totals.arrival, weights.arrival);
}

#[test]
fn baseline_reaches_targets_on_the_fixture_for_car_and_tank() {
    for vehicle in ["car", "tank"] {
        for mode in [Mode::Reactive, Mode::DirectPlan, Mode::PathPlanner] {
            let n = 8;
            let mut env = env(config(mode, vehicle), n, 3);
            let mut b = Buffers::new(n);
            b.reset(&mut env);
            let mut finished = vec![None; n];
            while finished.iter().any(|f| f.is_none()) {
                for (i, info) in b.step_baseline(&mut env).iter().enumerate() {
                    if finished[i].is_none() && (info.terminated || info.truncated) {
                        finished[i] = Some(*info);
                    }
                }
            }
            for info in finished.into_iter().flatten() {
                assert!(info.success, "{vehicle} {mode:?} baseline failed: {info:?}");
                assert!(info.terms[5] > 0.0);
            }
        }
    }
}

#[test]
fn replaying_a_record_reproduces_the_final_state() {
    for mode in [Mode::Reactive, Mode::PathPlanner] {
        let mut env = env(config(mode, "tank"), 2, 41);
        env.set_recording(true);
        let mut b = Buffers::new(2);
        b.reset(&mut env);
        let mut records = Vec::new();
        for step in 0..3000 {
            if records.len() >= 3 {
                break;
            }
            if mode == Mode::Reactive && step % 3 == 0 {
                // Perturb the baseline so episodes include contacts.
                let actions: Vec<i64> = (0..2).map(|i| scripted(step, i)).collect();
                b.step(&mut env, Actions::Reactive(&actions));
            } else {
                b.step_baseline(&mut env);
            }
            records.extend(env.take_records());
        }
        assert!(!records.is_empty());
        let data = data();
        let map = fixture_map(&data);
        for record in records {
            let json = serde_json::to_string(&record).unwrap();
            let record: fsim_core::vec_env::EpisodeRecord = serde_json::from_str(&json).unwrap();
            assert!(!record.overlays.is_empty());
            let mut world = map.template.clone();
            let mut vehicles = Vehicles::default();
            vehicles.push(&data, 0, 0);
            let mut patches = RasterPatches::default();
            let ctx = Ctx {
                data: &data,
                raster: &map.raster,
                world: &mut world,
                vehicles: &mut vehicles,
                patches: &mut patches,
            };
            let mut e = Episode::start(ctx, record.setup(), record.rules()).unwrap();
            for &a in &record.actions {
                let ctx = Ctx {
                    data: &data,
                    raster: &map.raster,
                    world: &mut world,
                    vehicles: &mut vehicles,
                    patches: &mut patches,
                };
                e.decide(ctx, a).unwrap();
            }
            let replayed = fsim_core::vec_env::EpisodeRecord::new(
                &e,
                0,
                record.env_index,
                0,
                &vehicles,
                record.overlays.clone(),
            );
            assert_eq!(replayed.result, record.result);
        }
    }
}

#[test]
fn cached_crops_match_the_uncached_rasterizer() {
    let data = data();
    let map = fixture_map(&data);
    let mut world = map.template.clone();
    let mut patches = RasterPatches::default();
    // Destroy some trees, a rock and a nest so patches are exercised.
    for (kind, count) in [("tree", 40), ("rock", 1), ("nest", 1)] {
        let indices: Vec<usize> = (0..world.entities.len())
            .filter(|i| world.entities[*i].kind == kind)
            .take(count)
            .collect();
        for i in indices {
            world.set_health(i, 0.0);
        }
    }
    // The cached raster covers map tiles only; the old rasterizer also marks
    // off-map cells under boxes that cross the map edge. Remove those.
    let edge = [[-64.0, -64.0], [64.0, 64.0]];
    for i in 0..world.entities.len() {
        let b = world.entities[i].shape().aabb();
        if b[0][0] < edge[0][0]
            || b[0][1] < edge[0][1]
            || b[1][0] > edge[1][0]
            || b[1][1] > edge[1][1]
        {
            assert_ne!(world.entities[i].kind, "cliff");
            world.set_health(i, 0.0);
        }
    }
    patches.sync(&world, &map.raster, &data);
    assert!(!patches.tiles.is_empty());
    let mut scratch = ObsScratch::default();
    let target = Target {
        x: 3.25,
        y: -7.5,
        radius: 3.0,
    };
    for (tx, ty) in [(0, 0), (-40, 17), (60, -63), (-64, -64), (25, 30), (3, -5)] {
        let mut local = vec![0u8; LOCAL_LEN];
        policy_obs::write_local(
            &map.raster,
            &patches,
            [tx, ty],
            Some(([target.x, target.y], target.radius)),
            &mut scratch,
            &mut local,
        );
        let reference = observation::local(&world, &data, tx as f64, ty as f64, Some(target));
        for (c, (cached, old)) in local.iter().zip(&reference).enumerate() {
            let expected = if c / (64 * 64) == 6 {
                friction_byte(*old as f64)
            } else {
                (*old * 255.0) as u8
            };
            assert_eq!(*cached, expected, "local cell {c} at ({tx}, {ty})");
        }
        let mut coarse = vec![0u8; COARSE_LEN];
        policy_obs::write_coarse(
            &map.raster,
            &patches,
            [tx, ty],
            Some(([target.x, target.y], target.radius)),
            &mut scratch,
            &mut coarse,
        );
        let reference = observation::coarse(&world, &data, tx as f64, ty as f64, Some(target));
        for (c, (cached, old)) in coarse.iter().zip(&reference).enumerate() {
            if c / (32 * 32) == 6 {
                // Mean of exact friction bytes vs byte of an f32 mean: rounding may differ by one.
                let expected = friction_byte(*old as f64);
                assert!(
                    cached.abs_diff(expected) <= 1,
                    "coarse friction {c} at ({tx}, {ty})"
                );
            } else {
                let expected = (*old * 255.0).round() as u8;
                assert_eq!(*cached, expected, "coarse cell {c} at ({tx}, {ty})");
            }
        }
    }
}

#[test]
fn spec_lists_vector_features_and_a_stable_hash() {
    let spec = serde_json::to_value(policy_obs::spec(&data())).unwrap();
    assert_eq!(
        spec["vector_features"].as_array().unwrap().len(),
        VECTOR_LEN
    );
    assert_eq!(spec["local_shape"], serde_json::json!([8, 64, 64]));
    let hash = spec["spec_hash"].as_str().unwrap();
    assert_eq!(hash.len(), 16);
    assert_eq!(hash, policy_obs::spec(&data()).spec_hash);
    assert_eq!(spec["vehicles"], serde_json::json!(["car", "tank"]));
}

#[test]
fn retarget_keeps_the_vehicle_and_restarts_bookkeeping() {
    let data = data();
    let map = fixture_map(&data);
    let mut world = map.template.clone();
    let raster = &map.raster;
    let mut vehicles = Vehicles::default();
    vehicles.push(&data, data.vehicle_index("car").unwrap(), 0);
    let mut patches = RasterPatches::default();
    let rules = config(Mode::Reactive, "car").rules();
    let target = config(Mode::Reactive, "car").target;
    let sampled = sampler::sample(&world, raster, &data, 0, &rules, &target, 5).unwrap();
    let setup = EpisodeSetup {
        map: world.info.name.clone(),
        vehicle: "car".into(),
        fuel: "coal".into(),
        start: sampled.start,
        target: sampled.target,
        radius: 3.0,
        start_speed: 0.0,
        seed: None,
    };
    macro_rules! ctx {
        () => {
            Ctx {
                data: &data,
                raster,
                world: &mut world,
                vehicles: &mut vehicles,
                patches: &mut patches,
            }
        };
    }
    let mut e = Episode::start_with_field(ctx!(), setup, rules.clone(), sampled.field).unwrap();
    for _ in 0..10 {
        e.decide(ctx!(), encode_action(1, 0)).unwrap();
    }
    assert_eq!(e.status, Status::Running);
    let before = vehicles.state(0, &data);
    assert!(before.speed > 0.0);

    // Back to the start: reachable, since the car just drove from there.
    let back = Point {
        x: sampled.start.x,
        y: sampled.start.y,
    };
    let r = Episode::retarget(ctx!(), "fixture".into(), back, 3.0, rules.clone())
        .unwrap()
        .unwrap();
    let after = vehicles.state(0, &data);
    assert_eq!(
        (
            after.x,
            after.y,
            after.orientation,
            after.speed,
            after.health,
            after.tick
        ),
        (
            before.x,
            before.y,
            before.orientation,
            before.speed,
            before.health,
            before.tick
        )
    );
    assert_eq!(
        (r.status, r.ticks, r.decisions(), r.stall_ticks()),
        (Status::Running, 0, 0, 0)
    );
    assert_eq!(r.totals.total(), 0.0);
    assert_eq!((r.setup.start.x, r.setup.start.y), (after.x, after.y));
    assert_eq!(r.setup.target, back);
    // The field now leads to the new target and is measured from the current position.
    assert!(r.field.sample(back.x, back.y) < 1.0);
    assert_eq!(r.optimal_distance, r.field.sample(after.x, after.y));
    assert_eq!(r.path_distance, r.optimal_distance);
    let limit = rules.time_limit;
    assert_eq!(
        r.time_limit_ticks,
        (limit.base_ticks + limit.per_tile_ticks * r.optimal_distance).ceil() as u64
    );

    // The policy observation is built as at a sampled episode's start.
    let mut vector = vec![0.0; VECTOR_LEN];
    policy_obs::write_vector(
        policy_obs::ObsSource {
            data: &data,
            world: &world,
            raster,
            patches: &patches,
            vehicles: &vehicles,
            episode: &r,
            route: None,
        },
        &mut vector,
    );
    let names = policy_obs::VECTOR_FEATURES;
    let feature = |name: &str| vector[names.iter().position(|n| *n == name).unwrap()];
    assert_eq!(feature("time_fraction"), 0.0);
    assert!(feature("speed") > 0.0);
    assert!(vector.iter().all(|v| v.is_finite()));

    // Off the map: unreachable, nothing changes.
    let far = Point {
        x: 10_000.0,
        y: 0.0,
    };
    assert!(
        Episode::retarget(ctx!(), "fixture".into(), far, 3.0, rules.clone())
            .unwrap()
            .is_none()
    );
    vehicles.health[0] = 0.0;
    assert!(Episode::retarget(ctx!(), "fixture".into(), back, 3.0, rules).is_err());
}

#[test]
fn random_start_poses_are_valid_for_car_and_tank() {
    let data = data();
    let map = fixture_map(&data);
    let (world, raster) = (&map.template, &map.raster);
    let field = fsim_core::rl_config::FieldConfig::default();
    for vehicle in ["car", "tank"] {
        let index = data.vehicle_index(vehicle).unwrap();
        let mut headings = std::collections::HashSet::new();
        for seed in 0..50 {
            let pose =
                sampler::sample_start(world, raster, &data, index, &field, seed, 10_000).unwrap();
            assert_eq!(
                sampler::sample_start(world, raster, &data, index, &field, seed, 10_000).unwrap(),
                pose
            );
            assert!(
                rl_episode::pose_is_clear(world, &data, index, pose),
                "{vehicle} {pose:?}"
            );
            assert!(!rl_episode::in_nest_zone(world, raster, pose.x, pose.y));
            assert!(!world.water(pose.x.floor() as i32, pose.y.floor() as i32, &data));
            let margin = sampler::START_MARGIN as f64;
            assert!(pose.x >= raster.x_min as f64 + margin);
            assert!(pose.x < (raster.x_min + raster.width as i32) as f64 - margin);
            assert!(pose.y >= raster.y_min as f64 + margin);
            assert!(pose.y < (raster.y_min + raster.height as i32) as f64 - margin);
            assert!((0.0..1.0).contains(&pose.orientation));
            headings.insert((pose.orientation * 1e6) as i64);
        }
        assert!(headings.len() > 10, "headings should vary");
    }
}

#[test]
fn student_rays_measure_clearance_from_the_hull() {
    let data = data();
    let (mut world, raster) = rock_world(&data);
    let (mut vehicles, mut patches) = (Vehicles::default(), RasterPatches::default());
    vehicles.push(&data, data.vehicle_index("car").unwrap(), 0);
    let rules = RlConfig::default().rules();
    let episode = start_episode(
        &data,
        &mut world,
        &raster,
        &mut vehicles,
        &mut patches,
        rules,
    );
    let mut student = vec![0.0; STUDENT_LEN];
    let source = policy_obs::ObsSource {
        data: &data,
        world: &world,
        raster: &raster,
        patches: &patches,
        vehicles: &vehicles,
        episode: &episode,
        route: None,
    };
    student_obs::write_student(source, &mut student);
    let names = student_obs::feature_names();
    let get = |name: &str| student[names.iter().position(|n| n == name).unwrap()] as f64;
    // Straight ahead (ray 15, 0 deg) from y = 12 toward the rock: the first
    // hard tile row at x = 0 above the car, minus the car's half length.
    let row = (-64..12)
        .rev()
        .find(|&y| raster.mask_at(0, y) & fsim_core::raster::ROCK != 0)
        .unwrap();
    let half_length = 1.0;
    let expected = (12.0 - (row as f64 + 1.0) - half_length) / student_obs::RAY_RANGE;
    assert!(
        (get("hard_15") - expected).abs() < 1e-6,
        "{}",
        get("hard_15")
    );
    assert_eq!(
        get("tree_15"),
        get("hard_15"),
        "trees count only before the hard hit"
    );
    // Behind and to the sides the 128-tile world is open within range.
    for ray in [7, 23, 31] {
        assert_eq!(get(&format!("hard_{ray}")), 1.0);
    }
    // The route leads around the rock toward the target 32 tiles north.
    assert!(get("route32_forward") > 0.8);
    assert!(get("route_length") > 32.0 / 64.0);
    assert_eq!(get("speed"), 0.0);
    assert_eq!(get("health"), 1.0);
}

#[test]
fn student_obs_matches_across_modes_and_shares_teacher_features() {
    let run = |observation: Observation| {
        let mut cfg = config(Mode::Reactive, "tank");
        cfg.observation = observation;
        let mut env = env(cfg, 3, 5);
        let full = observation.full() as usize;
        let mut local = vec![0; 3 * LOCAL_LEN * full];
        let mut coarse = vec![0; 3 * COARSE_LEN * full];
        let mut vector = vec![0.0; 3 * VECTOR_LEN * full];
        let mut student = vec![0.0; 3 * STUDENT_LEN];
        let (mut fl, mut fc, mut fv, mut fs) = (
            local.clone(),
            coarse.clone(),
            vector.clone(),
            student.clone(),
        );
        let mut trace = Vec::new();
        for step in 0..120 {
            let actions: Vec<i64> = (0..3).map(|i| scripted(step, i)).collect();
            env.step(
                Actions::Reactive(&actions),
                ObsBatch {
                    local: &mut local,
                    coarse: &mut coarse,
                    vector: &mut vector,
                    student: &mut student,
                },
                ObsBatch {
                    local: &mut fl,
                    coarse: &mut fc,
                    vector: &mut fv,
                    student: &mut fs,
                },
            )
            .unwrap();
            trace.push((student.clone(), vector.clone()));
        }
        trace
    };
    let both = run(Observation::Both);
    let student_only = run(Observation::Student);
    assert_eq!(both.len(), student_only.len());
    let names = student_obs::feature_names();
    for ((student, vector), (alone, _)) in both.iter().zip(&student_only) {
        assert_eq!(student, alone);
        assert!(student.iter().all(|v| v.is_finite()));
        for (row, features) in student.chunks(STUDENT_LEN).enumerate() {
            let teacher = &vector[row * VECTOR_LEN..(row + 1) * VECTOR_LEN];
            for (k, name) in names.iter().enumerate() {
                let ray = k < 3 * student_obs::RAY_COUNT;
                if ray {
                    assert!((0.0..=1.0).contains(&features[k]));
                }
                if let Some(t) = policy_obs::VECTOR_FEATURES.iter().position(|f| f == name) {
                    assert_eq!(features[k], teacher[t], "{name}");
                }
            }
        }
    }
    // A wrong buffer size for the configured observation is an error.
    let mut cfg = config(Mode::Reactive, "car");
    cfg.observation = Observation::Student;
    let mut env = env(cfg, 1, 5);
    let mut vector = vec![0.0; VECTOR_LEN];
    let err = env.reset(ObsBatch {
        local: &mut [],
        coarse: &mut [],
        vector: &mut vector,
        student: &mut [],
    });
    assert!(err.is_err());
}

#[test]
fn student_spec_has_its_own_hash_and_keeps_the_teacher_hash() {
    let spec = policy_obs::spec(&data());
    assert_eq!(spec.spec_hash, "16a3d4353d1658c1");
    let json = serde_json::to_value(&spec).unwrap();
    assert_eq!(json["student"]["size"], STUDENT_LEN);
    assert_eq!(
        json["student"]["features"].as_array().unwrap().len(),
        STUDENT_LEN
    );
    assert_eq!(json["student"]["spec_hash"].as_str().unwrap().len(), 16);
}
