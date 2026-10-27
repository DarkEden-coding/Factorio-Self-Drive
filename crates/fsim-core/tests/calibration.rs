//! Live Factorio 2.1.23 recordings. See docs/physics.md for provenance and gaps.
use fsim_core::{
    trace::{replay, Trace},
    Prototypes,
};
use std::{fs, path::Path};

#[test]
fn recorded_driving_and_impacts_replay_tick_for_tick() {
    let data = Prototypes::from_json(include_str!("../../../data/prototypes.json")).unwrap();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/traces");
    let mut paths: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    assert_eq!(
        paths.len(),
        38,
        "keep the representative fixture set intact"
    );
    let bytes: u64 = paths
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum();
    assert!(bytes < 3_000_000, "fixtures must stay below 3 MB");
    for path in paths {
        let trace: Trace = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let report = replay(&trace, &data).unwrap();
        let name = &report.name;
        assert_eq!(report.position.max, 0.0, "{name}: position");
        assert_eq!(report.orientation.max, 0.0, "{name}: orientation");
        assert_eq!(report.health.max, 0.0, "{name}: vehicle health");
        let speed_bound = match name.as_str() {
            "car-water-0.125" => 0.00006,
            "tank-water-0.125" => 0.000017,
            _ => 1e-12,
        };
        assert!(
            report.speed.max <= speed_bound,
            "{name}: speed {} > {speed_bound}",
            report.speed.max
        );
        // The world does not yet regenerate surviving trees. This records the
        // known gap instead of accepting a broad tolerance for impact damage.
        let obstacle_bound = if name == "car-impact-tree-02-0.2-0" {
            0.3
        } else {
            0.0
        };
        assert!(
            report.obstacle_health.max <= obstacle_bound,
            "{name}: obstacle health {} > {obstacle_bound}",
            report.obstacle_health.max
        );
    }
}
