# Real map pool and FSMAP001

All simulator episodes use maps exported by Factorio. There is no simulator map
generator. Run `scripts/collect-game-data.sh` with the local Factorio 2.1 install.
The defaults are 16 seeds and a chunk radius of 16. Each map contains 33 × 33
chunks, or 1056 × 1056 tiles. `--map-count N` changes the count; `--map-radius R`
accepts 0 through 31. `--seeds 12345,67890` supplies an explicit seed list and
overrides the count. Default seeds are `(12345 + i * 2654435761) mod 2^32`.
These are ordinary default-preset Nauvis saves with no map-generation overrides.
The autorun exporter requests generation for every missing chunk in its area.

The collector converts streamed JSON immediately, then deletes the isolated
Factorio directory. No map JSON is published to the pool. A successful collection
replaces the pool index and removes stale `.json` and `.fsmap` files from that
directory. `--skip-maps` preserves the current pool. `--skip-traces` skips new
recordings but still replays existing trace files. All started engines are
terminated and reaped on completion or failure.

## Binary layout

Integers and floating point numbers are little endian. A `.fsmap` file contains:

| Byte offset | Type | Meaning |
| --- | --- | --- |
| 0 | 8 bytes | ASCII `FSMAP001`, including format version |
| 8 | u32 | UTF-8 JSON header length H |
| 12 | u32 | Uncompressed body length B |
| 16 | H bytes | Metadata and name tables |
| 16 + H | remaining bytes | One zlib stream containing the body |

The small JSON header contains `name`, `seed`, `area: [[x0,y0],[x1,y1]]`,
`spawn: [0,0]`, `source` provenance, `tile_names: string[]`,
`resource_names: string[]`, `entity_names: [name,kind][]`,
`cliff_names: string[]`, `entity_count`, and `warnings: string[]`.
Names use UTF-8. Name-table indices are local to this file, independent of the
simulator prototype palette. Resource index 0 is always `"none"`. Missing chunks
use `"deepwater"` and produce a warning. `spawn` is in source Factorio coordinates, bounded to ±2^31 tiles.

The body contains two `width * height` u8 grids, tiles first and resources second,
then `entity_count` 32-byte records. Grids are row-major from source `(x0,y0)`.
Each record contains:

| Record offset | Type | Meaning |
| --- | --- | --- |
| 0 | u16 | Index into entity_names |
| 2 | f64 | Source x, preserving Factorio position precision |
| 10 | f64 | Source y |
| 18 | f64 | Factorio orientation in turns |
| 26 | u16 | Index into cliff_names, or 65535 for none |
| 28 | f32 | Current health, or NaN for unspecified/cliff health |

Collision boxes, maximum health and resistance values come from the prototype
snapshot. Resource amounts are intentionally absent; resource cells indicate
presence. Graphics variations are chosen by the viewer, not serialized game
visual variation indices. Compression is lossless except for health stored as
f32. Entity record order becomes simulator entity IDs starting at 1.

The loader validates bounds, name indices, finite positions/orientations,
nonnegative health, body length, zlib integrity, and trailing bytes. Dimensions
must be even, each in 2..8192, with area at most 16,777,216 tiles. Header size is
limited to 1 MiB and entity count to 4,000,000. Structural corruption returns an
error. Unknown prototype names use semantic fallbacks with warnings. Entity names
remain the original source names for rendering; physics uses the fallback's
geometry and resistance values. Unknown resources/tiles use a fallback palette
entry. See [mod.md](mod.md) for the transient JSON export and fallback rules.

## Coordinates, spawning and loading

Maps are recentered in simulator space. `source_offset` equals
`[x0 + width/2, y0 + height/2]`. Source position equals simulator position plus
this offset. For the default radius 16 export, the source area is
`[[-512,-512],[544,544]]`, the offset is `[16,16]`, and Factorio's origin is
simulator `[-16,-16]`. Do not assume the spawn is the simulator's center.

Reset first tries the exact source spawn translated into simulator coordinates,
with orientation north. If the hull is blocked, it chooses the closest valid
integer offset from that position by squared Euclidean distance. Ties keep the
first candidate visited in increasing square radius, then increasing y and x.
The search center is clamped into the map when the source spawn is outside it.
No valid hull position is an error. Loading is transactional, preserves vehicle
and fuel selections, clears the target, and increments both revisions. Reset
keeps the current map and destroyed obstacles.

## Pool index and native interface

`data/game/maps/index.json` is a JSON array, with no enclosing object:

```json
[{
  "name": "nauvis-seed-12345",
  "file": "nauvis-seed-12345.fsmap",
  "seed": 12345,
  "width": 1056, "height": 1056,
  "x_min": -528, "y_min": -528,
  "source_offset": [16,16], "entity_count": 12345,
  "split": "train"
}]
```

The example entity count is illustrative. `split` is `"train"` or `"holdout"`
(see [rl.md](rl.md)). The collector marks the last `--holdout-count` seeds
(default 8) as holdout, keeping at least one train map. The split is a function of
seed order, so the same seed list always gives the same split.

Resolve `file` relative to the index's
directory. The frontend must serve this pool at `/data/game/maps/`, fetch the
index, then fetch the chosen map as `Uint8Array`. The wasm API consumes the
compressed bytes directly. See [sim-api.md](sim-api.md).

Native Rust callers use:

```rust,ignore
let mut pool = fsim_core::map_pool::MapPool::load("data/game/maps", 42)?;
let (world, report) = pool.map(0, &prototypes)?; // or pool.sample(&prototypes)?
let sim = fsim_core::Simulator::new(prototypes, config, world)?;
```

`MapPool::load(path, seed: u64)` accepts a directory or index file. It keeps public
`entries: Vec<MapEntry>` metadata and a seeded PCG32 RNG. `map(index, data)` loads
one file and checks its metadata against the index. `split_indices(split)` lists
the entries of one split in index order. `sample(data)` uniformly
selects an entry using that RNG. Files are read on demand, so the full pool need
not fit in RAM. Identical ordered index files and RNG seeds give identical sample
sequences. Paths must be relative and cannot contain `..`.

`fsim_core::map_format::load_map(bytes: &[u8], &Prototypes)` returns
`Result<(World, MapLoadReport), String>`. The old JSON reader remains solely for
export audits. Simulator and wasm loading use `.fsmap` bytes.

The pool, including its index, is gitignored by `data/game/.gitignore`; regenerate
it locally. The 128 × 128 real-map fixture
`crates/fsim-core/tests/fixtures/nauvis-origin.fsmap` is intentionally tracked. It
is the 4 × 4 chunk cut `[-64,64)²` from the default Nauvis seed 12345 export made
by Factorio 2.1.23, without terrain or entity edits. The existing small synthetic
JSON fixture tests interchange edge cases only. It is not used for map generation.

## Measured default collection

On this Mac with Factorio 2.1.23, the full collector completed in 175.00 seconds,
including default map creation, all requested trace recordings, prototype import,
asset building, wasm build and replay of existing traces. The sixteen maps total
3,567,765 bytes, range from 89,965 to 333,626 bytes, and contain 430,543 entity
records. The index adds 4,248 bytes. The real 128 × 128 fixture is 4,531 bytes.
The RL pool is 104 maps (`--skip-traces --map-count 104`): 96 train and 8
holdout. Collection took 522.9 seconds (about 5 seconds per map). The maps total
24,990,574 bytes (70,888 to 354,901 each) and contain 3,066,871 entity records.
The index is 29,973 bytes.
Map collection measurements are retained in `data/game/map-collection.json`, which
trace-only runs do not overwrite. The first retained summary was recovered from
the verified run console log after a concurrent trace-only run replaced the
general report; its per-run times are rounded to 0.01 seconds. Subsequent full
collections save complete engine markers there automatically.
The expanded prototype catalog resolves every collected map name without fallback.
