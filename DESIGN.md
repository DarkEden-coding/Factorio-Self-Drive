# Factorio self-drive

RL agents that drive the car and tank in Factorio 2.1.23 (Nauvis), trained in a fast custom simulator and later run in the real game through a mod.

## Settled decisions

- Goal: models run in the real game. The simulator must match game behavior closely.
- Factorio 2.1.23 / Space Age, Nauvis only for now.
- Task: drive from A to a target without destroying the vehicle.
- Terrain: cliffs, trees, rocks, water and deep water, biome tiles, ore patches, biter nests. No player structures.
- Tiles matter for driving: each tile has a `vehicle_friction_modifier`.
- Biter nests are static obstacles with a penalty zone. No biter AI.
- Tank plows through trees, losing speed but taking no driving-impact damage. Car mostly can't.
- Vehicle health is simulated. Losing all health ends the episode.
- One shared policy for car and tank, conditioned on vehicle and fuel stats. Fuel type varies. No equipment grid.
- Physics values come from the game's prototype data, and get checked against traces recorded in the real game.
- Maps: real default-Nauvis maps exported by Factorio for training and evaluation. The simulator has no procedural generator. The local viewer uses the game's own graphics through a generated asset manifest.

## Agent (framework built, not trained yet; contract in docs/rl.md)

- Observation: 64x64 local tile grid around the vehicle with semantic channels, a coarse wider grid toward the target, and vehicle state numbers.
- Actions: throttle {accelerate, none, brake/reverse} x steer {left, none, right} = 9 discrete actions. Decision every 4 ticks by default, configurable.
- Architectures, switchable behind one interface:
  1. Path planner: waypoints up to ~100 tiles ahead, replanned every 0.5 s or on drift, followed by a fixed pure-pursuit controller.
  2. Direct control with planning ahead: outputs 1-2 s of inputs, executes a few, replans. Predicted path is drawn.
  3. Direct control, reactive: one action per step.
- Classical A* + pure pursuit baseline for comparison only. All training is RL (PPO).
- Rewards: progress along a distance field that routes around cliffs, water and nest zones, arrival bonus, per-tick penalty, collision and damage penalties, nest-zone penalty, terminal penalty on destruction.
- Curriculum on target distance, plus a weighted scenario mix for targeted training: long routes, forests, reverse-first starts, nest zones.
- PyTorch (MPS on Mac, CUDA later). Python pinned to 3.12 with uv.

## Real game

- `mod/fsim-autodrive` runs the trained small student entirely in Lua. A cached obstacle raster and zone-aware distance field supply the simulator observation. See `docs/autodrive-mod.md` for controls and measured results.
- Target set by selection-tool click or console command.
- Pressing a movement key gives the player control. A hotkey hands it back.
- Target and path drawn in game with the rendering API.

## Layout

- `crates/fsim-core`: simulator library. Prototype data, real-map loading and pools, world, physics, collision, observations. Data stored as flat arrays per field so batching and a later GPU port stay possible.
- `crates/fsim-wasm`: wasm-bindgen bindings used by the playable app. Contract in `docs/sim-api.md`.
- `crates/fsim-py`: PyO3 bindings with batched stepping (`import fsim`). Contract in `docs/rl.md`.
- `app/`: playable browser app (Vite + TypeScript + Canvas). Same simulator code as training.
- `data/prototypes.json`: vehicle, fuel, tile, and entity values from the game. The export mod regenerates it.
- `mod/fsim-export/`: Factorio 2.1.23 mod that dumps prototypes, records driving traces, and exports map chunks.
