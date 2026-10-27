use fsim_core::{
    geometry::Obb, map_format, observation, physics, world::World, Config, Prototypes, Simulator,
};

fn data() -> Prototypes {
    Prototypes::from_json(include_str!("../../../data/prototypes.json")).unwrap()
}
const REAL_MAP: &[u8] = include_bytes!("fixtures/nauvis-origin.fsmap");
fn real(kind: &str) -> Simulator {
    let data = data();
    let world = map_format::load_map(REAL_MAP, &data).unwrap().0;
    Simulator::new(
        data,
        Config {
            vehicle: kind.into(),
            ..Config::default()
        },
        world,
    )
    .unwrap()
}
fn flat(kind: &str) -> Simulator {
    let mut sim = real(kind);
    sim.world = World::empty(2, 123, sim.data.tile_id("grass-1"));
    sim
}

#[test]
fn same_real_map_and_inputs_are_identical() {
    let mut a = real("car");
    let mut b = real("car");
    assert_eq!(a.world.tiles, b.world.tiles);
    assert_eq!(a.world.resources, b.world.resources);
    assert_eq!(
        serde_json::to_string(&*a.world.entities).unwrap(),
        serde_json::to_string(&*b.world.entities).unwrap()
    );
    for tick in 0..1600 {
        let throttle = if tick % 200 < 160 { 1 } else { -1 };
        let steer = if tick % 500 < 250 { 1 } else { -1 };
        a.step(throttle, steer, 1).unwrap();
        b.step(throttle, steer, 1).unwrap();
    }
    assert_eq!(a.vehicle_state(), b.vehicle_state());
    assert_eq!(a.observation_local(), b.observation_local());
}

#[test]
fn coal_cruise_speeds_match_wiki_and_reverse_is_slower() {
    for (kind, expected) in [("car", 92.9), ("tank", 50.9)] {
        let mut sim = flat(kind);
        // Keep the vehicle on the same homogeneous tile without a boundary stop.
        for _ in 0..6000 {
            sim.vehicles.x[0] = 0.0;
            sim.vehicles.y[0] = 0.0;
            sim.step(1, 0, 1).unwrap();
        }
        let kmh = sim.vehicles.speed[0] * 216.0;
        println!("{kind}: {kmh:.3} km/h on grass/coal, wiki {expected}");
        assert!((kmh - expected).abs() < 0.7);
        let forward = sim.vehicles.speed[0];
        sim.vehicles.speed[0] = 0.0;
        for _ in 0..6000 {
            sim.vehicles.x[0] = 0.0;
            sim.vehicles.y[0] = 0.0;
            sim.step(-1, 0, 1).unwrap();
        }
        assert!((sim.vehicles.speed[0] / forward + 0.5_f64.sqrt()).abs() < 0.001);
        let cruise = physics::terminal_speed(
            &sim.data,
            sim.vehicles.prototype[0],
            sim.vehicles.fuel[0],
            sim.data.tile_id("grass-1"),
            false,
        );
        assert!((forward - cruise).abs() < 1e-9);
    }
}

#[test]
fn fuel_multipliers_match_real_grass_trace_terminal_speeds() {
    let mut sim = flat("car");
    for (fuel, expected) in [
        ("solid-fuel", 106.896),
        ("rocket-fuel", 143.388),
        ("nuclear-fuel", 168.985),
    ] {
        sim.set_vehicle("car", fuel).unwrap();
        for _ in 0..6000 {
            sim.vehicles.x[0] = 0.0;
            sim.vehicles.y[0] = 0.0;
            sim.step(1, 0, 1).unwrap();
        }
        assert!(
            (sim.vehicles.speed[0] * 216.0 - expected).abs() < 0.8,
            "{fuel}"
        );
    }
}

#[test]
fn water_blocks_vehicle_center_on_straight_and_diagonal_approaches() {
    let mut sim = flat("car");
    for y in -32..-4 {
        for x in -32..32 {
            sim.world.set_tile(x, y, sim.data.tile_id("water"));
        }
    }
    for _ in 0..200 {
        sim.step(1, 0, 1).unwrap();
    }
    assert!(sim.vehicles.y[0] >= -4.0);
    assert!(!sim.world.water(
        sim.vehicles.x[0].floor() as i32,
        sim.vehicles.y[0].floor() as i32,
        &sim.data
    ));
    assert_eq!(sim.vehicle_state().last_collision.unwrap().kind, "water");
    // The calibrated game rule blocks the center tile on diagonal approaches too.
    sim.reset().unwrap();
    sim.vehicles.orientation[0] = 0.12;
    sim.vehicles.speed[0] = 2.0;
    sim.step(0, 0, 8).unwrap();
    let s = sim.vehicle_state();
    assert!(!sim
        .world
        .water(s.x.floor() as i32, s.y.floor() as i32, &sim.data));
}

#[test]
fn cliff_stops_car_and_tree_destruction_slows_tank() {
    let mut car = flat("car");
    car.world.spawn_cliff(&car.data, "west-to-east", 0.0, -6.0);
    car.step(1, 0, 180).unwrap();
    assert!(car.vehicles.y[0] > -3.6);
    assert_eq!(car.vehicle_state().last_collision.unwrap().kind, "cliff");
    assert!(car.world.alive(0));

    let mut tank = flat("tank");
    tank.world.spawn_entity(&tank.data, "tree-02", 0.0, -2.0);
    tank.vehicles.speed[0] = 0.22;
    let before = tank.vehicles.speed[0];
    let revision = tank.world.revision;
    tank.step(0, 0, 4).unwrap();
    assert!(!tank.world.alive(0));
    assert!(tank.vehicles.speed[0] > 0.0 && tank.vehicles.speed[0] < before);
    assert!(tank.world.revision > revision);
    assert_eq!(
        tank.vehicles.health[0], 2000.0,
        "base tank is immune to tree impacts"
    );
    assert!(tank
        .world
        .entities_in_rect([[-3.0, -5.0], [3.0, 1.0]])
        .is_empty());
}

#[test]
fn tank_all_impact_immunity_preserves_obstacle_damage_and_stopping() {
    for name in ["tree-02", "huge-rock", "biter-spawner", "cliff"] {
        let collide = |all_immunity: bool| {
            let mut sim = flat("tank");
            let index = sim.vehicles.prototype[0];
            let p = &mut sim.data.vehicles[index];
            assert!(
                p.immune_to_all_impacts,
                "2.1 base tank must have all-impact immunity"
            );
            p.immune_to_all_impacts = all_immunity;
            // All-impact immunity must work even when the specific flags are false.
            p.immune_to_tree_impacts = false;
            p.immune_to_rock_impacts = false;
            p.immune_to_cliff_impacts = false;
            let entity = if name == "cliff" {
                sim.world.spawn_cliff(&sim.data, "west-to-east", 0.0, -3.0)
            } else {
                sim.world.spawn_entity(&sim.data, name, 0.0, -3.0)
            };
            sim.vehicles.speed[0] = 0.1;
            let destroyed = fsim_core::collision::impact(
                &mut sim.world,
                &mut sim.vehicles,
                0,
                entity,
                &sim.data,
            );
            (sim, destroyed)
        };
        let (immune, destroyed) = collide(true);
        let (vulnerable, other_destroyed) = collide(false);
        assert_eq!(immune.vehicles.health[0], 2000.0, "{name}");
        assert!(vulnerable.vehicles.health[0] < 2000.0, "{name}");
        assert_eq!(
            immune.vehicle_state().last_collision.unwrap().damage_taken,
            0.0
        );
        assert_eq!(immune.world.health(0), vulnerable.world.health(0));
        assert_eq!(immune.vehicles.speed[0], vulnerable.vehicles.speed[0]);
        assert_eq!(destroyed, other_destroyed);
        if name == "tree-02" {
            assert!(destroyed);
            assert!(immune.vehicles.speed[0] > 0.0 && immune.vehicles.speed[0] < 0.1);
        } else if name == "cliff" {
            assert!(!destroyed);
            assert_eq!(immune.vehicles.speed[0], 0.0);
        }
    }
}

#[test]
fn brake_then_reverse_and_tank_turns_in_place() {
    let mut sim = flat("tank");
    sim.step(0, 1, 10).unwrap();
    assert!(sim.vehicles.orientation[0] > 0.0);
    assert_eq!((sim.vehicles.x[0], sim.vehicles.y[0]), (0.0, 0.0));
    sim.step(1, 0, 10).unwrap();
    assert!(sim.vehicles.speed[0] > 0.0);
    let mut saw_stop = false;
    for _ in 0..200 {
        sim.vehicles.x[0] = 0.0;
        sim.vehicles.y[0] = 0.0;
        sim.step(-1, 0, 1).unwrap();
        saw_stop |= sim.vehicles.speed[0] == 0.0;
        if sim.vehicles.speed[0] < 0.0 {
            break;
        }
    }
    assert!(saw_stop && sim.vehicles.speed[0] < 0.0);
    sim.set_vehicle("car", "coal").unwrap();
    sim.vehicles.orientation[0] = 0.0;
    sim.step(0, 1, 10).unwrap();
    assert_eq!(sim.vehicles.orientation[0], 0.0);
}

#[test]
fn real_nauvis_fixture_has_resources_obstacles_and_valid_source_spawn() {
    let sim = real("car");
    assert_eq!(sim.world.info.width, 128);
    assert_eq!(sim.world.info.name, "nauvis-seed-12345-origin");
    assert_eq!(sim.world.info.source_offset, Some([0, 0]));
    assert!(sim.world.resources.iter().any(|id| *id != 0));
    assert!(!sim.world.entities.is_empty());
    let state = sim.vehicle_state();
    assert!(!sim.world.blocked(
        Obb::new(state.x, state.y, state.collision_box, 0.0),
        &sim.data
    ));
}

#[test]
fn observations_have_real_semantics_and_coarse_averages() {
    let mut sim = flat("car");
    sim.world.spawn_entity(&sim.data, "tree-02", 2.5, -0.5);
    sim.world.set_tile(-2, -1, sim.data.tile_id("water"));
    sim.set_target(5.0, 0.0).unwrap();
    let local = sim.observation_local();
    let plane = 64 * 64;
    assert_eq!(local.len(), 8 * plane);
    // At origin, the local grid spans [-32,32). World (2,-1) is column 34, row 31.
    let tree = 31 * 64 + 34;
    assert_eq!(local[tree], 1.0);
    assert_eq!(local[2 * plane + tree], 1.0);
    let water = 31 * 64 + 30;
    assert_eq!(local[4 * plane + water], 1.0);
    assert_eq!(local[6 * plane + 32 * 64 + 32], 1.6);
    assert_eq!(local[7 * plane + 32 * 64 + 37], 1.0);
    let coarse = sim.observation_coarse();
    assert_eq!(coarse.len(), 8 * 32 * 32);
    // Cell (16,15) covers x=[0,8), y=[-8,0), and contains one tree cell.
    assert_eq!(coarse[2 * 32 * 32 + 15 * 32 + 16], 1.0 / 64.0);
    assert_eq!(observation::spec().channels.len(), 8);
    assert!(local.iter().chain(&coarse).all(|v| v.is_finite()));
}

#[test]
fn episode_lifecycle_validates_inputs_and_preserves_map_on_reset() {
    let mut sim = flat("car");
    sim.set_target(0.0, 0.0).unwrap();
    assert!(sim.target_state().unwrap().reached);
    assert!(sim.step(2, 0, 1).is_err());
    assert!(sim.step(1, 0, 0).is_err());
    assert!(sim.set_target(f64::NAN, 0.0).is_err());
    let revision = sim.world.revision;
    sim.step(1, 0, 3).unwrap();
    let position = (sim.vehicles.x[0], sim.vehicles.y[0]);
    sim.set_vehicle("tank", "nuclear-fuel").unwrap();
    assert_eq!((sim.vehicles.x[0], sim.vehicles.y[0]), position);
    assert_eq!(sim.vehicles.speed[0], 0.0);
    sim.reset().unwrap();
    assert!(sim.target.is_none());
    assert_eq!(sim.world.revision, revision);
    sim.load_map(REAL_MAP).unwrap();
    assert_eq!(sim.world.info.width, 128);
    assert!(sim.world.revision > revision);
    sim.vehicles.health[0] = 0.0;
    let before = sim.vehicle_state();
    sim.step(1, 1, 100).unwrap();
    assert_eq!(before, sim.vehicle_state());
}

#[test]
fn impact_damages_both_car_and_rock_and_respects_flat_resistance() {
    let mut sim = flat("car");
    sim.world.spawn_entity(&sim.data, "big-rock", 0.0, -2.5);
    sim.vehicles.speed[0] = 0.43;
    sim.step(0, 0, 3).unwrap();
    assert!(sim.vehicles.health[0] < 450.0);
    assert!(sim.world.health(0) < 500.0);
    assert!(sim.world.alive(0));
    assert_eq!(sim.vehicles.speed[0], 0.0);
    let r = vec![fsim_core::prototypes::Resistance {
        damage_type: "impact".into(),
        decrease: 5.0,
        percent: 0.0,
    }];
    assert_eq!(fsim_core::collision::resisted_damage(5.0, &r), 0.5);
    assert_eq!(fsim_core::collision::resisted_damage(7.0, &r), 2.0);
}

#[test]
fn reset_uses_source_spawn_then_closest_valid_integer_offset() {
    let mut sim = flat("car");
    sim.world.spawn = [3.0, 4.0];
    sim.reset().unwrap();
    assert_eq!((sim.vehicles.x[0], sim.vehicles.y[0]), (3.0, 4.0));
    sim.world.set_tile(3, 4, sim.data.tile_id("water"));
    sim.reset().unwrap();
    let state = sim.vehicle_state();
    let distance = (state.x - 3.0).powi(2) + (state.y - 4.0).powi(2);
    assert!(distance > 0.0);
    assert!(!sim.world.blocked(
        Obb::new(state.x, state.y, state.collision_box, 0.0),
        &sim.data
    ));
    for dy in -3..=3_i32 {
        for dx in -3..=3_i32 {
            if ((dx * dx + dy * dy) as f64) < distance {
                assert!(sim.world.blocked(
                    Obb::new(3.0 + dx as f64, 4.0 + dy as f64, state.collision_box, 0.0),
                    &sim.data
                ));
            }
        }
    }
    // A source origin far outside an exported rectangle still searches only
    // this map and finds the closest valid hull placement.
    sim.world.spawn = [1_000_000.0, 1_000_000.0];
    sim.reset().unwrap();
    let s = sim.vehicle_state();
    assert!(s.x > 29.0 && s.y > 29.0);
    assert!(!sim
        .world
        .blocked(Obb::new(s.x, s.y, s.collision_box, 0.0), &sim.data));
}
