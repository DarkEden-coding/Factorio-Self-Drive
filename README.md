# Factorio self-drive

Autodrive mod, simulator and RL training tools for the Factorio car and tank. See [DESIGN.md](DESIGN.md).

## Autodrive mod

The standalone mod supports Factorio 2.1.23 or newer in the 2.1 series.
See [controls and settings](mod/fsim-autodrive/README.md) and
[planner documentation](docs/autodrive-mod.md). Diagnostic lines are hidden by
default and can be enabled in per-player mod settings.

Build the mod portal upload with:

```sh
python3 scripts/package-autodrive.py
```

The ZIP is written to `dist/fsim-autodrive_0.7.1.zip`. It contains the standalone
mod only, including Lua weights, locale, player documentation and changelog.
Upload it through the Factorio mod portal, or copy it into your Factorio `mods`
directory. The simulator, training tools, test fixtures and copied game assets
are excluded.

## Play the simulator

```sh
cd app
npm install
npm run dev
```

`npm run dev` rebuilds the wasm package (`scripts/build-wasm.sh`) and starts Vite. Open the printed URL.
Keys: click sets a target, WASD drives (overriding the driver while held), R picks a new start, T toggles game
graphics, H shows all keys.

The simulator only runs real Factorio maps. Generate the map pool and the game graphics once with
`scripts/collect-game-data.sh` (needs a local Factorio install). The app first loads the first map in
`data/game/maps/index.json`. Pick another map from the Map list in the panel, or use "Load map file" for a
local `.fsmap`. Vite serves the pool at `/data/game/maps/` in dev and preview, and `npm run build` copies it
into `dist/data/game/maps/`.

Game graphics come from `app/public/game-assets/` (gitignored, copied from your own Factorio install, so
`npm run build` also copies them into `dist/`; do not publish that). Without that directory the app shows the
debug palette view and a hint. Rebuild the graphics alone with `python3 scripts/build-game-assets.py`.

## RL viewer

The viewer is one live session. It loads a map and places the car at a random valid start pose (the
training sampler's rules). Click the map (left or right button; a drag pans in free-camera mode) to set a
target: the vehicle keeps its pose, speed and health, and the episode bookkeeping (distance field from the
current position, time limit, stall timer, reward totals, trail) starts over for the new target (wasm
`retarget`). Clicking again mid-drive retargets at once. A blocked or cut-off spot shows UNREACHABLE and keeps
the previous target.

The driver picked in the panel then drives there:

- **Policy** (default): connects to `fsim-rl serve` at `ws://localhost:8765` on load. The status line shows
  the server's `hello`; "Reconnect" tries again. The viewer refuses to drive if the server's `spec_hash`
  differs from the wasm build. Without a server the car brakes and waits, and the panel shows how to start it:
  ```sh
  cd python && uv run fsim-rl serve --checkpoint runs/students/field-smooth-v1/checkpoints/latest.pt  # current smooth-route student
  cd python && uv run fsim-rl serve --checkpoint runs/ft-strong/best/model.pt                  # full model
  ```
  Student checkpoints announce `observation: "student"` in `hello`, and the viewer then sends the compact
  student observation (`policy_student()`) instead of the grids.
  Episodes use the server's mode: `reactive` asks for every decision, `direct_plan` runs the first `execute`
  actions of each plan and draws the whole plan, `path_planner` follows the waypoints until the controller
  asks to replan.
- **A\* baseline**: an A* path down the distance field followed by the pure-pursuit controller
  (`baseline_path` + `follow_path`).
- **Manual only**: the vehicle moves only on WASD.

On reaching the target the vehicle brakes to a stop and REACHED shows until the next click. On timeout or
stall it stops too; a click retries from where it is. A destroyed vehicle needs R. Holding any drive key
(WASD / arrows) overrides the driver while held; on release the driver continues toward the current target.
R places the vehicle at a new random pose (full health, no target), also after picking another vehicle.

Episode overlays (toggles under View): trail, plan (waypoints or predicted path, the executed part in white),
A* path and the distance-field heatmap. The Policy panel shows the action probabilities, value, last action,
path distance and reward terms. While driving to a target the observation viewer shows the policy's u8 grids
and named vector features.

## Tests

```sh
export DYLD_LIBRARY_PATH="$(rustc --print sysroot)/lib"   # rust-lld workaround on this Mac
cargo test
node scripts/check-wasm.mjs
cargo run --release -p fsim-core --bin benchmark
cargo run --release -p fsim-core --bin rl-benchmark
```

The RL environment's Python module (`import fsim`) builds with maturin; see
[docs/simulator.md](docs/simulator.md#rl-environment).

## Docs

- [docs/sim-api.md](docs/sim-api.md): simulator API used by the app
- [docs/rl.md](docs/rl.md): RL environment contract (episodes, rewards, observations, Python and wasm APIs)
- [docs/physics.md](docs/physics.md): vehicle and collision formulas, with what still needs calibration
- [docs/prototypes-schema.md](docs/prototypes-schema.md): `data/prototypes.json`
- [docs/simulator.md](docs/simulator.md): core layout
- [docs/mod.md](docs/mod.md): the export mod, trace recording and replay, map export
