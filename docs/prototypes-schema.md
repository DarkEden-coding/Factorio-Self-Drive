# Prototype snapshot schema

`data/prototypes.json` uses schema version 2 and installed Factorio 2.1.23 base
Nauvis data at normal quality, without equipment, research or mod overrides.
The simulator's `physics` object contains assumptions, separate from game data.
Version 2 requires `immune_to_all_impacts`; version 1 snapshots must be
re-extracted or dumped again. Transient JSON map and trace schemas remain version 1. Binary maps use FSMAP001.

`fsim_core::Prototypes::from_json` loads the snapshot; wasm embeds it. Unknown
JSON metadata fields are allowed. Required fields must remain present.

| Key | Content |
| --- | --- |
| `schema_version` | Integer, currently 2. |
| `source` | Required `game_version`; optional `active_mods`. Extraction records `kind: "installed-game"`. Runtime dumps record active mods. No git provenance is invented for installed data. |
| `vehicles` | Car and tank. |
| `fuels` | Five chemical fuel items. |
| `tiles` | 21 tiles indexed by unsigned byte id. |
| `entities` | 25 base tree, rock and biter/spitter spawner prototypes. |
| `cliffs` | Cliff placement grid and orientation-specific collision geometry. |
| `resources` | Five ores and crude oil with ids 1 through 6. |
| `physics` | Simulator settings, preserved by the import script. |

## Vehicle mapping

Lengths are tiles; rotation rates are turns/tick. Weight stays in Factorio's
mass units: car 700, tank 20000. Energy uses joules and consumption uses watts.

| Snapshot field | Data-stage / runtime mapping |
| --- | --- |
| `name`, `weight`, `effectivity`, `rotation_speed`, `rotation_snap_angle`, `terrain_friction_modifier`, `energy_per_hit_point`, `tank_driving` | Same-named `LuaEntityPrototype` properties. Car terrain friction defaults to 1. |
| `consumption` | Parse the data-stage energy string as watts; runtime consumption ×60. |
| `braking_power` | Keep the simulator's existing normalized value: numeric data-stage/runtime `braking_force` ×60. The 2.1 API specifies a positive double, without an SI-unit label. Base car/tank expressions are `(200 * 1000) / 60` and `(800 * 1000) / 60`, yielding snapshot values 200000 and 800000. The removed `braking_power` energy-string property is not read. |
| `friction` | Data-stage/runtime `friction_force`, copied directly. |
| `max_health` | `LuaEntityPrototype.get_max_health("normal")`. |
| `collision_box` | Normalize to `[[left,top],[right,bottom]]`. |
| `resistances` | Array of `{ "type", "decrease", "percent" }`. Missing numbers mean 0. Runtime dictionary percentages ×100 give the snapshot's 0..100 scale. |
| `immune_to_tree_impacts`, `immune_to_rock_impacts`, `immune_to_cliff_impacts`, `immune_to_all_impacts` | Data-stage companion values, preserving explicit false and applying API defaults. All four are also available on `LuaEntityPrototype` in 2.1.23. |

Tree, rock and all-impact immunity default to false; cliff immunity defaults to
true. Base tank explicitly enables tree, rock and all-impact immunity.
`immune_to_all_impacts` covers damage from driving into anything, including a
spawner. It overrides specific false flags. The simulator suppresses vehicle
damage while retaining obstacle damage, blocking and destruction slowdown.
It does not model immunity to weapons or arbitrary impact damage sources.

## Fuels, tiles and resources

Fuel fields are `name`, `fuel_value` in joules,
`fuel_acceleration_multiplier` and `fuel_top_speed_multiplier`, matching
`LuaItemPrototype`. Both multipliers default to 1. Fuel supply is unlimited.

Tiles contain `id`, `name`, byte RGB `color`, `vehicle_friction_modifier`,
`is_water` and a list of enabled `collision_mask.layers`. Normalized colors
are multiplied by 255 and rounded. Selected names cover grass-1..4, dry-dirt,
dirt-1..7, red-desert-0..3, sand-1..3, water and deepwater. Natural friction
modifiers are 1.6 for grass/red desert, 1.4 for dirt and 1.8 for sand. Water
uses the tile default of 1 and remains impassable.

Resources contain `id`, `name` and byte RGB `color` from entity `map_color`.
Ids are iron=1, copper=2, coal=3, stone=4, uranium=5 and crude-oil=6. Grid zero means no resource.
Wasm `resource_palette()` adds the indexing placeholder
`{ "id": 0, "name": "none", "color": [0,0,0] }`; it is not a prototype.

## Obstacles and cliffs

Entities contain `name`, semantic `kind`, `max_health`, `collision_box` and
`resistances`. Runtime types tree, simple-entity and unit-spawner map to tree,
rock and nest. Live trees have 50 health and boxes of +/-0.4 tiles; dry trees
have 20 health. Rocks have 500 or 2000 health; biter spawners have 350.

`cliffs` contains `name`, `max_health: null`, `grid_size: [4,4]`,
`grid_offset: [0,0.5]` and 20 hyphenated orientation names. Each maps to
`{ "collision_box", "orientation" }`. Intrinsic rotations are 0 or 0.125 turns.
Corner/cap boxes rotate about their own centers. The data-stage companion
preserves `orientations.*.collision_bounding_box`, including asymmetric boxes;
the generic cliff box is insufficient. Runtime 2.1.23 also exposes orientations
and grid settings. Cliffs have no health and cannot be destroyed by vehicles.

## Simulator settings

These values are simulator choices, not exported game numbers. See
[physics.md](physics.md) for evidence and calibration limits.

| Key | Value | Meaning |
| --- | --- | --- |
| `reverse_power_multiplier` | 0.5 | Reverse power fraction. |
| `car_steering_full_speed` | 0.1 | Tiles/tick at full car steering rate. |
| `stop_speed` | 0.00001 | Coasting cutoff. |
| `collision_energy_scale` | 1 | Scale on tick-speed kinetic energy. |
| `collision_retained_energy` | 0.9 | Fraction of destruction energy cost waived. |
| `max_translation_substep` | 0.2 | Tiles per contact substep. |
| `max_rotation_substep` | 0.002 | Turns per contact substep. |
| `nest_penalty_radius` | 18 | Static hazard radius in tiles. |
| `target_radius` | 3 | Arrival radius in tiles. |

## Reproducing the snapshot

For the complete current catalog, run `scripts/collect-game-data.sh`. The live
import preserves existing IDs and physics settings and appends new entity/resource
names. The older offline extractor covers a smaller bootstrap catalog.

The extractor reads installed Lua text and the installed prototype API defaults.
It supports 2.1.23's selected expressions, not arbitrary Lua, and never executes
game code. It accepts the `Contents`, `Contents/data` or `Contents/data/base`
directory. Sources are `data/base/prototypes/{entity,item.lua,tile,decorative}`;
API definitions are `doc-html/{runtime-api,prototype-api}.json` beside `data`.

```sh
G="$HOME/Library/Application Support/Steam/steamapps/common/Factorio/factorio.app/Contents"
python3 scripts/extract-base-prototypes.py "$G"
python3 scripts/check-mod-api.py "$G"
```

The normalized 2.1.23 snapshot has no numeric changes from 2.0.77 in vehicles,
fuels, tiles, entities, cliffs or resources. The added immunity values are false
for car and true for tank. Actual engine update code remains proprietary.
