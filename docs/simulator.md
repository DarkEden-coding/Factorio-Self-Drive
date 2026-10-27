# Rust simulator

The Rust workspace implements the simulator API in [sim-api.md](sim-api.md). `fsim-core` has no wasm or Python dependencies. `fsim-wasm` exposes the `Sim` class and embeds the prototype snapshot.

## Build and run

From the repository root:

```sh
cargo test
cargo run --release -p fsim-core --bin benchmark
cargo install wasm-pack --locked  # only if wasm-pack is missing
export PATH="$HOME/.cargo/bin:$PATH"
wasm-pack build crates/fsim-wasm --target web --out-dir pkg
node scripts/check-wasm.mjs
```

On this machine, the Rust linker needs its installed LLVM library on the macOS search path. If a build reports `Library not loaded: @rpath/libLLVM.dylib`, set this before running the same build command:

```sh
export DYLD_LIBRARY_PATH="$(rustc --print sysroot)/lib${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
wasm-pack build crates/fsim-wasm --target web --out-dir pkg
```

The wasm target is `wasm32-unknown-unknown`. If needed on another machine, install it with `rustup target add wasm32-unknown-unknown`. The generated package is `crates/fsim-wasm/pkg`, including the JS initializer, TypeScript declarations and `fsim_wasm_bg.wasm`. `node scripts/check-wasm.mjs` initializes the web-target package from bytes without a browser or server, then checks the actual JS API and typed arrays.

```ts
import init, { Sim } from "../crates/fsim-wasm/pkg/fsim_wasm.js";
await init();
const map = new Uint8Array(await (await fetch("/data/game/maps/nauvis-seed-12345.fsmap")).arrayBuffer());
const sim = new Sim(JSON.stringify({ vehicle: "car", fuel: "coal" }), map);
sim.step(1, 0, 60);
console.log(JSON.parse(sim.vehicle_state()));
```

Construction requires real map bytes and accepts vehicle/fuel options, defaulting
to car and coal. Unknown config keys, invalid JSON, unknown names, invalid actions,
zero ticks, nonfinite target coordinates, and invalid rectangles throw a JS
exception. Reset finds a valid hull position nearest the source origin and clears
the target. It does not resurrect destroyed obstacles. Vehicle swaps preserve
position and orientation exactly, even if the larger tank hull overlaps an
adjacent obstacle. Such a swap may require reversing away or resetting.

## RL environment

```sh
cargo run --release -p fsim-core --bin rl-benchmark            # data/game/maps; add --quick for a short run
uv venv /tmp/fsim-py-venv --python 3.12
VIRTUAL_ENV=/tmp/fsim-py-venv uv pip install maturin numpy
(cd crates/fsim-py && /tmp/fsim-py-venv/bin/maturin develop --release)
/tmp/fsim-py-venv/bin/python crates/fsim-py/tests/smoke.py
```

`crates/fsim-py` is a PyO3 0.30 + numpy crate built with maturin as an abi3 wheel for Python 3.12 or newer (`import fsim`). The trainer in `python/` depends on it as a path dependency. A plain `cargo build` of the workspace also compiles it and links libpython; maturin builds the importable extension. `rl-benchmark` reports scenario sampler acceptance and reset cost per kind, distance-field, observation and reset cost, VecEnv decisions/sec per mode and env count, and A* baseline success on the train split. `--scenarios-only` stops after the scenario table.

Collect maps with `scripts/collect-game-data.sh`. The default pool has 16
1056 × 1056 maps. Use `--map-count`, `--map-radius`, or `--seeds` to configure it.
See [map-format.md](map-format.md) for collection, native MapPool signatures,
spawning and the binary file format. The benchmark reads a real pool map,
defaulting to `data/game/maps`; an optional positional argument selects another
pool directory or index. Tests embed a 128 × 128 real Nauvis fixture.

## Code layout

- `prototypes.rs` loads and validates the normalized prototype snapshot.
- `world.rs` stores tile/resource arrays, obstacle states and a spatial hash. `geometry.rs` handles oriented boxes.
- `map_import.rs` reads name-based chunk exports, re-centers coordinates, retains the source offset and reports prototype fallbacks. `trace.rs` reconstructs calibration scenarios and calculates replay errors; `bin/replay-trace.rs` is its command-line entry point. See [mod.md](mod.md).
- `map_format.rs` loads compact real `.fsmap` files. `map_pool.rs` reads an index and picks maps by index or seeded PCG32 sampling. There is no procedural terrain generator.
- `vehicle.rs` stores vehicle position, orientation, speed, health, prototype/fuel ids, ticks and collision metadata in separate flat arrays. `physics.rs` advances an indexed vehicle, so a future batch API can reuse it with separate worlds.
- `collision.rs` applies impact damage and obstacle destruction. See [physics.md](physics.md) for sourced rules and calibration assumptions.
- `target.rs` derives distance and arrival; `episode.rs` owns configuration, reset/swap/step and target lifecycle for the app.
- `world.rs` shares tiles, entities and the spatial hash through `Arc`, with live entity health per world. Cloning a loaded world copies only health, so RL envs reset cheaply on shared maps.
- RL environment (contract in [rl.md](rl.md)): `rl_config.rs` (config and episode rules), `raster.rs` (per-map u8 semantic raster plus per-world patches for destroyed entities), `distance_field.rs` (bucket-queue Dijkstra), `sampler.rs` (start/target sampling), `reward.rs`, `rl_episode.rs` (episode state, termination, reward bookkeeping), `policy_obs.rs` (u8 crops, vector features, `spec`), `controller.rs` (pure pursuit and `path_planner` plans), `baseline.rs` (A* on the field) and `vec_env.rs` (batched envs, rayon behind the default `parallel` feature, records). `fsim-wasm` builds `fsim-core` without `parallel`.
- `observation.rs` provides the actual training grids, reused by wasm without a frontend approximation.

## Observation semantics

Channel order, shapes, typed arrays and units follow the binding contract. Both grids align to world axes and are centered exactly on the vehicle. A local cell covers a one-tile square; tile channels sample the tile containing its center. The even-sized local grid spans `[x-32,x+32)` and `[y-32,y+32)`. Entity channels mark every observation cell whose square intersects the entity's oriented collision box.

- `blocked` is the union of water, out-of-map, trees, rocks, cliffs, and nest collision boxes. It is not expanded by the vehicle hull.
- `cliff`, `tree`, `rock`, `water` are binary occupancy channels.
- `nest_zone` is 1 at cell centers within a live nest's penalty radius, otherwise 0. Overlapping zones do not add together.
- `friction` is the raw tile `vehicle_friction_modifier`, not normalized and not adjusted for the current vehicle. Out-of-map uses 1.
- `target` is 1 at cell centers within the target's three-tile arrival circle, otherwise 0.

Coarse observations first rasterize the same semantics at one-tile resolution over a 256x256-tile square, then average each 8x8 block to a 32x32 grid. Channels are stored contiguously, then rows, then columns. No hidden resizing or frontend channel generation is required. Current observations contain no additional vehicle-number vector beyond `vehicle_state()`, `vehicle_list()`, and `fuel_list()` because the binding contract does not specify one.

Map revisions change on map load, tile edits, or obstacle destruction. Damage without destruction does not change the revision. Rectangle queries filter destroyed entities and test actual oriented-box intersection. Cliffs use health/max_health 0 because they have no health in the game. Palette id zero is reserved for no ore. All original prototype values and runtime-export limitations are described in [prototypes-schema.md](prototypes-schema.md).
