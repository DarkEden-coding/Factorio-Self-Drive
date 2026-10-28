# Progressive route planning, 0.7.0

The planner now extends a cached, verified corridor while the vehicle drives. It smooths across
section boundaries and revisits the distant route without repeatedly steering toward temporary
endpoints. The existing field-smooth controller and weights remain unchanged. Measurements
compare against the requested checkpoint, `7f5a4db`, version 0.6.0.

## Planner behavior

The first field still aims about 48 tiles ahead. A single background worker builds overlapping
160-tile sections toward a soft 768-tile horizon, with at most six ready sections. The last
section can overshoot the horizon. The measured maximum was about 895 tiles. Both solved fields
and terrain blocks are reused; consumed fields and terrain outside retained coverage are pruned.

Each tick builds at most four terrain blocks and advances at most 4,000 field work units.
Refinement runs when a section completes and every 120 ticks while multiple sections remain.
It keeps the next 24 tiles of guidance, then uses cost-aware any-angle shortcuts over verified
coverage. A three-tile buffer at coverage edges allows normal tracking error. Handoffs can skip
several intermediate fields when a shortcut bypasses their endpoints. The entire remaining route
must stay covered before those fields are discarded.

The destination and trip clock persist through handoffs. The actor's original 4/8/16/32-tile
lookaheads can cross section boundaries; temporary endpoints no longer replace the destination.
There was no retraining or input-layout expansion. A frontier brake uses the vehicle's kinetic
energy/braking power to stop before unverified terrain if the worker falls behind.

Mining/destruction invalidates affected cached coverage. The worker repairs it before replacing
the old conservative route and discards dependent future sections. Additions and tile edits are
outside this removal-only repair mechanism. The supported obstacle domain remains base Nauvis
terrain; player structures and moving units are not modeled.

Verification also exposed `0 * infinity` in axis-aligned ray traversal at integer tile boundaries.
Lua and Rust now avoid that NaN, with cardinal-direction regression checks. This correction is
included in the candidate measurements; the checkpoint retains its original ray behavior.

## Paired long trips

The isolated Factorio 2.1.23 suite uses car and tank on six 480–988 tile scenarios. It includes
straight travel, diagonal travel, repeated forest barriers, real Nauvis terrain, and a return
trip with rocket fuel. Each pair has identical initial pose, requested destination, vehicle,
fuel and terrain seed. Fixtures begin at the same 120-tick scheduler phase. Trees are restored
between vehicle cases. Startup planning, cache waits and frontier waits all count toward time.
No user saves or installed settings are used by the tests.

Two terrain seeds, 12345 and 67890, exercise 24 trips per planner. Ten synthetic cases repeat
across seeds; this is not a sample of 24 independent random maps. Speed/path comparisons use
only trips both planners completed. Failed trips remain in completion and safety totals.

Across the 23 common successes, summed end-to-end time falls **1.57%** and driven distance
falls **1.65%**. Both versions complete 23/24 attempts. Health damage is zero in both.
Blocked powered ticks rise from 30 to 42 across all long attempts.

Seed 12345 completes all twelve trips on both versions:

| Case | Total seconds, 0.6.0 → 0.7.0 | Driven tiles, 0.6.0 → 0.7.0 |
|---|---:|---:|
| car-straight480 | 21.15 → 21.47 | 479.4 → 479.7 |
| car-angle800 | 35.27 → 34.50 | 837.8 → 811.5 |
| car-forest700 | 32.35 → 31.07 | 761.6 → 725.6 |
| car-real700 | 29.75 → 29.93 | 719.3 → 720.3 |
| car-diagonal980 | 43.80 → 42.85 | 1053.5 → 1026.9 |
| car-return980 | 31.82 → 30.07 | 1056.0 → 1050.7 |
| tank-straight480 | 38.02 → 38.13 | 479.5 → 479.2 |
| tank-angle800 | 63.47 → 61.97 | 835.9 → 808.1 |
| tank-forest700 | 54.20 → 54.13 | 705.0 → 703.8 |
| tank-real700 | 53.88 → 54.40 | 706.6 → 707.3 |
| tank-diagonal980 | 78.95 → 77.33 | 1050.7 → 1024.6 |
| tank-return980 | 55.43 → 53.73 | 1051.3 → 1045.5 |

On seed 67890, the real-terrain tank trip takes 56.52 → 57.67 seconds, with 721.9 → 720.5
driven tiles and 29 → 40 blocked powered ticks. On seed 12345 that contact estimate is 1 → 2.

Both versions fail the same `car-real700` trip on seed 67890 during initial planning. The new
planner does not resolve that existing reachability failure. All completed trips stop within
one tile at at most 0.01 tiles/tick. All short regression fixtures complete. Frontier waits are
zero in the engine long suite; a deliberately delayed worker test separately checks braking
and recovery.

Blocked powered ticks are a contact estimate, not engine collision events. They increased in
the real-terrain tank cases. The result supports a modest route/time improvement, not improved
collision avoidance. Straight drives and real-terrain tank trips can be slightly slower.

The checked-in [benchmark data](progressive-route-benchmark.json) retains per-case results.
Raw reports, traces and profiler logs are under `data/autodrive/progressive-*` locally and are
ignored by Git. Profiler samples include background work and were collected with concurrent
verification jobs; they are diagnostics, not a controlled CPU-speed comparison or a hard
millisecond guarantee. Refinement geometry is bounded by retained coverage but is not itself
a resumable 4,000-unit field slice.

## Checks and reproduction

- 52 Rust tests pass, including parallel rays from integer tile boundaries.
- 65 Python/script tests and five subtests pass after rebuilding the native extension.
- The WASM build/smoke checks, TypeScript typecheck and Factorio API audit pass.
- Four Lua checks cover corridor refinement, immutable route consumption, coverage edges,
  frontier braking/recovery, selective repair, obstacle costs and grid caching.
- All twelve short engine fixtures arrive with zero damage and zero blocked powered ticks.
- Saving during an extended drive and replaying the checkpoint produces identical final
  results for all twelve long fixtures on seed 12345.
- On sixteen fixed real-map poses, Lua/Rust fields, all 135 features, logits and actions match
  exactly. All twelve engine snapshot actions match too. Engine ray differences are at most
  5.96e-8 and logit differences at most 1.11e-16. Corridor features are independently encoded
  from the published route; Rust supplies the ray and vehicle/target feature reference.
  This snapshot check does not claim native/Lua progressive-planner equivalence.

To reproduce the checkpoint comparison with identical test instrumentation:

```sh
mkdir -p data/autodrive/progressive-baseline-source
git archive 7f5a4db mod/fsim-autodrive | tar -x -C data/autodrive/progressive-baseline-source
cp scripts/fixtures/autodrive/test-runner.lua data/autodrive/progressive-baseline-source/mod/fsim-autodrive/
```

The archived test interface did not accept options. In its archived `control.lua` only, change
`start=function() test_runner.start() end` to `start=function(options) test_runner.start(options) end`.
Keep all archived production modules, including `driver.lua` and `observation.lua`, unchanged.
Then run each seed on both versions:

```sh
python3 scripts/test-autodrive.py --engine-only --suite long --seed 12345 --timeout 300 \
  --mod-source data/autodrive/progressive-baseline-source/mod/fsim-autodrive \
  --output data/autodrive/progressive-checkpoint-12345
python3 scripts/test-autodrive.py --engine-only --suite long --seed 12345 --save-reload --timeout 300 \
  --output data/autodrive/progressive-final-12345
python3 scripts/compare-autodrive.py \
  data/autodrive/progressive-checkpoint-12345/driving-report.json \
  data/autodrive/progressive-final-12345/driving-report.json
```

Repeat with seed 67890. `--case tank-diagonal980` isolates a particular fixture.
The comparison rejects mismatched initial states and excludes failed attempts from speed gains.
The native simulator still uses a full episode field, so it is not used as evidence for
asynchronous planning performance. Long-trip claims here come from the production Lua driver
in Factorio.
