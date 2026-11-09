# Vulcanus, Gleba and Fulgora, 2026-11-09

Mod 0.9.0 drives on Vulcanus, Gleba and Fulgora as well as Nauvis. Aquilo is out of scope.
The simulator now loads real planet maps, models their blocking and slow tiles, planet
obstacles and demolisher territory, and prices slow ground in the distance field. The shipped
student, `planets-v2`, is a PPO fine-tune of `field-smooth-v1` on Nauvis plus the three
planets; its observation layout and spec hash (`d1c084cc679f1f71`) are unchanged.

| Pool | Simulator holdout, shipped 0.7.1 model | `planets-v2` | A* baseline | In game, `planets-v2` (0.9.0) |
|---|---:|---:|---:|---:|
| Nauvis | 99.6% | 99.8% | 41.4% | 24/24 engine fixtures |
| Vulcanus | 95.8% | 98.7% | 45.3% | 63/64 trips |
| Gleba | 99.5% | 99.9% | 52.6% | 64/64 trips |
| Fulgora | 98.1% | 98.8% | 52.5% | 64/64 trips |

Simulator: holdout maps, seed 67890, 200 episodes for each of six scenario kinds (1,200 per
pool), both models on identical episodes with the same field settings. In game: Factorio 2.1.24
with Space Age, headless, enemies enabled, 16 trips per planet and vehicle on a training seed
and a holdout seed. The A* baseline is scored under the students' `stop` arrival rule (stop
within 1 tile), which pure pursuit handles badly; its numbers are a floor, not a rival.

## Planet data

`scripts/collect-game-data.py --planets vulcanus,gleba,fulgora` enables Space Age and exports
each planet surface of a default save, 40 seeds per planet (32 train, 8 holdout), 1056 x 1056
tiles each, into `data/game/maps-<planet>/`. Collection takes about 16 minutes on the Linux box.
Maps average 16,600 (Vulcanus), 35,500 (Gleba) and 6,500 (Fulgora) entities.

What the export and simulator now know:

- Tiles block vehicles when their collision mask meets the car's: water, deep water, lava, hot
  lava, Gleba's deep lake. Gleba's wetland (friction modifier 8) and Fulgora's oil ocean (4 and
  10) and land (4) are drivable and slow. Factorio's own masks decide this, not tile names.
- Obstacles are every tree, plant, simple entity, cliff, spawner and lightning attractor whose
  mask meets the car's: Vulcanus rocks, chimneys and lichen trees; Gleba's plants, water canes,
  stromatolites and spawners; Fulgora's ruins, fulgurites and attractor ruins.
- Planet cliffs load by name. Crater cliffs have their own geometry; the others match Nauvis.
- Vulcanus demolisher territory is exported per chunk (`zone_chunks`) and every tile in it is a
  zone tile: the field routes around it, the policy sees it in its zone rays, and entering it
  costs the nest-zone penalty. Demolishers and pentapods themselves move and are not modeled.
- `distance_field.friction_cost` adds `weight * max(0, sqrt(g(m) / g(1.6)) - 1)` per tile, the
  vehicle's slowdown against grass (`g(m) = 1 + (m - 1) * terrain_friction_modifier`). With
  weight 1 the field estimates travel time, so routes leave deep oil ocean or wetland when firm
  ground is faster. The planet model and the mod use weight 1.

Training mixes pools by episode weight (`env.map_pools`): Nauvis 0.34, each planet 0.22.

## Verification against the game

- **Traces.** 90 planet calibration traces (acceleration on planet ground, crossing oil ocean,
  driving into lava and deep lakes, and impacts with planet trees, plants, rocks, ruins and
  cliffs, both vehicles) replay with zero position error in 79 cases. Four exceed 0.05 tiles,
  all low-speed contacts; the largest is 0.31 tiles (car into an ashland lichen tree). All 341
  Nauvis traces still replay exactly.
- **Map export.** A Vulcanus trip exposed that the exporter read chunks as they generated, while
  generating a chunk can still change cliffs in its neighbours. The planet pools kept 0.4-0.7%
  of cliff pieces that the finished map lacks. The exporter now generates the whole area first;
  a re-collected map matches a fully generated game surface on every cliff piece. The Nauvis
  pool was unaffected (all 1,136 cliff pieces of the seed-12345 map match).
- **Lua parity.** The mod's raster, field and observation match Rust exactly on Nauvis and
  planet maps, with and without the friction cost: zero difference in all features, field
  values and logits, and the same action in all 28 fixtures
  ([parity](results/planets-parity-2026-11-09.json)).
- **Engine trips.** `scripts/planet-trips.mjs` picks trips the simulator confirms reachable on
  the same seed's map; `scripts/replay-engine-trips.py` replays a run's trips in the simulator.
  Comparing the two found the two mod bugs below.

## Mod fixes found on the planets

1. **Ungenerated terrain.** The 0.7.1 mod treated every `water_tile` layer as impassable, so
   Gleba wetland and the oil ocean blocked routes. Detours reached ungenerated chunks, whose
   invalid tiles raised a Lua error that stopped the run on the 27th trip (it had arrived on 12
   of 26). Tiles now block by collision mask, and ungenerated chunks read as the map edge.
2. **Destinations beside obstacles.** A destination next to a plant or rock lies inside the
   inflated grid's blocked clearance, even in the field rooted at it. The corridor then fell
   back to its first section and routed from the target to an old sub-target and back (a
   150-tile route six tiles from the target, so the policy accelerated through it), and the
   final section never activated, which disabled arrival and left stall tracking on an earlier
   field. With the same shipped weights, in-game arrivals rose from 179/192 to 190/192.

## Simulator results by scenario

Holdout, seed 67890, 200 episodes per row; success, and mean time on successful episodes.

| Pool | Scenario | Shipped | `planets-v2` | A* |
|---|---|---:|---:|---:|
| Vulcanus | standard | 98.5% 4.3 s | 99.5% 4.2 s | 39.5% 12.7 s |
| | long | 85.5% 16.6 s | 93.5% 16.4 s | 51.0% 32.6 s |
| | forest | 100.0% 3.4 s | 100.0% 3.3 s | 64.5% 9.0 s |
| | reverse | 96.0% 3.9 s | 99.0% 3.8 s | 39.5% 10.5 s |
| | nest* | 97.5% 4.3 s | 100.0% 4.4 s | 39.5% 12.3 s |
| | arrival | 97.0% 3.2 s | 100.0% 3.1 s | 38.0% 9.7 s |
| Gleba | standard | 100.0% 4.4 s | 100.0% 4.2 s | 41.5% 13.2 s |
| | long | 98.5% 20.9 s | 99.5% 20.7 s | 62.5% 37.2 s |
| | forest | 99.5% 3.5 s | 100.0% 3.4 s | 55.0% 9.2 s |
| | reverse | 100.0% 3.8 s | 100.0% 3.6 s | 48.0% 10.6 s |
| | nest | 99.0% 9.3 s | 100.0% 9.2 s | 64.0% 19.8 s |
| | arrival | 100.0% 3.1 s | 100.0% 2.9 s | 44.5% 9.7 s |
| Fulgora | standard | 98.5% 4.2 s | 99.5% 3.8 s | 51.5% 12.9 s |
| | long | 97.0% 22.6 s | 97.0% 21.9 s | 65.5% 36.7 s |
| | forest* | 99.5% 4.0 s | 100.0% 3.8 s | 51.5% 12.4 s |
| | reverse | 95.0% 4.0 s | 97.0% 3.7 s | 47.0% 11.0 s |
| | arrival | 99.0% 2.7 s | 99.5% 2.4 s | 48.0% 10.0 s |
| Nauvis | all six | 99.6% 7.4 s | 99.8% 7.4 s | 41.4% 18.7 s |

\* Vulcanus has no nests and Fulgora has no trees or nests, so those kinds fall back to standard
episodes there (Fulgora's forest and nest rows are identical). Mean damage per episode rose
slightly on Vulcanus (1.07 to 1.51 health), Gleba (1.43 to 1.69) and Nauvis (0.63 to 0.77), mostly
on long trips, and fell on Fulgora (0.93 to 0.65).
[Full holdout results](results/planets-holdout-67890-2026-11-09.json).

## In-game results

Same 192 trips for both weight sets, mod 0.9.0 code
([engine results](results/planets-engine-2026-11-09.json)):

| Planet | Shipped weights | `planets-v2` | Time on common arrivals |
|---|---:|---:|---:|
| Vulcanus | 64/64 | 63/64 | -3.0% |
| Gleba | 63/64 | 64/64 | -1.0% |
| Fulgora | 64/64 | 64/64 | -2.8% |

Total damage over the planet's trips changed from 92 to 8 health on Vulcanus, 23 to 37 on Gleba
and 601 to 901 on Fulgora (mostly ruin contacts). On Nauvis, the existing short and long engine
suites arrive 24/24 with both the 0.7.1 mod and 0.9.0, with planning plus driving time within 1%
and no damage.

## Training

- `students/planets-v1`: `configs/student_mlp64_planets.toml` from `field-smooth-v1`, 30M
  steps, 512 envs, about 16k steps/s on the RTX 5060. The config builds on
  `student_mlp64_steady.toml`, so steering changes cost as in the shipped `field-steady-v1`
  (that checkpoint was not available here; `scripts/student-from-weights.py` rebuilds it from
  `weights.lua` for evaluation).
- `students/planets-v2`: 10M more steps on the corrected planet pools. Chosen over v1 on
  training-split validation (seed 54321): equal or better success on three pools, faster and
  with less damage on Vulcanus ([training](results/planets-training-2026-11-09.json)).

The teacher was not retrained: the student fine-tunes directly with PPO on the unchanged
observation, as `field-smooth-v1` did.

## What is still weak

- **Pinning against cliff ends.** The only in-game failure with the fixes (a tank on Vulcanus),
  and the stalls left once the corridor bug was fixed, are a vehicle pressed against a cliff end
  or rock, repeating a blocked reverse-and-turn. The simulator blocks the same poses, and the argmax policy has no way out
  of a state it maps to that action. A mod-side escape (change action after N blocked powered
  ticks) would likely remove these, but it is outside what the policy was trained with.
- **Vulcanus long trips** are the weakest simulator scenario (93.5%): long cliff mazes and
  territory detours.
- **Moving enemies** are not modeled. Demolisher territory is avoided as a zone, but a
  demolisher that leaves its territory, or Gleba's pentapods, can still attack.
- **Unreachable destinations.** On random clicks the mod reports most cut-off targets as
  unreachable, but some end in a stall or timeout after a detour attempt.
- **Interactive play** (GUI clicks, takeover keys, multiplayer) was not retested; all engine
  runs are headless.

## Reproduce

```sh
python3 scripts/collect-game-data.py --planets vulcanus,gleba,fulgora --map-count 40 --skip-traces
cd python
uv run fsim-rl train --config configs/student_mlp64_planets.toml \
  --init-from runs/students/field-smooth-v1/checkpoints/latest.pt --run-name students/planets-v1
uv run fsim-rl train --config configs/student_mlp64_planets.toml --set ppo.total_steps=10_000_000 \
  --init-from runs/students/planets-v1/checkpoints/latest.pt --run-name students/planets-v2
uv run fsim-rl suite --checkpoint runs/students/planets-v2/checkpoints/latest.pt --baseline \
  --split holdout --episodes 200 --seed 67890 --set env.map_pool=data/game/maps-vulcanus
uv run fsim-rl export-student --checkpoint runs/students/planets-v2/checkpoints/latest.pt \
  --out runs/students/planets-v2/planets-v2.json
cd ..
python3 scripts/build-autodrive-weights.py
node scripts/planet-trips.mjs 443791920 16 > trips.json
python3 scripts/test-autodrive.py --suite planets --seed 443791920 --trips trips.json --timeout 5300
python3 scripts/engine-summary.py new=data/autodrive/engine/driving-report.json
```
