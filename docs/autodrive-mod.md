# In-game autodrive

Version 0.7.1 hides diagnostic lines by default and removes engine verification
fixtures from the gameplay mod. Enable **Show autodrive diagnostic lines** in
**Settings > Mod settings > Per player** to display them. Existing saves retain
previously saved preferences.

Release checks on Factorio 2.1.23 passed on 2026-10-28: Lua syntax and API-name
audit, overlay default and runtime toggle checks, planner regression scripts,
and 14 Python script tests including ZIP contents and reproducibility. The
isolated `tank-obstacles40` engine fixture arrived with zero damage and produced
identical results after save/reload. Interactive settings UI and multiplayer
were not exercised by these checks.

Version 0.7.0 extends and refines a cached any-angle corridor while driving. It retains the
`field-smooth-v1` controller, 135 inputs and spec hash `d1c084cc679f1f71`. The destination and
trip clock stay fixed across field handoffs. On the paired long-trip suite, the planner
reduces total time and driven distance modestly, with unchanged completion counts and some
individual regressions. See [progressive-route-results.md](progressive-route-results.md)
for the complete measurements and limits.

The model export is `python/runs/students/field-smooth-v1/field-smooth.json`, format
`fsim-student-v1`, with 13,312 multiply-adds per decision. Inference runs entirely in Factorio
Lua and requires Factorio 2.1.23 or newer in the 2.1 series.

## Controls

1. Sit in the driver's seat of a base car or tank and supply fuel.
2. Click **Set autodrive target** or press Ctrl+Alt+T.
3. Click or select a destination in the world or on the map. Map/remote view uses
   `LuaPlayer.physical_vehicle`. Targets must be on the same surface, in a generated chunk,
   and within 4096 tiles. A blocked target snaps to drivable ground within 8 tiles.
4. The vehicle brakes while preparing the first local field, then drives toward the target.
   Arrival requires the center within 1 tile and absolute speed at most 0.01 tiles/tick.

Movement keys return manual control. Ctrl+Alt+D resumes with fresh planning. Selecting a new
target also resumes. `/autodrive <x> <y>` sets exact coordinates; `/autodrive stop` returns
manual control. Leaving the vehicle or disconnecting cancels driving.

The overlay draws the final stop ring in green, the verified rolling corridor in blue, the coarse
Factorio path in grey, and the four student lookaheads in amber. Profiling writes local timings
to `script-output/factorio-autodrive/timing.txt`; `/autodrive-dump` writes a diagnostic snapshot.

## Routing and model

The restored driver computes bounded, resumable distance fields in Lua. Water, rocks, cliffs,
and nests block travel. Trees block the car and cost extra for the tank. Nest zones add cost,
so the local route can choose a detour rather than follow Factorio's waypoints directly.
After extraction, cost-aware string pulling replaces clear spans with arbitrary-angle segments.
Shortcuts must clear the vehicle-inflated grid and retain the original span's terrain-cost budget.
The 135 model inputs contain rays, smoothed-route lookaheads and length, vehicle state, and target
state. The model has no coarse-map convolution. It chooses throttle and steering every four
ticks; brake/reverse mapping is updated each tick as speed crosses zero.

Trips within 64 tiles use one field with a 48-tile margin. Longer trips start with a 48-tile
section, then extend overlapping sections of roughly 160 tiles toward a soft 768-tile horizon,
retaining at most six solved sections. Factorio's pathfinder suggests distant sub-targets;
verified cost-aware shortcuts can skip those endpoints. Refinements preserve the next 24 tiles
and reconsider the distant tail every 120 ticks and when another section becomes ready.

Per tick, terrain preparation builds at most four cache blocks and one worker advances at
most 4,000 field work units. Consumed sections and terrain behind the vehicle are pruned.
Destruction/mining events trigger selective repairs while conservative old guidance remains
available. Frontier braking prevents driving past verified guidance while extension catches up.
These are deterministic work limits, with no wall-clock scheduling or hard millisecond guarantee.

The controller still reads 4/8/16/32-tile lookaheads, now across section boundaries, and the real
final destination. Longer model lookaheads and a new actor architecture were not needed for this
planner change. Diagnostic dumps include `route_continues` and `verified_horizon`; these are
planner metadata, not additional network inputs. Axis-aligned ray traversal now handles exact
tile boundaries consistently in Lua and Rust.

Version 0.7.0 clears incompatible saved drive state and brakes controlled vehicles during
configuration changes. Set a target again after loading the update. Restart Factorio to load
changed mod files.

Regenerate the checked-in weights and geometry with:

```sh
python3 scripts/build-autodrive-weights.py
```

## Verification for the current planner

See [progressive-route-results.md](progressive-route-results.md) for paired long engine trips,
short regression fixtures, save/reload replay, geometry checks and observation parity.
The older model-training comparisons remain in [smooth-route-results.md](smooth-route-results.md)
and [efficient-driving-results.md](efficient-driving-results.md).

```sh
python3 scripts/test-autodrive.py --engine-only --suite long --save-reload --timeout 300
python3 scripts/test-autodrive.py --engine-only --output data/autodrive/progressive-short
lua scripts/check-progressive-route.lua
lua scripts/check-progressive-driver.lua
```

The default simulator comparison is a standalone episode using each snapshot's final destination.
It does not reproduce the asynchronous rolling planner. Use the engine-only long suite to compare
complete long trips, including planning and waits.

The earlier pathfinder investigation is preserved in [pathfinder-investigation.md](pathfinder-investigation.md).
Historical measurements below describe 0.1.0 and do not describe the current planner.

## Verification (0.1.0 field-route model)

The small engine suite creates disposable surfaces and saves in an isolated
write-data directory. It runs car and tank through flat 15-tile drives, mixed
obstacle/water/cliff 40-tile drives, 100-tile nest detours, and 60-tile real
Nauvis drives. Coal, rocket fuel and nuclear fuel are covered, with stopped,
forward-moving and reversing starts. Nest spawning is disabled only in the
verification companion. No user save is opened or changed.

```sh
python3 scripts/check-autodrive-parity.py
python3 scripts/test-autodrive.py
```

The engine runner checks `pmset -g batt` before each engine start and pauses below
20% while discharging. `--allow-low-battery` is an explicit override, used for
this task after the user authorized it. Each engine child is terminated and
reaped on success, failure or timeout. `--keep-temp` retains diagnostic files.

The parity script uses the existing Rust/WASM simulator, so no Rust API changes
or retraining are needed. It checks observations, field values and action
agreement on fixed real-map states, then compares engine snapshots against the
same terrain exported to `.fsmap`. It compares Lua logits against `student_ref.py`.

Results below describe the completed engine runs.

### Measured results, 2026-10-27

All eight engine fixtures stopped in the center successfully. The same eight
initial-state simulator episodes also succeeded. These are smoke tests, not an
estimate of general success rate. All engine drives had zero health damage and
zero detected blocked powered ticks. Factorio exposes no collision event here;
the contact count is a movement/input proxy, not an exact engine collision count.

| Case | Engine seconds | Simulator seconds | End distance, tiles | End speed, tiles/tick |
|---|---:|---:|---:|---:|
| car-flat15 | 1.55 | 1.55 | 0.509 | 0.00000 |
| car-obstacles40 | 2.85 | 2.93 | 0.701 | 0.00000 |
| car-nest100 | 5.27 | 5.22 | 0.668 | 0.00000 |
| car-real60 | 4.13 | 4.13 | 0.525 | 0.00000 |
| tank-flat15 | 2.95 | 2.95 | 0.659 | 0.00946 |
| tank-obstacles40 | 6.23 | 5.62 | 0.498 | 0.00000 |
| tank-nest100 | 9.40 | 9.25 | 0.902 | 0.00967 |
| tank-real60 | 7.85 | 7.85 | 0.425 | 0.00000 |

Driving time excludes planning. Initial planning took 314-580 ticks, about
5.2-9.7 seconds at 60 UPS. Short targets still require a margin-sized field, so
this delay is visible. The car holds brake during it.

Parity results:

| Comparison | States | Hard/tree/zone max abs | Route max abs | State max abs | Action agreement |
|---|---:|---:|---:|---:|---:|
| Lua vs Rust/WASM on fixed real-map poses | 16 | 0 | 0 | 0 | 16/16 |
| Engine Lua vs Rust/WASM on exported fixture terrain | 8 | 5.96e-8 | 2.87e-17 | 6.13e-17 | 8/8 |

Offline distance-field values matched exactly. Lua/reference logit max absolute
difference was zero offline and 1.11e-16 on engine snapshots. The real-map cases
include stopped, forward-moving and reversing states. Engine parity is checked
at the initial scripted states, not at every tick of a drive.

LuaProfiler results on this Mac, with local file writing outside the measured
interval. The route measurement includes cache preparation and field slices.
The decision measurement includes route extraction, observation and forward pass.

| Work | Samples | Mean ms | p95 ms | Max ms |
|---|---:|---:|---:|---:|
| decision, isolated run | 606 | 0.711 | 0.839 | 2.102 |
| route, isolated run | 3419 | 0.427 | 0.982 | 2.140 |
| decision, subsequent run with concurrent checks | 606 | 1.517 | 2.543 | 9.298 |
| route, subsequent run with concurrent checks | 3419 | 0.718 | 2.108 | 17.688 |

The isolated decision mean and p95 are comfortably below 2 ms. Occasional
samples exceeded 2 ms, and concurrent checks increased the tail substantially.
There is no hard 2 ms worst-case guarantee. Timings are diagnostic only and do
not change the deterministic work budget.

Evidence is in `data/autodrive/`: `offline-parity.json`, `isolated-timing.json`,
`isolated-timing.txt`, and `engine/` with initial snapshots, `.fsmap` fixtures,
`parity-report.json`, `driving-report.json`, `simulator-comparison.json` and logs.

Checks passed: all mod Lua syntax; installed Factorio 2.1.23 API-name audit;
11 existing Python script tests; existing Lua mod checks covering 140 scenarios;
existing WASM smoke checks. Rust was unchanged. Boolean settings are checked
against a small local schema because Factorio omits settings prototype definitions
from its JSON APIs. API auditing is a name/record check, not a type checker.

### Limits and remaining checks

- Supported driving domain is generated Nauvis terrain and unmodified base car/tank geometry. Player structures, moving units, equipment, quality/research bonuses and other mods are outside the trained simulator. All currently burning fuels are read through the runtime API, but the engine suite exercised only coal, rocket fuel and nuclear fuel.
- Fields are bounded and remain fixed for a trip, as in the simulator. Live ray masks update after destruction. New structures, tile edits and newly grown obstacles do not trigger a field rebuild. Set the target again to replan.
- GUI shortcut clicks, linked movement-key takeover, overlays and actual multi-client play still need a manual smoke test. Runtime names and prototypes loaded successfully in the engine; that does not verify those interactions.
- Save/load has not been verified end to end. An extra private-server checkpoint attempt failed because its disposable saves directory was missing. The runner now creates that directory and offers `--save-reload`, but that extra test was not rerun after the request to finish. Production state uses serializable storage tables and entity handles; profiler objects are never stored.
- The model can still become pinned against rocks or cliffs, as documented in `student-results.md`. Timeout or stall ends the attempt and holds brake until manual input or a new target.

## Installation and release packaging

Build the standalone mod portal upload:

```sh
python3 scripts/package-autodrive.py
```

Copy `dist/factorio-autodrive_0.7.1.zip` into Factorio's `mods` directory or upload
it to the mod portal. Remove duplicate unpacked copies of the same mod from
your local `mods` directory before installing the ZIP. Restart Factorio after
changing mod files. See the [player guide](../mod/factorio-autodrive/README.md).

The release contains runtime Lua, generated weights and geometry, locale, player
documentation and changelog. Engine test fixtures stay under
`scripts/fixtures/autodrive/` and are injected only into the disposable mod copy
created by `scripts/test-autodrive.py`.
