# In-game autodrive

`mod/fsim-autodrive` (0.3.0) runs the `path-stop` student entirely in Factorio Lua: the
pathfinder-route map student (`configs/student_map_path_stop.toml`) fine-tuned to stop in the
middle of the target. It needs Factorio 2.1.23 or newer in the 2.1 series. No external policy
server is used. Model export: `python/runs/students/path-stop/path-stop.json`, format
`fsim-student-v2`, spec hash `a38878852fab9d85`, about 62,000 multiply-adds per decision.

Holdout suite (200 episodes per scenario, stop-in-target rules), against the field-route
`mlp64-stop` the mod shipped before:

| Scenario | mlp64-stop (0.1-0.2) | path-stop (0.3) |
|---|---:|---:|
| standard | 99.0% | 99.5% |
| long | 98.5% | 96.5% |
| forest | 99.5% | 100% |
| reverse | 98.5% | 97.5% |
| nest | 98.0% (zone time 0.02%) | 97.0% (zone time 1.1%) |
| arrival | 99.5% | 99.5% |

## Controls

1. Sit in the driver's seat of a base car or tank and supply fuel.
2. Click **Set autodrive target** in the shortcut bar, or press Ctrl+Alt+T.
3. Click or select the destination in the world or on the map (map/remote view works:
   the mod reads `LuaPlayer.physical_vehicle`). The target must be on the vehicle's
   surface, in a generated chunk, and at most 4096 tiles away. A target on water, a
   rock or (for the car) a tree moves to the nearest drivable tile within 8 tiles;
   otherwise the trip is reported unreachable at once.
4. Driving starts once the blocks under the rays are cached and Factorio's pathfinder
   (`LuaSurface.request_path`) has answered, usually a fraction of a second. If no path
   arrives within 2 seconds the vehicle heads straight for the target until one does.
   Arrival means the center is within 1.0 tile and absolute speed is at most 0.01
   tiles/tick. The vehicle keeps braking after arrival.

Movement keys immediately hand control back. Ctrl+Alt+D resumes toward the saved
target with a fresh path. Selecting a new target also resumes. `/autodrive <x> <y>`
sets an exact world coordinate; `/autodrive stop` returns manual control. Passengers
cannot control the driver. Leaving the vehicle or disconnecting cancels the controller.

The per-player **Show autodrive route and target** setting draws a green 1-tile stop
ring, the blue route the student reads, the game's waypoints in grey and four amber
lookahead points, in the world and on the map. The global profiling setting writes
local timings to `script-output/fsim-autodrive/timing.txt`. Decisions occur every four
ticks; the acceleration/brake/reverse mapping is updated each tick as signed speed
crosses zero. `/autodrive-dump` writes the current observation, logits, route,
waypoints, cache and state under `script-output/fsim-autodrive/`.

## Implementation

- `path.lua` follows the game's waypoints as the simulator's stand-in does in training
  (`crates/fsim-core/src/pathfinder.rs`, docs/rl.md "Pathfinder route"): the route is the
  vehicle, its projection on the waypoints (never moving back along them), then the later
  waypoints and the target. It asks for a new path when the vehicle is more than 16 tiles
  off the waypoints, or on their last segment more than 16 tiles from the target (at most
  every 60 ticks). Requests are asynchronous, unlike the simulator: until an answer
  arrives the old waypoints, or a straight line, stand in. Failed requests are retried
  after 5 seconds (1 second for `try_again_later`). The game's pathfinder ignores nest
  zones; the map keeps the vehicle out of them.
- `observation.lua` builds the 135 student features (32 rays x hard/tree/zone, route
  lookaheads at 4/8/16/32 tiles and route length, vehicle state) and the coarse map: 16x16
  cells of 8x8 tiles in the vehicle frame, the fraction of 4x4 sample points per cell that
  are hard, tree and zone. Map blocks not cached yet read as open ground.
- `network.lua` normalizes, runs the two zero-padded stride-2 map convs (3->8->16 channels)
  and the 64x64 dense layers in exactly the reference's accumulation order
  (`python/src/fsim_rl/student_ref.py`). The first maximum wins ties.
- `raster.lua` uses oriented collision boxes, strict SAT overlap, tile-center nest zones of
  radius 18, and water/void masks. Cliff geometry comes from the pinned 2.1.23 prototype
  snapshot, including intrinsic orientation.
- `grid.lua` caches 16x16 blocks: the 36 tiles around the vehicle the rays need are urgent
  (the vehicle brakes until they are built), the 92 tiles the map covers fill in the
  background. 12 blocks per tick while stopped, 4 while driving. Blocks are invalidated on
  death, mining and raised script destruction, and pruned away from the vehicle.
- Progress is the remaining route length: stall is 1,200 ticks without a 1-tile
  improvement; timeout is 600 + 12 x the first route length in driving ticks, extended when
  a new path arrives. Both end the trip and hold the brake.
- All control, cache and path state lives in `storage`. Profilers remain local and never
  affect control. No clock, OS, external process or random sampling is used in play.

Regenerate the checked-in model and geometry modules with:

```sh
python3 scripts/build-autodrive-weights.py
```

### Offline checks for 0.3.0

- `scripts/check-autodrive-parity.py`: Lua vs Rust/WASM (`policy_student_map`) on 16 fixed
  real-map states (car and tank, stopped, moving and reversing), given the Rust route. Rays,
  route features, state and map all differ by 0; Lua logits match `student_ref.py`
  exactly; 16/16 actions agree.
- Plain Lua 5.5 on the Mac: about 1.25 ms for rays and map plus 1.0 ms for the network per
  decision. Factorio's Lua is likely somewhat slower. A plain-Lua trip harness with stubbed
  engine calls and toy kinematics (not Factorio physics) drove 40-950 tile trips: driving
  started 14 ticks after the target was set, every trip with a path stopped 0.06-0.74 tiles
  from the center, and the slowest tick was 3.7 ms.
- `check-mod-api.py` passes against the installed 2.1.23 docs; `check-mod.lua` passes.

Not yet done for 0.3.0: an engine run. The engine suite below (`test-autodrive.py`) and its
simulator comparison were written for the field-route model; its simulator side still drives
the dense mlp64-stop student, so it needs updating before it can verify 0.3.0.

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

## Installation

Installed and enabled as `~/Library/Application Support/factorio/mods/fsim-autodrive_0.3.0`,
a symlink to this repository (earlier version links removed). Factorio was not running when enabling the mod.
All existing mod-list entries were preserved. Backup:

`~/Library/Application Support/factorio/mods/mod-list.json.autodrive-backup-20261027-142319`

No user saves were touched. All started Factorio processes were terminated and reaped.
No commits were made.
