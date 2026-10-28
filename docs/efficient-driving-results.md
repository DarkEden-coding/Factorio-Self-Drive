# Efficient field driving, 2026-10-27

Mod 0.5.0 keeps field-route guidance and ships `field-efficient`, a supplemental PPO fine-tune
of `mlp64-stop`. It improves arrival time, produces a small reduction in total travel distance,
and increases collision-free arrivals on the held-out maps. This is an incremental improvement,
not a replacement for global path planning or a claim that tree collisions are solved.

## Planner changes

The Lua distance field now uses unit-width circular buckets instead of a binary heap. Every
edge costs at least 1 and at most sqrt(2) times 8, so nodes in a distance bucket cannot relax
one another; a 14-slot ring safely separates future buckets. Direct row/column neighbor
indexing also removes repeated coordinate conversion and bounds helper calls. The completed
float32 fields remain identical to the previous implementation and the Rust simulator.
All queue state remains serializable, and work is still bounded per tick.

Cache construction now queries trees, rocks and cliffs within an 8-tile border. Only nest
spawners use the wider 26-tile query needed for danger zones. This avoids repeatedly collecting
and rasterizing irrelevant forest entities for each 16x16 block. The pinned base collision
boxes, including intrinsic cliff orientations, fit within the smaller border. A regression
check compares every mask cell against the original broad query, including edge-crossing
obstacles and a nest outside the smaller query.

The model inputs and architecture remain unchanged: 135 features, dense layers 64x64, nine
control actions, 13,312 MACs per decision, student hash `49e75f8a02cb7e16`. The route, lookaheads,
obstacle clearance and nest-zone costs retain their previous meanings. The speedup comes from
computing the fields more cheaply and learning more efficient control.

Seven-repeat standalone Lua benchmark, median CPU milliseconds, against commit `1d6ff06`:

| Segment distance | Field cells | Before | After | Reduction |
|---|---:|---:|---:|---:|
| 48 | 16,875 | 24.77 ms | 18.23 ms | 26.4% |
| 160 | 43,425 | 64.00 ms | 47.96 ms | 25.1% |
| 256 | 75,250 | 112.09 ms | 83.74 ms | 25.3% |

Every field value is checked against the old implementation. This benchmark uses a fixed
obstacle mask and host Lua, not Factorio APIs. Engine latency also includes cache construction
and deterministic work scheduling, so its speedup differs.

## Supplemental training and checkpoint selection

The RTX 5060 run completed 7,995,392 new decisions in approximately 585 seconds. It initialized
from the old model's weights and normalizer with a fresh optimizer. PPO used 256 environments,
128 steps per rollout, learning rate 5e-5 decaying linearly, and entropy coefficient 0.003.
Training used the train map split and retained the original vehicle/fuel and scenario mix.

A new `reward.path_length` term charges actual displacement at each physics tick, including
reverse travel. It defaults to zero, preserving old configs and rewards. The supplemental
config uses -0.02 per traveled tile, -0.02 per decision, and -0.75 per decision with a contact.
Damage and nest-zone penalties remain enabled. Arrival still requires stopping within 1 tile
with absolute speed at most 0.01 tiles/tick.

Checkpoints were compared on training maps with separate seed 54321, 200 episodes per scenario.
Iteration 200, at 6,553,600 supplemental decisions, was selected for its balance of shorter
paths, faster arrivals, higher success, fewer contacts and less zone time. The final checkpoint
was slightly faster on validation but had more nest contacts and zone time. Holdout results
were not used to choose between these checkpoints.

[Training provenance and validation results](results/field-efficient-training-2026-10-27.json)
include checkpoint/export SHA-256 hashes, rewards, and selection measurements. The selected
checkpoint and JSON export remain locally available in `python/runs/students/field-efficient-v1/`;
the generated Lua actor is checked into the mod.

## Held-out comparison

The main comparison uses seed 67890 and 400 episodes per scenario, totaling 2,400 trips per
policy. Both policies use identical tasks, including maps, exact start poses, initial speed,
fuel, target, arrival rules, field costs and time limits. Timing is simulated game time.
Distance/time comparisons use only trips both policies complete; failures and safety metrics
are reported over every trip. A second check uses the established seed 12345, 200 per scenario.
The two comparisons total 7,200 policy rollouts.

Main holdout results:

| Scenario | Old success | Efficient success | Time change | Path length change |
|---|---:|---:|---:|---:|
| standard | 99.25% | 100.00% | -2.33% | -0.40% |
| long | 98.75% | 100.00% | -0.50% | -0.28% |
| forest | 98.25% | 99.50% | -6.05% | +0.68% |
| reverse | 97.25% | 98.75% | -3.25% | -0.07% |
| nest | 98.00% | 98.75% | -1.82% | -0.43% |
| arrival | 99.50% | 99.75% | -2.78% | -0.48% |

On the 2,359 common successes, mean time falls from 7.806 to 7.667 seconds, **1.78% faster**.
Mean path falls from 109.970 to 109.663 tiles, **0.28% shorter**. The paired mean differences
have bootstrap 95% intervals of -0.176 to -0.104 seconds and -0.405 to -0.207 tiles. The bootstrap
treats episodes as independent; it does not quantify uncertainty from training seed or new
world distributions.

Across all 2,400 trips, arrival success rises from 98.50% to 99.46%, collision-free success from
69.58% to 73.04%, and trips touching trees decrease from 27.33% to 24.67%. Tree contact ticks per
trip fall from 3.39 to 1.99. Mean damage increases from 0.40 to 0.54 health points, so reduced
contact frequency does not mean every remaining impact is gentler. Forest paths are 0.68%
longer in this seed while completion time and tree contacts improve. Gains are not uniform.

The second seed gives 1.50% faster arrivals, 0.42% shorter paths, and success increasing from
98.83% to 99.25%. It also has a small increase in mean damage and zone time. Both reports retain
per-scenario and per-vehicle measurements, including failed episodes:

- [Main holdout, seed 67890](results/field-efficient-holdout-67890-2026-10-27.json)
- [Second holdout, seed 12345](results/field-efficient-holdout-12345-2026-10-27.json)

## Factorio verification

The old 0.4.0 mod and new 0.5.0 mod drove the same ten isolated Factorio 2.1.23 fixtures. These
cover car and tank, three fuels, moving/reversing starts, water/rock/cliff avoidance, nest trips,
real Nauvis, and a new deterministic forest detour. Both versions completed all ten with no
measured damage or detected blocked powered ticks. The proxy does not detect every glancing
contact or a tree crushed without stopping.

| Total over ten fixtures | Old | New | Change |
|---|---:|---:|---:|
| Initial planning ticks | 486 | 434 | -10.70% |
| Driving ticks | 3,300 | 3,216 | -2.55% |
| Planning plus driving ticks | 3,786 | 3,650 | -3.59% |
| Travel distance | 635.008 tiles | 634.746 tiles | -0.04% |

These sparse fixtures are smoke tests, not an estimate of general driving performance. Some
individual trips are slower. The engine benchmark now keeps full-trip time and traveled
distance separately from the per-segment clock read by the policy. Its artificial pause for
map export is excluded. Older reports using the segment clock undercounted segmented trips.

- [Old engine run](results/field-efficient-engine-before-2026-10-27.json)
- [New engine run](results/field-efficient-engine-after-2026-10-27.json)
- [Offline parity](results/field-efficient-parity-2026-10-27.json): every field value and all 135
  features match Rust exactly on 16 real-map states; logits and all 16 actions match the
  exported reference exactly.

Checks passed: 48 Rust tests across the workspace, 62 Python/script tests plus five subtests,
WASM smoke checks, Lua grid-mask regression, installed Factorio API audit, and the ten engine
trips. The modified reward regression explicitly covers forward and reverse travel. Long-trip
plain-Lua checks also completed with the new actor and segmented planner; their simplified
kinematics are not an engine result. Save/reload and multi-client play were not tested.

## Reproduce

From `python/`, with locally generated map pools and training artifacts:

```sh
OMP_NUM_THREADS=2 MKL_NUM_THREADS=2 uv run fsim-rl train \
  --config configs/student_mlp64_efficient.toml \
  --init-from runs/students/mlp64-stop/checkpoints/latest.pt \
  --run-name students/field-efficient-v1
uv run python ../scripts/benchmark-students.py ../data/autodrive/efficient-holdout.json \
  old=runs/students/mlp64-stop/checkpoints/latest.pt \
  efficient=runs/students/field-efficient-v1/checkpoints/iter_000200.pt \
  --seed 67890 --episodes 400
uv run fsim-rl export-student \
  --checkpoint runs/students/field-efficient-v1/checkpoints/iter_000200.pt \
  --out runs/students/field-efficient-v1/field-efficient.json
```

From the repository root:

```sh
python3 scripts/build-autodrive-weights.py
python3 scripts/check-autodrive-parity.py
lua scripts/check-grid-lua.lua
python3 scripts/test-autodrive.py --engine-only --output data/autodrive/efficient-engine-after
```

For the CPU field benchmark, extract `mod/fsim-autodrive/field.lua` from commit `1d6ff06` to a
local file and pass it as the first argument to `lua scripts/benchmark-field.lua`, followed by
the current `mod/fsim-autodrive/field.lua`.
