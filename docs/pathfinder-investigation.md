# Pathfinder model investigation, 2026-10-27

The shipped `path-stop` model reliably arrives in the simulator but frequently hits trees.
Its route representation also directs the vehicle back to Factorio's path after a deviation.
The evidence supports changing route guidance and training together. Removing the path input
from the existing policy makes long trips much worse.

The production mod and model weights were left unchanged during this investigation.
The initial project snapshot is commit `21bcbf1`.

## What the model actually does

`StudentPolicy` is a reactive nine-action throttle/steering policy, evaluated every four ticks.
It receives 32 rays, four route lookahead points at 4/8/16/32 tiles, vehicle/target state, and a
16x16 coarse map. It does not output a planned path or compare candidate future trajectories.
The project's separate `path_planner` architecture is not what the mod runs.

`mod/factorio-autodrive/path.lua` and `crates/fsim-core/src/pathfinder.rs` construct the guidance as
vehicle position, nearest projection on the remaining waypoints, then later waypoints. This
makes the short lookaheads lead back toward the old route. There is no visibility or collision
check on the segment from the vehicle to that projection. Reprojection can also jump forward
across a route that doubles back, because it searches all remaining segments.

Reproduction with the production Lua modules, for a straight route from (0,0) to (100,0) and
a vehicle at (20,6), gives these world-coordinate lookaheads:

| Lookahead | Position |
|---|---|
| 4 tiles | (20,2) |
| 8 tiles | (22,0) |
| 16 tiles | (30,0) |
| 32 tiles | (46,0) |

The first point asks for a sideways return to the route even if the vehicle has a clear,
shorter diagonal toward a later waypoint. This confirms a bias in the input geometry; it
does not prove that the learned policy follows every such point.

The RL reward itself does not penalize distance from Factorio's waypoints. Progress uses a
separate target-rooted distance field (`rl_episode.rs`). Route dependence comes from the
observations, training data, and learned behavior rather than an explicit route-tracking loss.

## Fresh holdout measurements

Checkpoint `python/runs/students/path-stop/checkpoints/latest.pt`, step 7,995,392. Re-exporting
this checkpoint produces exactly the existing `path-stop.json` used by the Lua parity check.
Each variant runs 200 episodes per scenario, 16 environments, seed 12345, deterministic actions,
on CPU with one Torch thread. The six scenarios and three variants total 3,600 episodes.

`normal` uses the shipped inputs. `straight` replaces only the nine route features with a
straight line to the target. `no_map` sets the appended raw map channels to open ground and
keeps the rays and route. These input ablations are outside the training distribution. They
measure dependence and are not proposed deployment policies.

Full results, including vehicle breakdowns: [ablation JSON](results/path-stop-ablation-2026-10-27.json).

| Scenario | Input | Arrival success | SPL | Trips hitting trees | Tree contact ticks/trip | Nest-zone time |
|---|---|---:|---:|---:|---:|---:|
| standard | normal | 99.5% | 0.956 | 11.5% | 0.92 | 0.00% |
| standard | straight | 98.0% | 0.946 | 17.5% | 4.29 | 0.00% |
| standard | no_map | 98.0% | 0.942 | 12.0% | 0.73 | 0.26% |
| long | normal | 96.5% | 0.951 | 42.5% | 4.17 | 0.64% |
| long | straight | 54.5% | 0.542 | 74.5% | 139.53 | 1.08% |
| long | no_map | 96.5% | 0.956 | 43.5% | 4.38 | 1.26% |
| forest | normal | 100.0% | 0.945 | 76.5% | 10.22 | 0.00% |
| forest | straight | 100.0% | 0.967 | 90.0% | 26.46 | 0.00% |
| forest | no_map | 97.5% | 0.904 | 79.0% | 10.96 | 0.19% |
| reverse | normal | 97.5% | 0.871 | 15.0% | 2.16 | 0.65% |
| reverse | straight | 95.0% | 0.848 | 21.0% | 3.65 | 1.01% |
| reverse | no_map | 96.0% | 0.855 | 15.5% | 1.77 | 0.91% |
| nest | normal | 97.0% | 0.927 | 30.5% | 6.54 | 1.07% |
| nest | straight | 95.0% | 0.923 | 44.0% | 12.05 | 2.31% |
| nest | no_map | 91.5% | 0.881 | 33.0% | 8.09 | 11.56% |
| arrival | normal | 99.5% | 0.940 | 10.0% | 0.36 | 0.01% |
| arrival | straight | 98.5% | 0.931 | 15.0% | 1.47 | 0.00% |
| arrival | no_map | 99.0% | 0.933 | 8.5% | 0.15 | 0.18% |

The unmodified policy hits trees in 39 of 84 forest car trips (46.4%) and 114 of 116 forest
tank trips (98.3%). Both vehicle groups still have 100% arrival success. Contacts are simulator
collision ticks, not distinct trees or distinct impacts. Tank tree crushing is allowed by the
current simulator task, so strict tree avoidance needs its own evaluation requirement.

The shorter forest routes with straight guidance come with more tree contacts. SPL alone
therefore cannot define a better driving policy. Removing guidance reduces long-trip success
from 96.5% to 54.5%, while removing the map increases nest-zone time from 1.07% to 11.56%.
The map is useful; these results do not establish that it can independently optimize routes.

The inherited reward charges -0.5 per decision containing any contact, versus +0.1 per tile
of field progress. Multiple contact ticks in one decision still incur one penalty. Damage
penalties cannot enforce tree avoidance for an immune tank, and measured forest damage is
small. This tradeoff permits successful episodes with contacts. Raising the penalty is an
experiment to evaluate, not a demonstrated fix.

## Simulator and game limitations

- The training route source is a distance-field stand-in, not recorded Factorio paths. It
  dilates obstacles by rounded-up half-width and simplifies the route with a 1-tile geometric
  tolerance, without checking the resulting segments against collision geometry. The actual
  game uses the prototype collision box and its own pathfinding rules. Neither guidance
  implementation checks that a turning vehicle can execute each corner at its current speed.
- The model's coarse cells span 8x8 tiles, sampled at 16 points per cell. Narrow obstacles can
  be missed by that sampling; rays provide the more local detail. In-game map blocks also read
  as open ground until cached, whereas training has the map available immediately.
- Lua requests are asynchronous. The simulator answers immediately. The mod can drive on a
  straight-line fallback after two seconds, and continues on an old route while awaiting a
  replacement. Training does not reproduce those conditions.
- The mod's 60-tick request interval only guards the end-of-route branch. The >16-tile
  deviation branch bypasses it, so fast responses can trigger repeated requests every decision.
  This is a secondary mismatch, not a demonstrated cause of tree hits.
- The existing parity suite injects Rust's route into Lua. It verifies feature arithmetic and
  network inference; it does not verify that Factorio's route matches the training stand-in.

## Verification

All 49 Python tests pass, including two new ablation regression tests. On 16 fixed real-map
states, Lua and Rust observations match exactly and the exported network has identical logits
and actions. This rules out a translation discrepancy in those samples, not every runtime bug.

The production mod also completed all eight existing isolated Factorio 2.1.23 smoke trips,
with zero measured damage and zero blocked-powered-tick contact estimates. Mean decision
profile time was 3.44 ms, p95 4.21 ms, maximum 11.45 ms in this run. These measurements include
cache work performed in that tick. The fixtures are short and sparse, and the contact estimate
does not detect every glancing hit or a tree crushed without stopping. These are arrival smoke
tests, not a forest safety or route-optimality benchmark. Save/reload was not tested.

Engine results: [engine JSON](results/path-stop-engine-2026-10-27.json).

The engine runner now supports `--engine-only`, because its default simulator comparison still
runs the older dense-only model. The default engine parity check also compares against the
simulator's field route, so it cannot establish route parity for the current pathfinder model.

## Recommended implementation

1. Keep the global path as guidance, and replace the forced return-to-projection with a
   collision-checked connection to a useful point farther along it. Check the vehicle's swept
   footprint, trees, and zone costs before skipping waypoints. Implement the same semantics in
   Rust and Lua and retrain against them. Geometric simplification alone is insufficient.
2. Train on imperfect suggestions: detours, stale paths, delayed/missing answers, off-route
   starts, and safe alternatives to the suggestion. Keep useful global guidance available so
   long-range navigation does not collapse as it does in the straight-input ablation. Route
   dropout alone will not teach the policy to choose safe shortcuts.
3. Explicitly optimize tree avoidance and safe progress. Track collision-free arrival by
   vehicle, distinct tree impacts/destruction, minimum clearance, travel time, and excess
   safe-route length. Use those metrics for checkpoint comparisons alongside arrival success.
   Validate any stronger collision penalty on both car and tank before adopting it.
4. Add deterministic in-engine forest, tight-turn, and deliberately detoured-route fixtures,
   with tree-contact/destruction and path-length measurements. Compare local guidance changes
   and any predictive policy against the same initial states. A predictive actor is worth
   testing if the reactive student still cannot anticipate turns after these changes.

The immediate next experiment should combine safer forward route connections with route
variation during training. An inference-only change would feed the current weights inputs
they were not trained on, and the ablations show that can be costly.

## Reproduction

From the repository root:

```sh
cd python
uv run python ../scripts/student-ablation.py \
  runs/students/path-stop/checkpoints/latest.pt \
  ../data/autodrive/path-stop-ablation-200.json --episodes 200 --num-envs 16
uv run pytest -q
cd ..
python3 scripts/check-autodrive-parity.py
python3 scripts/test-autodrive.py --engine-only \
  --output data/autodrive/path-stop-engine --timeout 90
```

Training checkpoints, map pools, copied game graphics, and raw engine outputs are local ignored
artifacts. The snapshot commits the shipped Lua weights, source, configs, locks, prototype
snapshot, and test fixtures; it does not back up every training checkpoint.
