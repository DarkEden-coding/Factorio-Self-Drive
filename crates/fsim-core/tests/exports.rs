use fsim_core::{
    map_import::{load_map, MapExport},
    physics,
    trace::{
        reconstruct, replay, InputSegment, Inputs, Trace, TraceHeader, TraceSample, TraceSurface,
        TraceVehicle,
    },
    Config, Prototypes, Simulator,
};
use std::{fs, process::Command};

const FIXTURE: &str = include_str!("fixtures/map-export.json");
fn data() -> Prototypes {
    Prototypes::from_json(include_str!("../../../data/prototypes.json")).unwrap()
}

#[test]
fn prototype_schema_requires_impact_immunity_and_accepts_live_provenance() {
    let mut snapshot: serde_json::Value =
        serde_json::from_str(include_str!("../../../data/prototypes.json")).unwrap();
    snapshot["source"] = serde_json::json!({
        "game_version": "2.1.23", "active_mods": {"base": "2.1.23", "fsim-export": "0.1.0"}
    });
    let loaded = Prototypes::from_json(&snapshot.to_string()).unwrap();
    assert_eq!(loaded.source.game_version, "2.1.23");
    assert_eq!(loaded.source.active_mods["base"], "2.1.23");
    snapshot["schema_version"] = serde_json::json!(1);
    assert!(Prototypes::from_json(&snapshot.to_string()).is_err());
    snapshot["schema_version"] = serde_json::json!(2);
    snapshot["vehicles"][0]
        .as_object_mut()
        .unwrap()
        .remove("immune_to_all_impacts");
    assert!(Prototypes::from_json(&snapshot.to_string()).is_err());
}

#[test]
fn exported_map_round_trip_preserves_geometry_names_and_offset() {
    let data = data();
    let (world, report) = load_map(FIXTURE, &data).unwrap();
    assert!(report.warnings.is_empty());
    assert_eq!(report.offset, [80, -16]);
    assert_eq!(world.info.source_offset, Some([80, -16]));
    assert_eq!(world.info.width, 32);
    assert_eq!(world.tile(-16, -16), Some(data.tile_id("sand-1")));
    assert_eq!(world.tile(15, -16), Some(data.tile_id("water")));
    assert_eq!(world.resources[world.index(2, 2).unwrap()], 2);
    assert_eq!(world.entities[0].x, -9.5);
    assert_eq!(world.entities[0].y, -4.5);
    assert_eq!(world.health(0), 37.0);
    assert_eq!(world.entities[0].orientation, 0.125);
    let cliff = &world.entities[2];
    let shape = &data.cliffs.orientations["west-to-north"];
    assert_eq!(cliff.collision_box, shape.collision_box);
    assert_eq!(cliff.orientation, shape.orientation);
    assert_eq!(cliff.cliff_orientation.as_deref(), Some("west-to-north"));
    assert_eq!(world.entities[3].kind, "nest");
    let map: MapExport = serde_json::from_str(FIXTURE).unwrap();
    let (round_trip, report) = load_map(&serde_json::to_string(&map).unwrap(), &data).unwrap();
    assert!(report.warnings.is_empty());
    assert_eq!(world.tiles, round_trip.tiles);
    assert_eq!(world.resources, round_trip.resources);
    assert_eq!(
        serde_json::to_value(&*world.entities).unwrap(),
        serde_json::to_value(&*round_trip.entities).unwrap()
    );
}

#[test]
fn map_fallbacks_warn_and_failed_load_does_not_change_the_episode() {
    let data = data();
    let mut map: MapExport = serde_json::from_str(FIXTURE).unwrap();
    map.chunks[0].tiles[0] = "alien-sand".into();
    map.chunks[0].entities[0].name = "alien-tree".into();
    map.chunks[0].entities[2].cliff_orientation = Some("unknown".into());
    map.chunks[0].resources[0].name = "alien-copper-ore".into();
    let (world, report) = load_map(&serde_json::to_string(&map).unwrap(), &data).unwrap();
    assert_eq!(report.warnings.len(), 4);
    assert_eq!(world.tiles[0], data.tile_id("sand-1"));
    assert_eq!(world.entities[0].name, "alien-tree");
    let bytes = include_bytes!("fixtures/nauvis-origin.fsmap");
    let world = fsim_core::map_format::load_map(bytes, &data).unwrap().0;
    let mut sim = Simulator::new(data, Config::default(), world).unwrap();
    sim.set_target(10.0, 10.0).unwrap();
    sim.load_map(bytes).unwrap();
    assert!(sim.target.is_none());
    let state = sim.vehicle_state();
    let revision = sim.world.revision;
    map.chunks.clear();
    assert!(sim.load_map(b"invalid").is_err());
    assert_eq!(state, sim.vehicle_state());
    assert_eq!(revision, sim.world.revision);
    assert!(load_map("{}", &sim.data).is_err());
    let mut duplicate: MapExport = serde_json::from_str(FIXTURE).unwrap();
    duplicate.chunks.push(duplicate.chunks[0].clone());
    assert!(load_map(&serde_json::to_string(&duplicate).unwrap(), &sim.data).is_err());
}

fn synthetic_trace(data: &Prototypes) -> Trace {
    let header = TraceHeader {
        name: "synthetic".into(),
        surface: TraceSurface {
            tile: "grass-1".into(),
            area: [[-16, -16], [16, 16]],
            water: vec![],
        },
        vehicle: TraceVehicle {
            name: "tank".into(),
            position: [0.0, 0.0],
            orientation: 0.0,
            speed: 0.0,
            health: 2000.0,
        },
        fuel: "coal".into(),
        obstacles: vec![],
        inputs: vec![
            InputSegment {
                ticks: 4,
                throttle: 1,
                steer: 0,
            },
            InputSegment {
                ticks: 3,
                throttle: -1,
                steer: 0,
            },
        ],
    };
    let (mut world, mut vehicles) = reconstruct(&header, data).unwrap();
    let mut samples = vec![];
    let mut record = |vehicles: &fsim_core::vehicle::Vehicles, inputs| {
        let state = vehicles.state(0, data);
        samples.push(TraceSample {
            tick: state.tick,
            inputs,
            x: state.x,
            y: state.y,
            orientation: state.orientation,
            speed: state.speed,
            health: state.health,
            obstacle_health: vec![],
        });
    };
    record(
        &vehicles,
        Inputs {
            throttle: 0,
            steer: 0,
        },
    );
    for segment in &header.inputs {
        for _ in 0..segment.ticks {
            physics::step_vehicle(
                &mut world,
                &mut vehicles,
                0,
                data,
                segment.throttle,
                segment.steer,
            );
            record(
                &vehicles,
                Inputs {
                    throttle: segment.throttle,
                    steer: segment.steer,
                },
            );
        }
    }
    Trace {
        schema_version: 1,
        header,
        samples,
    }
}

#[test]
fn replay_binary_reports_zero_error_for_simulator_generated_trace() {
    let data = data();
    let trace = synthetic_trace(&data);
    let report = replay(&trace, &data).unwrap();
    assert_eq!(report.position.max, 0.0);
    assert_eq!(report.position.rms, 0.0);
    assert_eq!(report.first_position_error_tick, None);
    let path = std::env::temp_dir().join(format!("fsim-synthetic-{}.json", std::process::id()));
    fs::write(&path, serde_json::to_string(&trace).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_replay-trace"))
        .arg(&path)
        .arg(&path)
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    let result: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    for metric in [
        "position",
        "orientation",
        "speed",
        "health",
        "obstacle_health",
    ] {
        assert_eq!(result["report"][metric]["max"], 0.0, "{metric}");
        assert_eq!(result["report"][metric]["rms"], 0.0, "{metric}");
    }
    let mut perturbed = trace.clone();
    perturbed.samples[2].x += 0.06;
    perturbed.samples[2].orientation = 0.99;
    let report = replay(&perturbed, &data).unwrap();
    assert_eq!(report.first_position_error_tick, Some(2));
    assert!((report.orientation.max - 0.01).abs() < 1e-12);
    perturbed.samples[2].tick = 5;
    assert!(replay(&perturbed, &data).is_err());
}

#[test]
fn binary_maps_validate_preserve_real_names_and_load_transactionally() {
    use fsim_core::map_format;
    let data = data();
    let bytes = include_bytes!("fixtures/nauvis-origin.fsmap");
    let (world, report) = map_format::load_map(bytes, &data).unwrap();
    assert_eq!(report.name, "nauvis-seed-12345-origin");
    assert!(world.entities.iter().any(|e| e.name == "tree-02-red"));
    assert!(report.warnings.is_empty());
    assert!(map_format::load_map(&bytes[..bytes.len() - 1], &data).is_err());
    let mut trailing = bytes.to_vec();
    trailing.push(0);
    assert!(map_format::load_map(&trailing, &data).is_err());
    let mut sim = Simulator::new(data, Config::default(), world).unwrap();
    sim.set_target(10.0, 10.0).unwrap();
    let before = sim.vehicle_state();
    let revision = sim.world.revision;
    assert!(sim.load_map(&trailing).is_err());
    assert_eq!(sim.vehicle_state(), before);
    assert_eq!(sim.world.revision, revision);
    assert!(sim.target.is_some());
    sim.load_map(bytes).unwrap();
    assert_eq!(sim.world.revision, revision + 1);
    assert!(sim.target.is_none());
}

#[test]
fn pool_loads_index_and_samples_reproducibly() {
    use fsim_core::map_pool::MapPool;
    let root = std::env::temp_dir().join(format!("fsim-map-pool-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let data = data();
    let bytes = include_bytes!("fixtures/nauvis-origin.fsmap");
    let world = fsim_core::map_format::load_map(bytes, &data).unwrap().0;
    fs::write(root.join("real.fsmap"), bytes).unwrap();
    let entry = serde_json::json!({"name": world.info.name, "file": "real.fsmap", "seed": world.info.seed,
        "width":128, "height":128, "x_min":-64, "y_min":-64, "source_offset":[0,0],
        "entity_count":world.entities.len(), "split": "train"});
    fs::write(
        root.join("index.json"),
        serde_json::json!([entry.clone(), entry]).to_string(),
    )
    .unwrap();
    let mut a = MapPool::load(&root, 42).unwrap();
    let mut b = MapPool::load(root.join("index.json"), 42).unwrap();
    assert_eq!(a.entries.len(), 2);
    assert!(a.map(2, &data).is_err());
    for _ in 0..4 {
        assert_eq!(
            a.sample(&data).unwrap().0.tiles,
            b.sample(&data).unwrap().0.tiles
        );
    }
    fs::write(root.join("index.json"), "[]").unwrap();
    assert!(MapPool::load(&root, 1).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn no_spawn_binary_map_is_rejected_without_changing_episode() {
    use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
    use std::io::{Read, Write};
    let bytes = include_bytes!("fixtures/nauvis-origin.fsmap");
    let h = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut body = Vec::new();
    ZlibDecoder::new(&bytes[16 + h..])
        .read_to_end(&mut body)
        .unwrap();
    // Tile name zero in this real fixture is deepwater. Leave the rest of the
    // exported records unchanged so this exercises spawn validation, not parsing.
    body[..128 * 128].fill(0);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&body).unwrap();
    let mut blocked = bytes[..16 + h].to_vec();
    blocked.extend(encoder.finish().unwrap());
    let data = data();
    let world = fsim_core::map_format::load_map(bytes, &data).unwrap().0;
    let mut sim = Simulator::new(data, Config::default(), world).unwrap();
    sim.set_target(5.0, 6.0).unwrap();
    let before = sim.vehicle_state();
    let revision = sim.world.revision;
    assert_eq!(
        sim.load_map(&blocked).unwrap_err(),
        "no valid spawn for this vehicle"
    );
    assert_eq!(before, sim.vehicle_state());
    assert_eq!(revision, sim.world.revision);
    assert!(sim.target.is_some());
}
