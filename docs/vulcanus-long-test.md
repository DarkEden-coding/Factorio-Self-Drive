# Long-distance Vulcanus test set

A simulator test set for long trips on Vulcanus, where the shipped student hits rocks and cliffs
far more often than on the standard scenarios. It is a measurement only; nothing here changes
the model or the mod.

## The set

- **Pool:** `data/game/maps-vulcanus-open`, the 40 Vulcanus maps without demolisher territory
  (`scripts/strip-territory.py` drops each map's `zone_chunks`; tiles, entities and cliffs are
  unchanged). With territory, which covers about 85% of every map, long routes detour around
  it (median route 3.2 times the straight line); without it they cross the plains. Same
  train/holdout split as `maps-vulcanus`, so the train split can be used for training.
- **Scenario:** `long` with `min_distance = 500`, `max_distance = 1000`, `max_tortuosity = 3`
  (the training default is 200-700 tiles, and long is 2.5% of training episodes).
- **Episodes:** holdout split, seed 67890, 600 episodes over the training vehicle/fuel mix,
  deterministic student with the mod's escape rule.

```sh
python3 scripts/strip-territory.py data/game/maps-vulcanus data/game/maps-vulcanus-open
cd python
uv run fsim-rl eval --checkpoint runs/students/planets-v2/checkpoints/latest.pt \
  --split holdout --episodes 600 --seed 67890 --scenario long \
  --set env.map_pool=data/game/maps-vulcanus-open \
  --set 'env.scenarios=[{kind="long",min_distance=500,max_distance=1000,max_tortuosity=3.0,weight=1.0}]' \
  --out long.json --replays long-replays.json
cd ..
python3 scripts/impact-report.py python/long-replays.json
```

`scripts/impact-report.py` counts a hit when the speed falls by more than 0.08 tiles/tick
(17 km/h) within one decision from above 0.1 tiles/tick; braking cannot do that.

## `planets-v2` results (2026-11-09)

| Set | Success | Destroyed | Stalled | Mean damage | Hits >50 km/h per episode | Hits per 1000 tiles |
|---|---:|---:|---:|---:|---:|---:|
| Existing long scenario (200-700 tiles, territory) | 98.8% | 0.2% | 1.0% | 12.0 | 0.34 | 2.11 |
| Long-distance set (500-1000 tiles, no territory) | 91.8% | 1.3% | 6.8% | 44.9 | 1.26 | 4.13 |

By vehicle and fuel on the long-distance set:

| Vehicle/fuel | Success | Destroyed | Stalled | Mean damage | Damaged episodes | Top speed | Hits >50 km/h | Hits /1000 tiles |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| car/nuclear-fuel | 75.4% | 12.3% | 12.3% | 184.8 | 74% | 178 km/h | 2.68 | 7.80 |
| car/rocket-fuel | 91.7% | 1.7% | 6.7% | 145.9 | 68% | 162 km/h | 2.43 | 6.84 |
| car/solid-fuel | 98.5% | 0.0% | 1.5% | 45.7 | 46% | 129 km/h | 0.94 | 3.12 |
| car/coal | 91.8% | 0.0% | 8.2% | 31.5 | 51% | 111 km/h | 1.05 | 4.68 |
| car/wood | 98.5% | 0.0% | 1.5% | 39.3 | 47% | 113 km/h | 0.91 | 2.82 |
| tank/nuclear-fuel | 89.1% | 0.0% | 10.9% | 0.0 | 0% | 94 km/h | 1.74 | 5.68 |
| tank/rocket-fuel | 87.9% | 0.0% | 12.1% | 0.0 | 0% | 81 km/h | 1.64 | 5.58 |
| tank/solid-fuel | 93.8% | 0.0% | 6.2% | 0.0 | 0% | 62 km/h | 0.75 | 2.61 |
| tank/coal | 96.2% | 0.0% | 3.8% | 0.0 | 0% | 53 km/h | 0.37 | 1.56 |
| tank/wood | 93.4% | 0.0% | 6.6% | 0.0 | 0% | 53 km/h | 0.38 | 1.50 |

([full results](results/vulcanus-long-test-2026-11-09.json))

What the set shows:

- Hits and damage grow with top speed. Cars on nuclear and rocket fuel reach 160-180 km/h on
  open plains, where the student's 32-tile obstacle view is about half a second of travel,
  and they hit rocks and cliffs at speed: one nuclear car in eight is destroyed. In the real
  game a 1,700-tile nuclear trip ended the same way (destroyed after 520 tiles).
- Even slow vehicles hit about twice as often per tile as on the existing long scenario, and
  stalls rise to 7% (tanks on fast fuel 11-12%).
- The simulator records no damage for tanks, so their hits show only in the hit counts.
- Long trips are 2.5% of training episodes and none crossed open plains of this length:
  the existing pool routes around demolisher territory.

## Impact and recovery training (2026-11-09, simulator only)

In game, five cliff-maze trips stalled with `planets-long-b2`: a car hit a rock at about 80
km/h and stayed pinned. The simulator reproduces those states exactly, and showed the car only
needed to drive straight ahead, which the old escape rule (mirror the blocked action) never
tried. Two changes:

- The escape rule now tries the same throttle straight, the opposite throttle straight, then
  the opposite throttle with each steering, moving on as soon as one fails to move the vehicle.
  From the in-game wedge every model gets free in 1.8 s; on the long set stalls fall from 1.9%
  to 1.0% with b2.
- Two training scenarios (docs/rl.md): `impact` starts moving fast straight at an obstacle
  3-32 tiles ahead, `recover` starts at rest touching one. `configs/student_mlp64_planets_impact.toml`
  fine-tunes b2 with them (v1: 10% impact, 5% recover; v2: 18% and 10%; v3: 25% and 15% from
  b2; v2b: v2 for 30M more steps). Each 30M-step run takes about 16 minutes, two in parallel.

| | b2 | v2 | v2b (shipped) | v3 |
|---|---:|---:|---:|---:|
| Long set success | 98.9% | 98.7% | 98.8% | 99.1% |
| Nuclear car long: success, destroyed | 99.2%, 0.4% | 99.4%, 0% | 99.8%, 0% | 99.6%, 0.2% |
| Impact collisions per episode, Nauvis / Vulcanus / Gleba / Fulgora | 2.6 / 11.6 / 2.6 / 1.5 | 1.7 / 4.2 / 1.7 / 0.9 | 1.4 / 4.4 / 1.7 / 1.9 | 1.6 / 5.0 / 2.2 / 1.3 |
| Recover collisions per episode | 3.3 / 6.4 / 8.4 / 12.6 | 2.4 / 4.4 / 7.9 / 10.3 | 2.1 / 3.8 / 2.8 / 12.8 | 1.6 / 4.6 / 2.6 / 15.2 |

The other six scenario kinds stay at 99.5-100% on every pool. Damage in impact episodes falls
only a little: many starts are too close to stop, so the first hit is unavoidable. Not yet
verified in game ([results](results/impact-training-2026-11-09.json)).
