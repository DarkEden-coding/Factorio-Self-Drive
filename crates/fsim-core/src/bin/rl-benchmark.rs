//! RL environment benchmark on the real pool: distance field, observation and
//! reset cost, scenario sampler acceptance, VecEnv decisions/sec per mode and
//! env count, and A* baseline success.
//! Usage: rl-benchmark [pool-dir] [--quick] [--scenarios-only]
use fsim_core::{
    distance_field::{DistanceField, FieldParams, Region},
    policy_obs::{self, ObsScratch, ObsSource, COARSE_LEN, LOCAL_LEN, VECTOR_LEN},
    raster::RasterPatches,
    rl_config::{
        ArrivalScenario, ForestScenario, LongScenario, Mode, NestScenario, ReverseScenario,
        RlConfig, Scenario, StandardScenario, TargetConfig, VehicleChoice,
    },
    rl_episode::{Ctx, Episode, EpisodeSetup},
    sampler::{self, SampleStats},
    student_obs::{self, STUDENT_LEN},
    vec_env::{Actions, ObsBatch, OwnedActions, StepInfo, VecEnv},
    vehicle::Vehicles,
    Prototypes,
};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg32;
use std::{hint::black_box, sync::Arc, time::Instant};

struct Buffers {
    local: Vec<u8>,
    coarse: Vec<u8>,
    vector: Vec<f32>,
    final_local: Vec<u8>,
    final_coarse: Vec<u8>,
    final_vector: Vec<f32>,
}

impl Buffers {
    fn new(n: usize) -> Self {
        Self {
            local: vec![0; n * LOCAL_LEN],
            coarse: vec![0; n * COARSE_LEN],
            vector: vec![0.0; n * VECTOR_LEN],
            final_local: vec![0; n * LOCAL_LEN],
            final_coarse: vec![0; n * COARSE_LEN],
            final_vector: vec![0.0; n * VECTOR_LEN],
        }
    }
    fn step(&mut self, env: &mut VecEnv, actions: Actions<'_>) -> Vec<StepInfo> {
        env.step(
            actions,
            ObsBatch {
                local: &mut self.local,
                coarse: &mut self.coarse,
                vector: &mut self.vector,
                student: &mut [],
            },
            ObsBatch {
                local: &mut self.final_local,
                coarse: &mut self.final_coarse,
                vector: &mut self.final_vector,
                student: &mut [],
            },
        )
        .unwrap()
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
}

fn config(pool: &str, mode: Mode) -> RlConfig {
    RlConfig {
        map_pool: pool.into(),
        mode,
        vehicle_mix: vec![
            VehicleChoice {
                vehicle: "car".into(),
                fuel: "coal".into(),
                weight: 1.0,
            },
            VehicleChoice {
                vehicle: "tank".into(),
                fuel: "coal".into(),
                weight: 1.0,
            },
        ],
        ..RlConfig::default()
    }
}

fn random_actions(env: &VecEnv, rng: &mut Pcg32) -> (Vec<i64>, Vec<f32>) {
    let n = env.num_envs();
    let rules = env.rules();
    match env.mode() {
        Mode::Reactive => ((0..n).map(|_| rng.gen_range(0..9)).collect(), Vec::new()),
        Mode::DirectPlan => (
            (0..n * rules.direct_plan.horizon)
                .map(|_| rng.gen_range(0..9))
                .collect(),
            Vec::new(),
        ),
        Mode::PathPlanner => (
            Vec::new(),
            (0..n * rules.path_planner.waypoints)
                .flat_map(|k| {
                    let ahead = 4.0 * (k % rules.path_planner.waypoints + 1) as f32;
                    [rng.gen_range(-0.5..0.5) * ahead, ahead]
                })
                .collect(),
        ),
    }
}

fn actions<'a>(mode: Mode, discrete: &'a [i64], waypoints: &'a [f32]) -> Actions<'a> {
    match mode {
        Mode::Reactive => Actions::Reactive(discrete),
        Mode::DirectPlan => Actions::DirectPlan(discrete),
        Mode::PathPlanner => Actions::PathPlanner(waypoints),
    }
}

/// Sampler acceptance and reset cost per scenario kind (default parameters).
fn scenario_table(
    pool: &str,
    data: &Arc<Prototypes>,
    maps: &[Arc<fsim_core::vec_env::MapData>],
    quick: bool,
) {
    println!(
        "\n| scenario | vehicle | samples | map success | start accept | target accept | ms/sample | ms/reset | fallback |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|");
    let kinds = [
        Scenario::Standard(StandardScenario::default()),
        Scenario::Long(LongScenario::default()),
        Scenario::Forest(ForestScenario::default()),
        Scenario::Reverse(ReverseScenario::default()),
        Scenario::Nest(NestScenario::default()),
        Scenario::Arrival(ArrivalScenario::default()),
    ];
    let samples = if quick { 48 } else { 192 };
    let resets = if quick { 24 } else { 96 };
    for scenario in kinds {
        for vehicle in ["car", "tank"] {
            let mut c = config(pool, Mode::Reactive);
            c.vehicle_mix = vec![VehicleChoice {
                vehicle: vehicle.into(),
                fuel: "coal".into(),
                weight: 1.0,
            }];
            c.scenarios = vec![scenario];
            let rules = c.rules();
            let v = data.vehicle_index(vehicle).unwrap();
            let mut stats = SampleStats::default();
            let mut ok = 0;
            let t = Instant::now();
            for i in 0..samples {
                let map = &maps[i % maps.len()];
                ok += sampler::sample_scenario(
                    &map.template,
                    &map.raster,
                    data,
                    v,
                    &rules,
                    &scenario,
                    &c.target,
                    i as u64,
                    &mut stats,
                )
                .is_ok() as usize;
            }
            let per_sample = t.elapsed().as_secs_f64() / samples as f64;
            // Full resets on one env: map and vehicle draw, sampling with
            // retries on other maps, fallback to standard, episode start.
            let mut env = VecEnv::with_maps(c, data.clone(), maps.to_vec(), 1, 5).unwrap();
            let mut buffers = Buffers::new(1);
            let mut fallbacks = 0;
            let t = Instant::now();
            for _ in 0..resets {
                buffers.reset(&mut env);
                fallbacks += (env.scenario(0) != scenario.index()) as usize;
            }
            let per_reset = t.elapsed().as_secs_f64() / resets as f64;
            let pct = |a: u64, b: u64| {
                if b == 0 {
                    "-".to_string()
                } else {
                    format!("{:.1}%", 100.0 * a as f64 / b as f64)
                }
            };
            println!(
                "| {} | {vehicle} | {samples} | {} | {} | {} | {:.2} | {:.2} | {} |",
                scenario.name(),
                pct(ok as u64, samples as u64),
                pct(stats.starts, stats.start_draws),
                pct(stats.targets, stats.target_checks),
                per_sample * 1e3,
                per_reset * 1e3,
                pct(fallbacks as u64, resets as u64),
            );
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let quick = args.iter().any(|a| a == "--quick");
    let scenarios_only = args.iter().any(|a| a == "--scenarios-only");
    let pool = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "data/game/maps".into());
    let data =
        Arc::new(Prototypes::from_json(include_str!("../../../../data/prototypes.json")).unwrap());
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!("threads: {threads}");

    let start = Instant::now();
    let base = VecEnv::new(config(&pool, Mode::Reactive), data.clone(), 1, 1).unwrap();
    let maps = base.maps().to_vec();
    let raster_bytes: usize = maps.iter().map(|m| m.raster.memory_bytes()).sum();
    println!(
        "load {} train maps + rasters: {:.2} s, raster cache {:.1} MB total",
        maps.len(),
        start.elapsed().as_secs_f64(),
        raster_bytes as f64 / 1e6
    );

    scenario_table(&pool, &data, &maps, quick);
    if scenarios_only {
        return;
    }

    // Distance field cost by target distance, car and tank, over several maps.
    let rules = base.rules().clone();
    for max in [64.0, 128.0, 256.0, 512.0] {
        for vehicle in ["car", "tank"] {
            let v = data.vehicle_index(vehicle).unwrap();
            let target = TargetConfig {
                min_distance: max * 0.75,
                max_distance: max,
                max_tortuosity: 3.0,
                radius: 3.0,
            };
            let (mut total, mut count, mut cells) = (0.0, 0, 0);
            for (i, map) in maps.iter().take(8).enumerate() {
                let Ok(s) = sampler::sample(
                    &map.template,
                    &map.raster,
                    &data,
                    v,
                    &rules,
                    &target,
                    i as u64,
                ) else {
                    continue;
                };
                let params = FieldParams::new(&data.vehicles[v], &rules.distance_field);
                let region = Region::around(
                    &[[s.start.x, s.start.y], [s.target.x, s.target.y]],
                    rules.distance_field.margin,
                    &map.raster,
                );
                let t = Instant::now();
                let reps = 5;
                for _ in 0..reps {
                    black_box(DistanceField::compute(
                        &map.raster,
                        region,
                        params,
                        [s.target.x, s.target.y],
                        f64::INFINITY,
                    ));
                }
                total += t.elapsed().as_secs_f64() / reps as f64;
                count += 1;
                cells += region.width() * region.height();
            }
            if count > 0 {
                println!(
                    "distance field {vehicle} path {:.0}-{max:.0} tiles: {:.3} ms ({} cells avg, {count} maps)",
                    max * 0.75,
                    total / count as f64 * 1e3,
                    cells / count
                );
            }
        }
    }

    // Observation cost.
    {
        let map = &maps[0];
        let mut world = map.template.clone();
        let mut vehicles = Vehicles::default();
        vehicles.push(&data, 0, data.fuel_index("coal").unwrap());
        let mut patches = RasterPatches::default();
        let s = sampler::sample(
            &world,
            &map.raster,
            &data,
            0,
            &rules,
            &TargetConfig::default(),
            7,
        )
        .unwrap();
        let ctx = Ctx {
            data: &data,
            raster: &map.raster,
            world: &mut world,
            vehicles: &mut vehicles,
            patches: &mut patches,
        };
        let setup = EpisodeSetup {
            map: map.template.info.name.clone(),
            vehicle: "car".into(),
            fuel: "coal".into(),
            start: s.start,
            target: s.target,
            radius: 3.0,
            start_speed: 0.0,
            seed: None,
        };
        let episode = Episode::start(ctx, setup, rules.clone()).unwrap();
        let mut scratch = ObsScratch::default();
        let (mut local, mut coarse, mut vector) = (
            vec![0; LOCAL_LEN],
            vec![0; COARSE_LEN],
            vec![0.0; VECTOR_LEN],
        );
        let source = ObsSource {
            data: &data,
            world: &world,
            raster: &map.raster,
            patches: &patches,
            vehicles: &vehicles,
            episode: &episode,
            route: None,
        };
        let reps = 2000;
        let t = Instant::now();
        for _ in 0..reps {
            policy_obs::write_obs(
                source,
                &mut scratch,
                policy_obs::ObsMut {
                    local: &mut local,
                    coarse: &mut coarse,
                    vector: &mut vector,
                    student: &mut [],
                },
            );
            black_box(&local);
        }
        println!(
            "policy obs (local + coarse + vector): {:.1} us",
            t.elapsed().as_secs_f64() / reps as f64 * 1e6
        );
        let mut student = vec![0.0; STUDENT_LEN];
        let t = Instant::now();
        let mut visited = 0;
        for _ in 0..reps {
            visited = student_obs::write_student(source, &mut student);
            black_box(&student);
        }
        println!(
            "student obs: {:.1} us, {visited} ray tiles",
            t.elapsed().as_secs_f64() / reps as f64 * 1e6
        );
    }

    // Reset cost: new episodes for 64 envs in parallel and serially per env.
    {
        let mut env = VecEnv::new(config(&pool, Mode::Reactive), data.clone(), 64, 3).unwrap();
        let mut buffers = Buffers::new(64);
        let reps = if quick { 3 } else { 10 };
        let t = Instant::now();
        for _ in 0..reps {
            buffers.reset(&mut env);
        }
        let per_batch = t.elapsed().as_secs_f64() / reps as f64;
        let mut serial = VecEnv::new(config(&pool, Mode::Reactive), data.clone(), 1, 3).unwrap();
        let mut single = Buffers::new(1);
        let t = Instant::now();
        for _ in 0..reps * 20 {
            single.reset(&mut serial);
        }
        println!(
            "reset: {:.2} ms per env (single thread), {:.2} ms per 64-env batch",
            t.elapsed().as_secs_f64() / (reps * 20) as f64 * 1e3,
            per_batch * 1e3
        );
    }

    // Throughput.
    println!("\n| mode | N | env steps/s | decisions/s | episodes/s |");
    println!("|---|---:|---:|---:|---:|");
    let seconds = if quick { 0.5 } else { 2.0 };
    for mode in [Mode::Reactive, Mode::DirectPlan, Mode::PathPlanner] {
        for n in [1, 16, 64, 256] {
            let mut env =
                VecEnv::with_maps(config(&pool, mode), data.clone(), maps.clone(), n, 11).unwrap();
            let mut buffers = Buffers::new(n);
            buffers.reset(&mut env);
            let mut rng = Pcg32::seed_from_u64(5);
            let (mut steps, mut decisions, mut episodes) = (0u64, 0u64, 0u64);
            let t = Instant::now();
            while t.elapsed().as_secs_f64() < seconds {
                let (d, w) = random_actions(&env, &mut rng);
                let infos = buffers.step(&mut env, actions(mode, &d, &w));
                steps += n as u64;
                for i in &infos {
                    decisions += i.decisions as u64;
                    episodes += (i.terminated || i.truncated) as u64;
                }
            }
            let e = t.elapsed().as_secs_f64();
            println!(
                "| {} | {n} | {:.0} | {:.0} | {:.0} |",
                mode.name(),
                steps as f64 / e,
                decisions as f64 / e,
                episodes as f64 / e
            );
        }
    }

    // Baseline success on train maps.
    println!("\n| baseline | mode | episodes | success | destroyed | timeout | stalled |");
    println!("|---|---|---:|---:|---:|---:|---:|");
    for vehicle in ["car", "tank"] {
        for mode in [Mode::Reactive, Mode::DirectPlan, Mode::PathPlanner] {
            let mut c = config(&pool, mode);
            c.vehicle_mix = vec![VehicleChoice {
                vehicle: vehicle.into(),
                fuel: "coal".into(),
                weight: 1.0,
            }];
            let n = 64;
            let mut env = VecEnv::with_maps(c, data.clone(), maps.clone(), n, 21).unwrap();
            let mut buffers = Buffers::new(n);
            buffers.reset(&mut env);
            let mut counts = [0u32; 4];
            let target = if quick { 64 } else { 256 };
            while counts.iter().sum::<u32>() < target {
                let infos = match env.baseline_actions() {
                    OwnedActions::Discrete(d) => buffers.step(&mut env, actions(mode, &d, &[])),
                    OwnedActions::Waypoints(w) => buffers.step(&mut env, actions(mode, &[], &w)),
                };
                for i in &infos {
                    for (k, flag) in [i.success, i.destroyed, i.timeout, i.stalled]
                        .into_iter()
                        .enumerate()
                    {
                        counts[k] += flag as u32;
                    }
                }
            }
            let total: u32 = counts.iter().sum();
            let pct = |c: u32| format!("{:.0}%", 100.0 * c as f64 / total as f64);
            println!(
                "| {vehicle} | {} | {total} | {} | {} | {} | {} |",
                mode.name(),
                pct(counts[0]),
                pct(counts[1]),
                pct(counts[2]),
                pct(counts[3])
            );
        }
    }
}
