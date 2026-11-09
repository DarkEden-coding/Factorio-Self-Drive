//! Space Age terrain: blocking tiles, slow ground in the distance field,
//! demolisher territory as zone tiles, planet cliffs and mixed map pools.
use fsim_core::{
    distance_field::{friction_extra, DistanceField, FieldParams, Region},
    map_import,
    raster::{MapRaster, NEST_ZONE, WATER},
    rl_config::{FieldConfig, PoolChoice, RlConfig},
    rl_episode::in_nest_zone,
    world::World,
    Prototypes,
};

fn data() -> Prototypes {
    Prototypes::from_json(include_str!("../../../data/prototypes.json")).unwrap()
}

fn tile(data: &Prototypes, name: &str) -> u8 {
    data.tiles.iter().find(|t| t.name == name).unwrap().id
}

#[test]
fn lava_and_deep_lakes_block_while_wetland_and_oil_ocean_are_slow_ground() {
    let data = data();
    for name in ["lava", "lava-hot", "gleba-deep-lake"] {
        assert!(
            data.tiles[tile(&data, name) as usize].is_water,
            "{name} blocks"
        );
    }
    for (name, friction) in [
        ("wetland-blue-slime", 8.0),
        ("oil-ocean-deep", 10.0),
        ("fulgoran-rock", 4.0),
    ] {
        let t = &data.tiles[tile(&data, name) as usize];
        assert!(!t.is_water, "{name} is drivable");
        assert_eq!(t.vehicle_friction_modifier, friction);
    }
}

#[test]
fn friction_cost_routes_around_slow_ground_only_when_enabled() {
    let data = data();
    // A 64x64 world of volcanic soil (friction 1) with a 30-tile band of oil
    // ocean (friction 10, 2.5x the car's grass travel time) across the straight
    // line and a gap at its east end. Crossing costs about 45 extra tiles, the
    // detour through the gap about 31.
    let mut world = World::empty(2, 1, tile(&data, "volcanic-soil-dark"));
    let oil = tile(&data, "oil-ocean-deep");
    for y in -15..15 {
        for x in -32..20 {
            world.set_tile(x, y, oil);
        }
    }
    let raster = MapRaster::build(&world, &data);
    let car = &data.vehicles[data.vehicle_index("car").unwrap()];
    let region = Region::around(&[[0.5, -20.5], [0.5, 20.5]], 30, &raster);
    let plain = FieldParams::new(car, &FieldConfig::default());
    let priced = FieldParams::new(
        car,
        &FieldConfig {
            friction_cost: 1.0,
            ..FieldConfig::default()
        },
    );
    let route = |params| {
        let field = DistanceField::compute(&raster, region, params, [0.5, 20.5], f64::INFINITY);
        field.route(0.5, -20.5, [0.5, 20.5])
    };
    let crosses = |route: &[[f64; 2]]| route.iter().any(|p| p[0] < 20.0 && p[1].abs() < 15.0);
    assert!(
        crosses(&route(plain)),
        "without friction cost the route goes straight through"
    );
    assert!(
        !crosses(&route(priced)),
        "with friction cost it takes the gap"
    );
    // The cost follows the vehicle's terrain_friction_modifier: the tank slows less.
    let tank = &data.vehicles[data.vehicle_index("tank").unwrap()];
    let car_cost = friction_extra(10.0, car.terrain_friction_modifier, 1.0);
    let tank_cost = friction_extra(10.0, tank.terrain_friction_modifier, 1.0);
    assert!(car_cost > 1.0 && tank_cost > 0.0 && tank_cost < car_cost);
    assert_eq!(friction_extra(1.0, car.terrain_friction_modifier, 1.0), 0.0);
}

#[test]
fn demolisher_territory_marks_zone_tiles() {
    let data = data();
    let mut world = World::empty(4, 1, tile(&data, "volcanic-soil-dark"));
    world.info.source_offset = Some([0, 0]);
    world.set_territory_chunks(&[[0, 0]]);
    let raster = MapRaster::build(&world, &data);
    assert_ne!(raster.mask_at(5, 5) & NEST_ZONE, 0);
    assert_eq!(raster.mask_at(-5, 5) & NEST_ZONE, 0);
    assert_eq!(raster.mask_at(5, 5) & WATER, 0);
    assert!(in_nest_zone(&world, &raster, 31.9, 0.1));
    assert!(!in_nest_zone(&world, &raster, 32.1, 0.1));
}

#[test]
fn planet_cliffs_use_their_own_prototype() {
    let data = data();
    // Load through the JSON interchange, as map exports do.
    let mut chunk = serde_json::json!({"position": [0, 0], "tiles": vec!["volcanic-soil-dark"; 1024],
        "resources": [], "entities": []});
    for (name, x) in [("cliff-vulcanus", 8.0), ("crater-cliff", 24.0)] {
        chunk["entities"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"name": name, "kind": "cliff",
            "position": [x, 8.5], "cliff_orientation": "west-to-east"}));
    }
    let json =
        serde_json::json!({"schema_version": 1, "area": [[0, 0], [32, 32]], "chunks": [chunk]});
    let (world, report) = map_import::load_map(&json.to_string(), &data).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    for (index, name) in ["cliff-vulcanus", "crater-cliff"].into_iter().enumerate() {
        let expected = data.cliff(name).unwrap().orientations["west-to-east"].collision_box;
        assert_eq!(world.entities[index].collision_box, expected);
        assert_eq!(world.entities[index].name, name);
    }
    // Crater cliffs are wider than the Nauvis cliff.
    assert!(world.entities[1].collision_box[1][0] > world.entities[0].collision_box[1][0]);
}

#[test]
fn map_pool_weights_are_validated() {
    let mut config = RlConfig {
        map_pools: vec![
            PoolChoice {
                pool: "a".into(),
                weight: 0.0,
            },
            PoolChoice {
                pool: "b".into(),
                weight: 0.0,
            },
        ],
        ..RlConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    assert!(RlConfig::from_json(&json).is_err());
    config.map_pools[1].weight = 2.0;
    let parsed = RlConfig::from_json(&serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(parsed.pools(), vec![("a".into(), 0.0), ("b".into(), 2.0)]);
}
