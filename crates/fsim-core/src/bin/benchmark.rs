use fsim_core::{map_pool::MapPool, Config, Prototypes, Simulator};
use std::{hint::black_box, time::Instant};

fn main() {
    let data = Prototypes::from_json(include_str!("../../../../data/prototypes.json")).unwrap();
    let start = Instant::now();
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data/game/maps".into());
    let pool = MapPool::load(path, 123).unwrap();
    let world = pool.map(0, &data).unwrap().0;
    let elapsed = start.elapsed();
    println!(
        "real map load: {:.2} ms, {} entities",
        elapsed.as_secs_f64() * 1000.0,
        world.entities.len()
    );
    println!(
        "map fingerprints: tiles {:08x}, resources {:08x}",
        fingerprint(&world.tiles),
        fingerprint(&world.resources)
    );
    let mut sim = Simulator::new(data, Config::default(), world).unwrap();
    let steps = 200_000;
    let start = Instant::now();
    // A repeatable driving loop on the real map, resetting on destruction.
    for tick in 0..steps {
        if tick % 4000 == 0 || sim.vehicles.health[0] <= 0.0 {
            sim.reset().unwrap();
        }
        sim.step(1, 1, 1).unwrap();
        black_box(sim.vehicles.x[0]);
    }
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "vehicle on real map: {:.0} ticks/sec ({steps} ticks, {:.3} sec)",
        steps as f64 / elapsed,
        elapsed
    );
    let start = Instant::now();
    for _ in 0..100 {
        black_box(sim.observation_local());
        black_box(sim.observation_coarse());
    }
    println!(
        "local + coarse observations: {:.3} ms/pair",
        start.elapsed().as_secs_f64() * 10.0
    );
}

fn fingerprint(bytes: &[u8]) -> u32 {
    bytes.iter().fold(2166136261, |hash, byte| {
        (hash ^ *byte as u32).wrapping_mul(16777619)
    })
}
