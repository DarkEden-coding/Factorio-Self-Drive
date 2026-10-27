# Simulator API for the playable app

`crates/fsim-wasm` exposes a `Sim` class through wasm-bindgen. It is built with
`wasm-pack build crates/fsim-wasm --target web --out-dir pkg` and imported by `app/`
from `crates/fsim-wasm/pkg`. The app must only use what this file lists. Changes to
this contract go in this file first.

## Coordinates and units

- Factorio conventions. 1 tile = 1 meter. `x` grows east, `y` grows south.
- Map is centered on the origin: tiles cover `x` in `[-width/2, width/2)` and `y` in `[-height/2, height/2)`.
  Tile `(tx, ty)` covers `[tx, tx+1) x [ty, ty+1)`.
- Orientation is Factorio's: a float in `[0, 1)`, 0 = north, 0.25 = east, clockwise.
- Speed is tiles per tick (signed, negative = reversing). km/h = speed * 216.
- The simulation runs at 60 ticks per second.

## Constructing

```ts
const index = await (await fetch("/data/game/maps/index.json")).json();
const map = new Uint8Array(await (await fetch(
  `/data/game/maps/${index[0].file}`,
)).arrayBuffer());
const sim = new Sim(JSON.stringify({ vehicle: "car", fuel: "coal" }), map);
```

Exact TypeScript signatures are `new Sim(config_json: string, map: Uint8Array)`
and `sim.load_map(map: Uint8Array): string`. Config contains only `vehicle`
(`"car" | "tank"`) and `fuel` (a name from `fuel_list()`). Both are optional and
default to `"car"` and `"coal"`. Unknown config keys throw. There is no procedural
map API, regenerate method, or map-size/seed configuration. A map's seed is read
only provenance.

`map` is a complete compressed `.fsmap` file. See [map-format.md](map-format.md)
for its binary layout and pool index. The frontend must expose the local pool at
`/data/game/maps/`; it is generated outside `app/public/` to avoid duplicate files.
The constructor loads the supplied real map and spawns the selected vehicle.

`sim.load_map(map)` returns JSON
`{ "offset": [16,16], "warnings": [], "name": "nauvis-seed-12345" }`.
Factorio coordinates equal simulator coordinates plus `offset`. Unknown tile,
resource, entity, or cliff-orientation names use semantic physics fallbacks and
appear in the warning list. Original entity names are retained for graphics.
The method preserves vehicle/fuel selection, resets the vehicle, clears the
target, and increments both map revisions. Invalid files or maps with no valid
vehicle spawn throw and leave the current episode unchanged. The constructor
throws on those same errors. Maximum area is 16,777,216 tiles, each dimension at
most 8192. `map_info()` always includes `name` and `source_offset` for a loaded
map. Width and height may differ.

The preferred spawn is Factorio's origin translated by `source_offset`, even
when this is not the simulator's center. If blocked, reset uses the nearest valid
integer-tile offset by Euclidean distance. See [map-format.md](map-format.md) for
tie-breaking. The hull faces north at spawn.

`sim.reset()` respawns the vehicle at a valid spawn point on the current map with full health and clears the target.
`sim.set_vehicle(kind, fuel)` swaps the vehicle in place (same position and orientation, full health, zero speed).

## Stepping

```ts
sim.step(throttle, steer, ticks) // throttle, steer in {-1, 0, 1}; ticks >= 1
```

- `throttle`: 1 = accelerate (W), -1 = brake, then reverse once stopped (S), 0 = coast.
- `steer`: -1 = left (A), 1 = right (D), 0 = straight.
- The same input is applied for `ticks` ticks. The app calls this with ticks = 1 from a fixed 60 Hz loop and larger values for fast-forward.
- Stepping a destroyed vehicle does nothing.

## Reading state

All structured results are returned as JSON strings (parse with `JSON.parse`) unless noted.

`sim.vehicle_state()`:

```json
{
  "kind": "car", "fuel": "coal",
  "x": 0.0, "y": 0.0, "orientation": 0.0, "speed": 0.0,
  "health": 450.0, "max_health": 450.0,
  "destroyed": false,
  "tick": 0,
  "collision_box": [[-0.7, -1.0], [0.7, 1.0]],
  "last_collision": null
}
```

`last_collision` is `null` or `{ "tick": 123, "entity_id": 42, "kind": "tree", "damage_taken": 12.5 }` for the most recent collision.
`kind` is an entity kind, or `"water"` (entity_id 0) when water or the map edge stopped the vehicle.
Fuel is unlimited, so there is no fuel level.
`collision_box` is the unrotated box relative to the vehicle center, as in the prototype.

`sim.target_state()`: `null` or `{ "x": 10.0, "y": -4.0, "radius": 3.0, "distance": 12.3, "reached": false }`.
`sim.set_target(x, y)` places a target. `sim.clear_target()` removes it.

## Map data for rendering

- `sim.map_info()`: `{ "name": "nauvis-seed-12345", "width": 1056, "height": 1056, "x_min": -528, "y_min": -528, "seed": 12345, "source_offset": [16,16] }`.
- `sim.tile_palette()`: array indexed by tile id: `{ "id": 0, "name": "grass-1", "color": [r, g, b], "vehicle_friction_modifier": 1.0, "is_water": false }`.
  Colors are each tile prototype's `map_color`.
- `sim.tiles()`: `Uint8Array` of tile ids, row-major, length `width * height`, index `(ty - y_min) * width + (tx - x_min)`.
- `sim.resources()`: `Uint8Array` with the same layout. 0 = no ore, otherwise an index into `sim.resource_palette()` (`{ "id", "name", "color" }`, id 0 unused).
- `sim.entities_in_rect(x0, y0, x1, y1)`: entities whose collision box intersects the rectangle:

```json
[{
  "id": 42, "kind": "tree", "name": "tree-02",
  "x": 10.5, "y": 3.2,
  "collision_box": [[-0.4, -0.4], [0.4, 0.4]],
  "orientation": 0.0,
  "cliff_orientation": null,
  "health": 50.0, "max_health": 50.0
}]
```

  `collision_box` is relative to `(x, y)`. The box is rotated by `orientation` about its own center, which stays at `(x, y)` plus the unrotated box center. Cliff boxes come from their cliff orientation, and `orientation` is that box's own rotation (0 or 0.125).
  `kind` is one of `"tree"`, `"rock"`, `"cliff"`, `"nest"`. Cliffs set `cliff_orientation` to the Factorio name (for example `"west-to-east"`) and use the collision box of that orientation. Destroyed entities are not returned. Nests also include `"penalty_radius"`.
- `sim.map_revision()`: `u32` that increases when tiles or the entity set changes (a map is loaded, or an entity is destroyed). The app re-fetches entities when it changes.
- `sim.tile_revision()`: `u32` that increases only when tiles change. The app rebuilds its tile image when it changes.

## Observations

- `sim.observation_spec()`: `{ "local_size": 64, "coarse_size": 32, "coarse_cell_tiles": 8, "channels": ["blocked", "cliff", "tree", "rock", "water", "nest_zone", "friction", "target"] }`.
- `sim.observation_local()`: `Float32Array`, channel-major, `channels * local_size * local_size`. Grid is aligned to world axes (north up) and centered on the vehicle's exact position, not snapped to its tile. Cell `(row, col)` covers `[x - 32 + col, x - 31 + col) x [y - 32 + row, y - 31 + row)`. Row 0 is the north edge.
  Channels are 0 or 1, except `friction`, which is the raw tile `vehicle_friction_modifier` (1.4 dirt, 1.6 grass and red desert, 1.8 sand on Nauvis). `blocked` is set for any obstacle, water, or off-map cell.
- `sim.observation_coarse()`: `Float32Array`, same channels, `channels * coarse_size * coarse_size`, centered on the vehicle, each cell averaging `coarse_cell_tiles x coarse_cell_tiles` tiles.

These f32 grids are exact-position observations for the app. RL policies use the u8 tile-snapped grids from `policy_local`/`policy_coarse` (see [rl.md](rl.md#observation)), cropped from a cached raster with the same cell tests.

## RL episodes

`Sim` also runs RL episodes with the same Rust code as the training environment:
`start_episode`, `sample_episode`, `retarget`, `place_random`, `episode_state`, `policy_local`,
`policy_coarse`, `policy_vector`, `spec`, `decide`, `follow_path`, `plan_waypoints`,
`predict_path`, `baseline_path`, `distance_field` and `distance_field_values`. Their contract is
in [rl.md](rl.md#wasm-additions-sim). Starting an episode restores destroyed obstacles, moves the
vehicle to the start pose and sets the target. `reset`, `set_vehicle`, `load_map` and
`place_random` end the episode. The methods above keep working during an episode.

The viewer uses the live-session pair:

- `sim.retarget(x, y, radius?, rules_json?)`: JSON. Sets the target and starts an episode from the
  vehicle's current state (pose, speed and health kept; distance field, time limit and reward
  bookkeeping restart). Returns `{reachable: true, optimal_distance, time_limit_ticks, ...episode}`
  or `{reachable: false}`, in which case nothing changes.
- `sim.place_random(seed)`: JSON `{x, y, orientation}`. A random valid start pose (the training
  sampler's start rules, random heading), full health, zero speed; restores destroyed obstacles,
  clears the target and ends the episode. `seed` is a `u32`.

## Lists

- `sim.vehicle_list()`: `[{ "name": "car", "max_health": 450, ... }]` with the prototype values used.
- `sim.fuel_list()`: `[{ "name": "coal", "fuel_value": 4000000, "fuel_acceleration_multiplier": 1.0, "fuel_top_speed_multiplier": 1.0 }]`.

## Game graphics

The local `/game-assets/manifest.json` contract is in [assets.md](assets.md).
Use palette names to resolve tiles/resources, entity names to resolve trees and
rocks/nests, cliff orientations for cliffs, and vehicle kind/orientation for
vehicle direction layers. The frontend does not parse Factorio prototype graphics.
