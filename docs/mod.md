# Factorio export mod

`mod/fsim-export` targets Factorio 2.1.23 or later in the 2.1 series, including
Space Age. It exports selected Nauvis prototypes, calibration traces and maps.
The installed 2.1.23 API definitions and base data support the offline checks;
live calibration traces are still needed to verify engine behavior.

## Install on macOS

From this repository:

```sh
mkdir -p "$HOME/Library/Application Support/factorio/mods"
ln -s "$PWD/mod/fsim-export" "$HOME/Library/Application Support/factorio/mods/fsim-export_0.1.0"
```

Enable `fsim-export` in Factorio's Mods menu and restart. Use a disposable save
for calibration. The commands require an admin player, or the server console
where a player position is unnecessary. Lua commands and mods affect achievement
eligibility. The recorder creates its own character and force; it does not move
the player's character. Leave the game unpaused until an operation completes.

## Commands and output

All paths below are relative to Factorio's `script-output` directory. On macOS
this is `~/Library/Application Support/factorio/script-output/`.

| Command | Result |
| --- | --- |
| `/fsim-dump-prototypes` | Writes `fsim/prototypes.json`. |
| `/fsim-list-scenarios` | Lists all 80 scenario names in the player's console. |
| `/fsim-record <scenario>` | Prepares a fresh test surface and writes `fsim/traces/<scenario>.json`. |
| `/fsim-record-all` | Runs every scenario in order, deleting each test surface afterward. |
| `/fsim-export-map <radius_chunks>` | Writes `fsim/maps/<surface>-<tick>-p<player>.json`. Radius is an integer from 0 through 31. |

Only one recording queue and one map export can run at once. A queue can survive
save/reload through Factorio's `storage`. Trace files with the same scenario name
are overwritten. In multiplayer, output goes to the invoking player's computer;
server-console dumps and recordings go to the server. Streamed files are only
complete after the completion message. Saving/reloading can restore job state,
but cannot roll back files: restart an unfinished export after loading an older
save. If recording aborts, discard its incomplete file and retry the command.

Example:

```text
/fsim-dump-prototypes
/fsim-record car-accelerate-grass-1-coal
/fsim-record car-brake-reverse
/fsim-export-map 4
```

Radius 4 requests a 9 by 9 square of chunks centered on the player's chunk,
including the center chunk. The exporter checks one chunk each tick and never
requests generation. Ungenerated chunks are omitted. It exports live state at
the time each chunk is read, so a large export is not an atomic snapshot. Keep
other activity away from the export area when reproducibility matters. Only
resources and natural obstacles are included, not player structures or enemies
that move. Simple entities must have the game's rock deconstruction flag.

## Space Age planets

With Space Age active, the catalog adds the Vulcanus, Gleba and Fulgora tiles, natural
obstacles, resources and cliffs (`catalog.space_age`). A tile's `is_water` means "blocks
vehicles": its collision mask shares a layer with the car's. That is true for water, deep water,
lava, hot lava and Gleba's deep lakes, and false for Gleba's wetland and Fulgora's oil ocean,
which are drivable but slow (`vehicle_friction_modifier` 8 and 4-10). Planet cliffs go in
`other_cliffs`; all but `crater-cliff` share the Nauvis geometry. Plants count as trees and
Fulgora's lightning-attractor ruins as rocks.

The trace table adds planet scenarios when Space Age is active: acceleration on planet ground,
crossing oil ocean, driving into lava and deep lakes, and impacts with planet trees, plants,
rocks, ruins and cliffs, for both vehicles. Their surfaces copy the planet's gravity and
pressure. Factorio 2.1.24 crashes deleting a surface after `set_property`, so those test
surfaces stay until the collection save is thrown away. Of 90 recorded planet traces, 79
replay with zero position error, including every acceleration, oil-ocean, lava and deep-lake
run. The rest are contacts with planet obstacles; four exceed 0.05 tiles, the largest 0.31 tiles
(car into an ashland lichen tree at 0.1 tiles/tick) and 0.16 tiles (car into a lightning-attractor
ruin at 0.3 tiles/tick). All 341 Nauvis traces still replay with zero error.

## Prototype import

The data-stage companion reads all four vehicle immunity flags and the full
cliff orientation geometry, grid size and offset from `data.raw` in
`data-final-fixes.lua`. A `mod-data` prototype carries those values into runtime
through `prototypes.mod_data`. No cliff geometry is guessed from the generic
runtime box. Explicit false immunity values stay false; omitted values use the
prototype API defaults. Mods that change prototypes after this mod's
`data-final-fixes` require an ordering dependency or another dump strategy.

The dump contains the Nauvis prototype catalog in `catalog.lua`, including every base tree/rock/spawner and all six resources. It
records `helpers.game_version` and `script.active_mods` in `source`. Runtime
consumption is multiplied by 60 to obtain watts. Numeric `braking_force` is
multiplied by 60 to retain the simulator's `braking_power` convention, matching
the base car/tank expressions of 200000/60 and 800000/60. Runtime
resistance percentages are multiplied by 100. Colors become integer RGB bytes.
Cliff `max_health` is JSON null. The dump omits `physics`.

```sh
python3 scripts/import-prototypes.py \
  "$HOME/Library/Application Support/factorio/script-output/fsim/prototypes.json" \
  data/prototypes.json
```

The script atomically replaces the current file, keeping its `physics` object
and tile/resource IDs by name. Use `-o /tmp/prototypes.json` to inspect a merged
file first. It rejects missing or duplicate names and incompatible schema versions. New
entity/resource names are appended in sorted order, with resource IDs continuing
after the existing palette. Vehicle, fuel and tile name sets must stay exact. Rebuild wasm after importing to update its embedded snapshot. The physics settings are preserved during this expansion. Unknown modded map
names still use the permissive map-loader fallbacks.

## Calibration workflow

The scenario table is `mod/fsim-export/scenarios.lua`. It includes:

- 30 straight acceleration runs: car/tank, grass/dirt/sand and all five fuels.
  Each lasts 6000 ticks on a 64 by 8192 tile track.
- Coasting down toward the zero-speed cutoff, braking through zero into reverse,
  and braking back toward forward motion, for both vehicles.
- Left/right steering at zero, near-zero, moderate, high and reverse speeds,
  followed by straight coasting to expose axis snapping and turn-related loss.
- Car impacts with trees, rocks and cliffs at three speeds, with straight and
  oblique approaches. Tank runs through repeated trees and into rocks at two
  speeds. Both vehicles approach water straight and diagonally.

Other tests use 128 by 1024 tile surfaces. Every surface has a fixed seed,
disabled autoplace, no enemies, no decoratives, peaceful mode and fixed daytime.
Preparation fills one chunk per tick, removing generated entities/decoratives.
Vehicles and their seated standalone character use a dedicated unresearched
`fsim-calibration` force and normal quality, with no equipment. The recorder fills
the fuel inventory and replenishes the chosen fuel every tick. Burner startup
and fuel insertion timing are deliberately recorded, not hidden by warmup.
Most collision/steering tests set initial signed speed directly to isolate that
behavior. No physics tuning is changed by the tools.

A trace has `schema_version: 1`, `header` and `samples`. The header includes the
scenario name, game/mod versions, surface tile and half-open area, water tile
patches, initial vehicle position/orientation/speed/health, fuel, obstacle
names/kinds/positions/health/cliff orientations and input segments. Each segment
has `ticks`, `throttle` and `steer`. Values use the simulator units in
[sim-api.md](sim-api.md).

Sample 0 is the initial state. Sample N records the state after N physics ticks
and labels the input applied during tick N. Every sample contains `tick`,
`inputs`, `x`, `y`, `orientation`, `speed`, `health` and `obstacle_health`.
`obstacle_health` follows header obstacle order; cliffs and destroyed obstacles
use zero. The recorder selects accelerating/braking/reversing from signed speed
and logs that exact enum name alongside the simulator input. It sets
`riding_state` each tick. Destruction ends a recording early. An
`on_entity_died` handler captures the death position; check
`death_position_captured` on such files. If another mod removes a vehicle without
a death event, the final sample uses its last valid position.

Replay after importing matching prototype values:

```sh
cargo run --release -p fsim-core --bin replay-trace -- \
  --prototypes data/prototypes.json \
  "$HOME/Library/Application Support/factorio/script-output/fsim/traces/"*.json
```

The optional `--prototypes` selects a snapshot without rebuilding. Otherwise the
binary embeds `data/prototypes.json` at compile time. One JSON report per trace
contains max/RMS position error in tiles, shortest wrapped orientation error in
turns, signed speed error in tiles/tick, vehicle health error and obstacle health
error. Obstacle RMS includes each obstacle at each sample. Initial samples count
in all metrics. `first_position_error_tick` is null unless position error first
exceeds 0.05 tiles. Malformed traces fail with a filename and error; other input
files are still processed. Trace reconstruction requires exact known prototypes,
unlike map import's permissive fallbacks. Error magnitude does not determine the
process exit code, since these traces are for calibration.

## Map interchange

Map JSON has `schema_version: 1`, `area: [[x0,y0],[x1,y1]]`, optional `seed`, and
`chunks`. Coordinates remain absolute Factorio coordinates on disk. The area is
half-open and chunk-aligned. Metadata records source mods, surface name and start
and finish ticks. Each chunk contains:

```json
{
  "position": [2, -1],
  "tiles": ["grass-1", "grass-1"],
  "resources": [{"name": "copper-ore", "position": [82, -14]}],
  "entities": [{"name": "cliff", "kind": "cliff", "position": [68, -7.5],
                "cliff_orientation": "west-to-north", "orientation": 0}]
}
```

The abbreviated tile array above must actually contain 1024 names, row-major:
local index `dy * 32 + dx`. Resources use integer tile positions. Entities use
exact positions and belong to the chunk containing their center, so boundary
queries cannot duplicate them. Destroyable entities also record current health.

Rust `fsim_core::map_import::load_map(json, &prototypes)` returns
`Result<(World, MapLoadReport), String>`. The collector converts this transient JSON to the compact format in
[map-format.md](map-format.md). `Simulator::load_map` and wasm
`sim.load_map(map: Uint8Array): string` accept only those binary map bytes and
return JSON `{ "offset": [x, y], "warnings": [...], "name": "..." }`. The offset is also retained in
`map_info().source_offset`. Factorio position = simulator position + offset.
Unknown names fall back by semantic kind/name, warning once per distinct mapping:
water-like tiles stay water, sand/dirt use their matching terrain family,
unknown trees/rocks/nests retain source names for graphics and use known representatives for physics, and unknown resources use a
name match or stone. Unknown cliff orientations use `west-to-east`. Ungenerated
gaps become blocked deepwater. No valid spawn causes episode loading to fail
without changing the current map, target or vehicle. The frontend contract is in [sim-api.md](sim-api.md). Graphics extraction and the
render-ready manifest are defined in [assets.md](assets.md).

## Verified API and first live run

The rerunnable name audit uses the installed game's `doc-html/runtime-api.json`
and `prototype-api.json`, not the moving website docs. It extracts API names,
indexed vehicle properties, prototype types, riding enums and method record
fields from the Lua. It also checks `MapGenSettings`, its autoplace dictionaries
and `CliffPlacementSettings`. Local handle aliases are declared in the checker;
add bindings when introducing new aliases. This is a static name audit, not an
engine test. All existing map-generation fields remain valid in 2.1.23.

```sh
G="$HOME/Library/Application Support/Steam/steamapps/common/Factorio/factorio.app/Contents"
python3 scripts/check-mod-api.py "$G" --list
```

The companion remains the source of immunity and oriented cliff geometry,
although 2.1.23 also exposes these fields on `LuaEntityPrototype`. The tank's
`immune_to_all_impacts` prevents damage from driving into any entity. It does
not remove blocking, damage to the obstacle, or destruction slowdown.

Before using traces for tuning, verify these in Factorio:

- Mod startup, data-stage companion loading, command registration and JSON empty
  arrays/null values. Verify output goes to the expected client/server folder.
- Dump the unmodified base prototypes and compare values with the pinned snapshot,
  especially consumption/braking ×60, resistance ×100, colors, false immunity
  flags and all 20 cliff boxes including their intrinsic rotations.
- Confirm empty test generation, exact flat tile coverage including water edges,
  no unexpected entities/decoratives/enemies, and successful surface deletion.
- Confirm the standalone seated character permits driving, fuel burns and stays
  supplied, and the tank has no equipment or research bonuses.
- Run a short trace and inspect sample 0 and the first few moving ticks. Confirm
  `on_tick` samples describe the preceding input, including the first burner tick.
  Check brake-to-reverse transitions and both reverse steering directions.
- Check placed obstacle/cliff positions and orientations, obstacle health order,
  collision death capture and early completion, and the 6000-tick tracks staying
  within prepared bounds. Compare engine quantization, snapping, glancing contact,
  damage and destruction slowdown against replay reports.
- Export at a generated/ungenerated boundary. Confirm 1024 tiles per included
  chunk, fractional entity positions, resource floor-to-tile behavior, no duplicate
  boundary entities and the coordinate offset after loading in Rust/wasm.
- Exercise record-all and save/reload during preparation and recording. Confirm
  streamed files close correctly and the next scenario starts on a fresh surface.

Offline checks:

```sh
find mod/fsim-export -name '*.lua' -exec luac -p {} \;
lua scripts/check-mod.lua
python3 -m unittest discover -s scripts/tests
cargo test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
scripts/build-wasm.sh
node scripts/check-wasm.mjs
```

The Lua checks cover syntax, normalization, companion defaults, dump conversions
and scenario coverage using table stubs. They cannot establish engine behavior.

## Automated collection

`scripts/collect-game-data.sh` creates isolated base-only saves with fsim-export
and fsim-autorun. It never changes the user's saves or installed mods. Defaults
are 16 seeds, chunk radius 16, and all trace scenarios on the first seed only.
`--map-count`, `--map-radius` and `--seeds` configure the real map pool.
`--skip-traces` or `--skip-maps` skip the corresponding new exports. Existing
traces are still replayed. `--scenarios substring1,substring2` filters trace names.

The manual map command above only reads generated chunks. In contrast, autorun
explicitly requests and forces missing chunk generation before exporting. Each
map export is converted straight into `data/game/maps/<name>.fsmap`; raw JSON
exists only in the temporary engine directory and is deleted after collection.
The collector writes the map index, imports live prototypes while preserving
physics settings, builds local graphics assets, rebuilds wasm and writes replay
reports. Collection timing and per-run provenance are in
`data/game/map-collection.json`. Its file inventory includes the pool's compressed
byte sizes. Engines started by the script are always terminated and reaped.

The new graphics companion is in `mod/fsim-autorun/data-final-fixes.lua`, separate
from the physics companion. The API checker audits its prototype aliases against
the installed prototype API JSON. All Lua syntax checks should include both mod
directories. See [assets.md](assets.md) for the generated graphics contract.
