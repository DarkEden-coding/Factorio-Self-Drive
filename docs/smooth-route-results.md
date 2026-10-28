# Arbitrary-angle field routes, 2026-10-27

Mod 0.6.0 emits arbitrary-angle route guidance and ships the `field-smooth` model tuned to it.
An open destination 18.4 degrees from the horizontal now gets one direct segment, rather than
a staircase of diagonal and horizontal waypoints. The distance field still supplies global
connectivity; the route presented to the model uses cost-aware line-of-sight shortcuts.

## Route construction

Both Rust training and Factorio Lua first extract the existing field route from the current
vehicle position. String pulling replaces spans with direct segments when they satisfy both:

- The segment clears the field's vehicle-inflated obstacle grid. Supercover tile traversal
  checks corner and border touches as well as the cells crossed by the line.
- Its integrated travel, tree and nest-zone cost does not exceed the cost of the original span.

The algorithm tries the target first, then progressively closer route points if the shortcut
is obstructed or too costly. Tight sections can retain original waypoints. The emitted angles
have no fixed increment. This is a piecewise straight guide, not a curvature-constrained
trajectory; the learned controller still handles acceleration and turning with the game's
physics. Native heading snapping near axes and the four-tick decision interval are unchanged.

The same algorithm and operation order are implemented in `route_smoothing.rs` and
`route-smoothing.lua`. The field retains its cost/clearance flags for these checks. Training
continues to use the original field for reachability, progress rewards and task sampling, so
changing the guidance does not change which start/target tasks are sampled.

Field parameters now travel with the exported model instead of being implicit in the mod.
This also aligns the runtime nest-zone cost with training: the selected checkpoint uses 4,
where the old mod hard-coded 3. Trees still block the car and cost extra for the tank.

The input remains 135 features and the actor remains 64x64 with nine control actions. The
new guidance has its own observation hash, `d1c084cc679f1f71`. A grid checkpoint can explicitly
initialize smooth training with the same feature layout, weights and normalizer, but cannot
resume under the wrong guidance contract. The viewer handshake selects the matching WASM
observation, applies the model's field costs and shows the smooth guidance in the plan overlay.

## Geometry measurements

On 400 fixed real-map routes, covering two maps, both vehicles and 100 seeds:

| Measurement | Grid guide | Smooth guide |
|---|---:|---:|
| Total guide length | 22,926.77 tiles | 21,949.30 tiles |
| Total waypoint count | 20,279 | 1,367 |
| Distance more than 1 degree from a multiple of 45 degrees | 2.36% | 80.76% |

Guide length is 4.26% shorter. These are guide geometry measurements, not driven distances or
proof of a globally shortest any-angle path. [Full geometry results](results/smooth-route-geometry-2026-10-27.json).

## Training and selection

The supplemental RTX 5060 run completed 5,996,544 decisions in approximately 455 seconds,
starting from the 0.5.0 checkpoint at `field-efficient-v1/checkpoints/iter_000200.pt`. The run
uses `student_mlp64_smooth.toml`, learning rate 5e-5, the existing scenario/vehicle/fuel mix,
and the same time, traveled-distance, collision, damage and zone rewards as the efficiency run.

Validation used training maps and separate seed 54321, 200 episodes per scenario. Comparisons
included unchanged weights with smooth guidance, intermediate tuned checkpoints and the final
checkpoint. The final checkpoint was selected for its faster arrivals, shorter paths, fewer
contacts and much lower damage than the intermediate tuned checkpoints, with similar success.
The guidance-only comparison isolates the input change and is not the shipped checkpoint.

[Training provenance](results/smooth-route-training-2026-10-27.json) records hashes and selection
measurements. Local artifacts are in `python/runs/students/field-smooth-v1/`; the generated Lua
weights are committed with the mod. The completed checkpoint is `checkpoints/latest.pt` and
the export is `field-smooth.json`.

## Holdout results against 0.5.0

The main holdout uses seed 67890, 400 episodes per scenario, totaling 2,400 trips per policy.
The second holdout uses seed 12345, 200 per scenario. Start poses, initial speeds, fuels, targets,
arrival rules, field costs and time limits are matched exactly. Task geometry is unchanged.
Time and path comparisons use common successes; safety and success include every episode.

| Scenario | Grid success | Smooth success | Time change | Driven path change |
|---|---:|---:|---:|---:|
| standard | 100.00% | 100.00% | -2.74% | -0.82% |
| long | 100.00% | 99.50% | -1.92% | -1.61% |
| forest | 99.50% | 99.75% | -2.36% | -0.09% |
| reverse | 98.75% | 99.25% | -3.36% | -0.87% |
| nest | 98.75% | 99.75% | -2.15% | -1.12% |
| arrival | 99.75% | 100.00% | -3.73% | -0.92% |

On 2,384 common successes, mean time falls from 7.671 to 7.492 seconds, **2.34% faster**.
Mean driven path falls from 109.646 to 108.165 tiles, **1.35% shorter**. Paired bootstrap 95%
intervals are -0.208 to -0.151 seconds and -1.641 to -1.332 tiles. They treat episodes as
independent and do not measure training-seed or new-world uncertainty.

Across all main-holdout trips, success increases from 99.46% to 99.71%, collision-free success
from 73.04% to 74.13%, and tree-hit trips decrease from 24.67% to 23.83%. Mean damage falls
from 0.54 to 0.37 health points. Moving decision samples with headings more than 1 degree off
the 45-degree axes increase from 79.86% to 87.18%. The old policy could already drive at other
angles; the new guide increases their use rather than unlocking forbidden vehicle headings.

The second seed gives 2.38% faster arrivals, 1.29% shorter paths, and success increasing from
99.25% to 99.50%. Gains are not uniform: long-trip success in the main set falls by two episodes,
forest tree-hit rates rise slightly, and mean damage increases on the second seed. The guide
checks do not guarantee collision-free learned control.

- [Main holdout, seed 67890](results/smooth-route-holdout-67890-2026-10-27.json)
- [Second holdout, seed 12345](results/smooth-route-holdout-12345-2026-10-27.json)

## Factorio verification

Both versions completed all twelve isolated Factorio 2.1.23 fixtures with no measured damage
or detected blocked-powered-tick contacts. The suite includes a new open 18.4-degree destination
for both vehicles. The initial guide changes from 62 points to exactly the start and target:
`(-96,80) -> (-36,100)`. Both vehicles drive the angle case with non-grid headings throughout
their measured moving ticks.

| Total over twelve fixtures | 0.5.0 | 0.6.0 | Change |
|---|---:|---:|---:|
| Driving ticks | 3,952 | 3,806 | -3.69% |
| Planning plus driving ticks | 4,488 | 4,342 | -3.25% |
| Traveled distance | 764.880 tiles | 753.796 tiles | -1.45% |

These are smoke fixtures, not a general success estimate. Contact detection is a proxy and can
miss glancing hits or tree crushing. Summaries retain the explicit angle-case routes; full raw
outputs remain in `data/autodrive/smooth-engine-before/` and `smooth-engine-after/`.

Mean profiled decision work is 0.87 ms, p95 1.09 ms in this run, versus 0.79 ms and 0.91 ms before.
Smoothing adds some computation. Timings include work in that tick and are diagnostic, with no
hard worst-case guarantee. Save/reload and multi-client play were not tested.

- [Old engine run](results/smooth-route-engine-before-2026-10-27.json)
- [Smooth engine run](results/smooth-route-engine-after-2026-10-27.json)
- [Parity report](results/smooth-route-parity-2026-10-27.json): all field values, route features,
  other observations, logits and 16 actions match Rust/reference exactly on fixed real-map
  states, including moving and reversing starts.

Checks passed: 51 Rust tests, 63 Python/script tests plus five subtests, WASM smoke checks with
the smooth observation/path API, TypeScript checking, Lua smoothing/grid regressions and the
installed Factorio API audit. Geometry tests cover open arbitrary-angle routes, obstacle and
expensive-zone detours, and blocked corner/border touches. The training test permits an explicit
compatible warm start and rejects resuming a grid checkpoint as a smooth policy.

## Reproduce

From `python/`, with the locally generated maps and previous training artifacts:

```sh
OMP_NUM_THREADS=2 MKL_NUM_THREADS=2 uv run fsim-rl train \
  --config configs/student_mlp64_smooth.toml \
  --init-from runs/students/field-efficient-v1/checkpoints/iter_000200.pt \
  --run-name students/field-smooth-v1
uv run python ../scripts/benchmark-students.py ../data/autodrive/smooth-holdout.json \
  grid=runs/students/field-efficient-v1/checkpoints/iter_000200.pt \
  smooth=runs/students/field-smooth-v1/checkpoints/latest.pt --seed 67890 --episodes 400
uv run fsim-rl export-student --checkpoint runs/students/field-smooth-v1/checkpoints/latest.pt \
  --out runs/students/field-smooth-v1/field-smooth.json
```

From the repository root:

```sh
python3 scripts/build-autodrive-weights.py
scripts/build-wasm.sh
node scripts/benchmark-route-smoothing.mjs data/autodrive/smooth-route-geometry.json
python3 scripts/check-autodrive-parity.py
lua scripts/check-route-smoothing.lua
python3 scripts/test-autodrive.py --engine-only --output data/autodrive/smooth-engine-after
```

For the viewer, serve `python/runs/students/field-smooth-v1/checkpoints/latest.pt` with
`fsim-rl serve` and rebuild/reload the app. Its plan overlay shows the guide the model receives;
the optional A* overlay remains the raw grid baseline for comparison.
