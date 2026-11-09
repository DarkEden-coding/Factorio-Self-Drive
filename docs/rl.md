# RL framework contract

This file is the contract between the Rust environment (`crates/fsim-core`, `crates/fsim-py`,
`crates/fsim-wasm`), the Python trainer (`python/`), and the viewer (`app/`). Change it here
first. Anything the policy sees or any rule that decides reward lives in Rust, so training,
evaluation and the viewer compute exactly the same thing.

## Episodes

- A map comes from the pool (`data/game/maps/index.json`). Each index entry has
  `"split": "train" | "holdout"`. Training uses `train`; evaluation defaults to `holdout`.
  `map_pools` (a list of `{pool, weight}`) replaces `map_pool` with several pools, such as
  Nauvis and one per Space Age planet (`data/game/maps-<planet>`). A reset picks a pool by
  weight (weights count episodes), then one of its maps uniformly. `info["map_index"]`
  counts entries of earlier pools first.
- Vehicle and fuel are sampled per episode from `vehicle_mix` (weighted list), and the scenario
  kind from `scenarios` (weighted list, see "Scenarios"). The rules below are the `standard` kind.
- Start: a random valid pose (collision-free hull, not in water, not in a nest zone) with a random
  orientation, inside the map with a margin so observations stay mostly on-map.
- Target: a point whose **path distance** from the start (see distance field) is within
  `[min_distance, max_distance]`, and whose tortuosity (path distance / straight distance) is at
  most `max_tortuosity`. Unreachable targets are rejected and resampled. Target radius `radius`.
- Termination: `success` when the vehicle has arrived under the `arrival` rule (below);
  `destroyed` when health reaches 0. Truncation: `timeout` after `base_ticks + per_tile_ticks * optimal_distance`
  ticks, or `stalled` when path distance hasn't improved by 1 tile for `stall_ticks`.
- Episodes are deterministic given (map, vehicle, fuel, start pose and speed, target, inputs).

Arrival rule (`arrival`, an episode rule like `reward`; defaults in brackets):

| Key | Meaning |
|---|---|
| `mode` ["enter"] | `enter`: success as soon as the vehicle center is within `radius` of the target. `stop`: success when the center is within `stop_radius` of the target point **and** \|speed\| <= `stop_speed`. Driving through the target is not success; the episode goes on so the vehicle can come back |
| `stop_radius` [1.0] | tiles |
| `stop_speed` [0.01] | tiles/tick (2.2 km/h). Braking ends at exactly 0, so a braked vehicle qualifies |

In `stop` mode, `radius` still marks the target area for the observation and the stop reward
terms. The speed check is per tick, so reversing direction inside `stop_radius` (speed passing 0)
also counts as stopping there.

Sampling and termination details (`sampler.rs`, `rl_episode.rs`):

- Starts lie at least 32 tiles inside the map, on Factorio's 1/256-tile position grid, with a
  uniformly random heading passed through the physics orientation quantizer. The start tile and its
  dilation neighborhood (see distance field) must be free for that vehicle.
- Targets are tile centers, drawn uniformly from tiles whose path distance from the start fits the
  range and tortuosity. The episode's own field (from the target) must confirm both for the start.
- Each sampled episode has a `seed`. The same seed, map, vehicle, fuel and settings give the same
  start and target, in the VecEnv and in wasm `sample_episode`.
- Conditions are checked after every tick, in the order destroyed, success (arrival rule),
  timeout, stalled. A
  decision stops at the tick that ends the episode. `stalled` means `stall_ticks` ticks since path
  distance last fell 1 tile below its best value.

## Scenarios

`scenarios` is a weighted list of tables `{kind, weight, ...params}`; each reset picks one kind by
weight (no draw when the list has one entry, so standard-only runs keep their episode sequences).
Missing parameters take the defaults below. Weights count episodes, not samples: a `long` episode
runs several times as many decisions as a `standard` one. If a kind finds no episode on
`EPISODE_ATTEMPTS` (32) random maps, the reset falls back to `standard`. `info["scenario"]` and the
record's `scenario` name the kind that ran. `spec()["scenarios"]` lists the kinds in index order:
`standard, long, forest, reverse, nest, arrival`. Every kind uses the standard start checks (open tile and
dilation neighborhood, clear hull, outside nest zones, `START_MARGIN`) unless stated otherwise, and
confirms the target on the episode's own field.

| Kind | Parameters [defaults] | Episode |
|---|---|---|
| `standard` | — | path distance in the curriculum range (`target`, `set_curriculum`) |
| `long` | `min_distance` [200], `max_distance` [700], `max_tortuosity` [3] | path distance in range; far tiles are thinned (kept with probability `min_distance / value`) so the distance is about uniform over the range |
| `forest` | `min_distance` [16], `max_distance` [96], `max_tortuosity` [2], `density_radius` [6], `start_density` [0.2], `route_density` [0.2], `route_fraction` [0.5] | tree density at a tile = fraction of tree tiles in the (2r+1)² square around it. The start tile has density >= `start_density`, the target tile >= `route_density`, and at least `route_fraction` of the field route's tiles have density >= `route_density` |
| `reverse` | `min_distance` [8], `max_distance` [48], `max_tortuosity` [3], `min_gap` [0.3], `max_gap` [3], `min_route_angle` [110], `route_lookahead` [8] | the vehicle faces a blocking obstacle (tree for the car only, rock, cliff, nest, water or map edge): sweeping the hull straight ahead in 0.05-tile steps touches it after `min_gap`..`max_gap` tiles. The field route's point `route_lookahead` tiles from the start lies at least `min_route_angle` degrees off the heading (behind or beside). Speed 0. The start tile need not be open (the hull must be clear) and may lie in a nest zone |
| `nest` | `min_distance` [24], `max_distance` [160], `min_zone_tiles` [4], `max_detour` [1.6] | start 4-40 tiles outside a random nest's zone; the straight line to the target passes within that zone's radius; the zone-free field route (nest_zone_cost 0) has >= `min_zone_tiles` zone tiles and a path distance in range; the episode field route has no zone tiles and is at most `max_detour` times as long (with `nest_zone_cost` 0 the zone-skirting route below stands in, so zone-blind fields can be evaluated on the same episodes); keeping out of zones (no margin) costs >= 2 more than the zone-free route, so every zone-free shortest route enters a zone. The target is not in a zone |
| `arrival` | `min_distance` [6], `max_distance` [40], `max_tortuosity` [2], `moving_fraction` [0.4], `backward_fraction` [0.2], `max_start_speed` [0.25] | a standard-rules episode with a short path distance, so most decisions are approach and braking. Then one draw: with probability `moving_fraction` the heading points at the field route's point 4 tiles ahead (+-7 degrees jitter) and the start speed is uniform in [0, `max_start_speed`] tiles/tick (0.25 = 54 km/h; above a slow fuel's top speed the vehicle just slows down); with probability `backward_fraction` the heading points the opposite way at speed 0, so backing into the target is the short way; otherwise the random heading at speed 0 stays. If the turned hull isn't clear, the random heading at speed 0 stays |

Sampler acceptance and cost on the 96 train maps (`rl-benchmark --scenarios-only`, one thread,
default parameters, coal): "map success" is the share of maps where the kind found an episode,
"start accept" valid starts per drawn pose, "target accept" accepted targets per confirming field.

| scenario | vehicle | map success | start accept | target accept | ms/sample | ms/reset | fallback |
|---|---|---:|---:|---:|---:|---:|---:|
| standard | car | 100% | 65.6% | 99.0% | 0.60 | 0.71 | 0% |
| standard | tank | 100% | 69.9% | 99.0% | 0.66 | 0.80 | 0% |
| long | car | 100% | 65.6% | 91.0% | 18.3 | 18.6 | 0% |
| long | tank | 100% | 69.8% | 99.0% | 23.6 | 23.1 | 0% |
| forest | car | 79.7% | 1.2% | 45.0% | 10.8 | 15.4 | 0% |
| forest | tank | 100% | 2.3% | 53.6% | 4.9 | 5.8 | 0% |
| reverse | car | 100% | 10.6% | 77.4% | 0.53 | 0.59 | 0% |
| reverse | tank | 100% | 3.9% | 79.7% | 0.60 | 0.70 | 0% |
| nest | car | 100% | 57.5% | 47.6% | 3.1 | 2.9 | 0% |
| nest | tank | 100% | 63.6% | 58.7% | 3.5 | 3.3 | 0% |

Starts for `forest`, `reverse` and `nest` are drawn with cheap tile tests first, so low start
acceptance costs little. `long` pays for map-sized fields (two per episode); its episodes last
hundreds of decisions, so the cost per decision stays small.

## Distance field

Computed once per episode on the tile grid, 8-connected Dijkstra from the target over a region
covering start and target plus a margin (default 48 tiles). Blocked cells: water (any tile whose
collision mask meets the car's: water, deep water, lava, Gleba's deep lakes), cliffs, nests,
rocks; trees are blocked for the car and cost `tank_tree_cost` extra per tile for the tank.
Vulcanus demolisher territory counts as nest zone, tile by tile (whole chunks).
Obstacles are dilated by the vehicle's half-width (rounded up) so the field never routes through
gaps the hull can't pass. Tiles within `nest_zone_margin` (default 2) tiles of a nest-zone tile cost
`nest_zone_cost` (default 3) extra, so the field, and with it the progress reward and the A*
baseline, leads around nest zones when the detour is shorter than about 3x the distance inside the
zone. The margin keeps routes, and vehicles cutting their corners, off the zone edge. The field's
value at the vehicle position (bilinear) is the `path_distance`; at the start it is
`optimal_distance`. The A* baseline follows this field.

`path_distance` and `optimal_distance` are costs: tree and zone tiles count extra. The geometric
length of the field route from the start (steepest descent through tile centers, plus the legs to
the start and target points) is `optimal_length`; SPL uses it.

- Values are at tile centers; infinity marks blocked or unreachable tiles. The target tile starts
  at the straight distance from the target to its center.
- Steps cost 1 or sqrt(2) times `1 + (e_a + e_b) / 2`, where `e` is `tank_tree_cost` on tank tree
  tiles plus `nest_zone_cost` on (dilated) zone tiles, plus the slow-ground cost, and 0 otherwise.
  The slow-ground cost (`friction_cost` [0], off by default) is
  `friction_cost * max(0, sqrt(g(m) / g(1.6)) - 1)` for a tile with `vehicle_friction_modifier`
  `m`, where `g(m) = 1 + (m - 1) * terrain_friction_modifier` of the vehicle: how much slower
  the vehicle's terminal speed is than on grass. With weight 1 the field is a travel-time
  estimate, so routes leave Fulgora's oil ocean (modifier 10) or Gleba's wetland (8) when a
  detour on firm ground is faster. The game's pathfinder stand-in (`pathfinder.rs`) ignores it. The tree cost is dilated
  with the obstacles. Diagonal steps need both orthogonal neighbors open (no corner cutting).
- Interpolation uses only finite corners. If none applies, the value is the best finite tile within
  3 tiles plus the straight distance to its center. If the vehicle reaches a tile with no finite
  value nearby, `path_distance` keeps its last finite value.
- An unreachable target makes `start_episode` fail.


## Reward

Per decision step, weights from `reward` config (defaults in brackets):

| Term | Value |
|---|---|
| `progress` [0.1] | per tile of path-distance decrease (negative when moving away) |
| `time` [-0.005] | per decision step |
| `path_length` [0.0] | per tile actually traveled during the decision, including reverse travel |
| `steer_change` [0.0] | per steering step changed from the previous decision (left to right is 2) |
| `heading_error` [0.0] | reactive mode: per radian between the heading and the route point 8 tiles ahead, after each decision driven forward |
| `damage` [-5.0] | times damage taken / max health |
| `collision` [-0.2] | per decision step with a new collision event |
| `nest_zone` [-0.15] | per decision step with the vehicle center inside a nest penalty radius |
| `success` [10.0] | once, on success |
| `destroyed` [-10.0] | once, on destruction |
| `arrival` [2.0] | `stop` arrival only: once, the first time the vehicle center comes within `radius` |
| `target_speed` [-1.0] | `stop` arrival only: per decision ending with the center within `radius`, times \|speed\| in tiles/tick (-0.3 at 65 km/h) |

The env reports each term separately (info) and their sum (reward). `damage` uses health lost
during the decision. `collision` counts collisions with entities, water and the map edge.
`progress` and `nest_zone` use the state at the end of the decision. `path_length` sums
actual displacement at every physics tick, so circling and backing up cannot cancel travel.
Its zero default preserves rewards for older configs and checkpoints. The supplemental
`student_mlp64_efficient.toml` uses -0.02 per tile, time -0.02 per decision, and collision -0.75.
Observation features and their spec hashes are unchanged.

Why the stop terms: with `stop`, `progress` already measures to the target point (the field is 0
there), so overshooting costs the progress it earned. `arrival` keeps a small reward for getting
into the target area, so the speed penalty doesn't teach the policy to stay out of it.
`target_speed` makes every decision spent fast inside the area cost something, which rewards
braking before and inside it; driving through the 6-tile disk at 65 km/h costs about 1.5. The
`success` bonus, now paid only for stopping, is what the stop is mainly worth.

Why these defaults (they were -0.05 and -0.05, with a field that ignored nest zones):

- A policy that drives about 1.2 tiles per decision earns 0.1 per tile, so a -0.05 contact cost
  half a tile of progress, less than the time a shortcut through trees saves. At -0.2 a contact
  costs 2 tiles, and pressing against an obstacle costs that every decision, which pays for
  backing off.
- Progress is the dominant dense signal, and with gamma 0.99 the policy follows the field's
  gradient. While the field crossed nest zones, the progress reward paid for driving through them
  and -0.05 per decision could not compete. With `nest_zone_cost` the gradient leads around the
  zone, and -0.15 per decision makes a 36-tile crossing (about 30 decisions) cost about 4.5, half
  the success bonus, against well under 1 for a typical detour.
- `damage`, `success` and `destroyed` are unchanged. `python/configs/finetune_strong.toml` raises
  collision, nest zone and damage further (-0.5, -0.3, -10).

## Observation

All in vehicle-centered grids, world-aligned (north up), crop **snapped to the vehicle's tile**
(the sub-tile offset is a vector feature). Values are `u8`; the model divides by 255.

- `local`: `u8 [C, 64, 64]`, 1 tile per cell.
- `coarse`: `u8 [C, 32, 32]`, 8x8 tiles per cell, cell value = mean of the fine cells.
- Channels (C = 8): `blocked, cliff, tree, rock, water, nest_zone, friction, target`.
  Binary channels are 0 or 255. `friction` = round(clamp((modifier - 1.0) / 1.0, 0, 1) * 255).
  Off-map cells are blocked and water, with friction 0. Entity boxes crossing the map edge are not
  drawn into off-map cells.
- Local cell `(row, col)` is tile `(tx - 32 + col, ty - 32 + row)` for vehicle tile `(tx, ty)`.
  Coarse cells cover tiles `[tx - 128, tx + 128)`. A coarse value is `(sum of 64 fine values + 32) / 64`.
- `vector`: `f32 [V]`, names in `spec()["vector_features"]`. It includes at least: speed, sin/cos
  of heading, target offset in the vehicle frame and its distance, straight distance and path
  distance to target, health fraction, sub-tile offset, vehicle and fuel stats (normalized), tile
  friction under the vehicle, previous action one-hot, fraction of time limit used.

  The current 39 features, in order:

  | Feature | Value |
  |---|---|
  | `speed` | signed km/h / 100 |
  | `heading_sin`, `heading_cos` | sin, cos of 2*pi*orientation (world frame, north = 0) |
  | `target_right`, `target_forward` | target offset in the vehicle frame / 64, clamped to +-4 |
  | `target_dir_right`, `target_dir_forward` | unit vector toward the target, vehicle frame |
  | `straight_distance`, `path_distance` | tiles / 64, clamped to [0, 8] |
  | `path_dir_right`, `path_dir_forward` | unit vector toward the tile 4 steps down the field's steepest descent |
  | `target_radius` | radius / 8 |
  | `health` | health / max health |
  | `subtile_x`, `subtile_y` | x - floor(x), y - floor(y) |
  | `vehicle_tank` | 1 for tank steering, else 0 |
  | `vehicle_weight`, `vehicle_max_health` | / 20000, / 2000 |
  | `vehicle_rotation_speed`, `vehicle_braking_power` | / 0.015, / 1e6 W |
  | `vehicle_power_to_weight` | engine power incl. fuel multipliers / weight / 100 |
  | `vehicle_top_speed` | grass terminal speed with this fuel, km/h / 100 |
  | `vehicle_half_width`, `vehicle_half_length` | tiles |
  | `fuel_acceleration`, `fuel_top_speed` | fuel multipliers |
  | `tile_friction` | modifier under the vehicle center - 1 |
  | `prev_action_0` .. `prev_action_8` | one-hot of the previous decision's action (zeros at start) |
  | `time_fraction` | episode ticks / time limit ticks |
  | `stall_fraction` | ticks since path distance last improved / `stall_ticks` |
  | `in_nest_zone` | 1 inside a live nest's penalty radius |

Static channels are rasterized once per map and cropped each step; destroyed entities update the
cached raster. The shared raster holds the map as loaded. Each env patches the tiles under
entities it destroyed, so envs on the same map don't affect each other.

`spec()` returns `local_shape`, `coarse_shape`, `coarse_cell_tiles`, `channels`,
`vector_features`, `vector_size`, `actions`, `reward_terms`, `modes`, `scenarios`,
`collision_kinds`, `vehicles`, `fuels` and `spec_hash`. `spec_hash` is the FNV-1a 64-bit hex hash
of the JSON of the shapes, channels, vector feature names and action layout (currently
`16a3d4353d1658c1`; scenarios and rewards don't change it).

## Student observation

A compact observation for small policies that run inside the Factorio mod in Lua (distilled from a
full-observation teacher, see "Students"). Everything is in the vehicle frame, so it needs no
grids: ray casts over a small cached obstacle grid, a few points along the route, and vehicle
state. The config key `observation` picks what the VecEnv returns: `"full"` (default: `local`,
`coarse`, `vector`), `"student"` (only `student`, `f32 [N, S]`), or `"both"` (all four, for
distillation). Final observations follow the same keys (`info["final_student"]`). The teacher's
layout and `spec_hash` don't change; `spec()["student"]` holds `features`, `ray_angles_deg`,
`ray_range`, `ray_channels`, `route_lookaheads`, `size` and its own `spec_hash` (FNV-1a 64 of the
JSON of the layout). Code: `crates/fsim-core/src/student_obs.rs`; wasm `policy_student()`.

Frame: with `h = 2 * pi * orientation`, world x east and y south, forward is `(sin h, -cos h)` and
right is `(cos h, sin h)`. A point `p` relative to the vehicle center `c` has
`right = (p - c) . (cos h, sin h)` and `forward = (p - c) . (sin h, -cos h)`.

The S = 135 features, in order:

| Features | Value |
|---|---|
| `hard_0` .. `hard_31` | ray clearance to the first hard tile, / `ray_range` |
| `tree_0` .. `tree_31` | ray clearance to the first tree tile, at most the hard one, / `ray_range` |
| `zone_0` .. `zone_31` | ray clearance to the first nest-zone tile, at most the hard one, / `ray_range` |
| `route4_right`, `route4_forward`, ... `route32_*` | route point at arc length s = 4, 8, 16, 32, vehicle frame, / s |
| `route_length` | geometric length of the whole route, tiles / 64, clamped to [0, 8] |
| `target_dir_right` .. `in_nest_zone` | 30 features copied from the teacher's vector (table above): `target_dir_right`, `target_dir_forward`, `straight_distance`, `target_radius`, `speed`, `health`, `vehicle_tank`, `vehicle_weight`, `vehicle_max_health`, `vehicle_rotation_speed`, `vehicle_braking_power`, `vehicle_power_to_weight`, `vehicle_top_speed`, `vehicle_half_width`, `vehicle_half_length`, `fuel_acceleration`, `fuel_top_speed`, `tile_friction`, `prev_action_0` .. `prev_action_8`, `time_fraction`, `stall_fraction`, `in_nest_zone` |

Rays:

- 32 rays at these angles in degrees from forward, positive to the right, in this (circular)
  order, index 0..31: `-164 -144 -124 -106 -90 -76 -64 -53 -43 -34 -26 -19 -13 -8 -4 0 4 8 13 19
  26 34 43 53 64 76 90 106 124 144 164 180`. Ray `i` with angle `a` has world direction
  `(sin(h + a), -cos(h + a))`. `ray_range` R = 32 tiles.
- Tile classes, per tile of the live raster (`raster.rs` bits, with this env's destroyed-entity
  patches): **hard** = cliff, rock, water, nest collision box, off-map, or blocked by an entity
  that isn't a tree; **tree** = a live tree's box touches the tile; **zone** = the tile center is
  within a live nest's penalty radius.
- Each ray walks the tiles it crosses from the vehicle center (Amanatides-Woo grid traversal): the
  start tile has `t = 0`, every other tile the ray parameter (tiles) where the ray enters it. Step
  in x when the next x boundary is strictly nearer than the next y boundary, else in y. The walk
  stops at the first hard tile (`t_hard = t`) or when `t >= extent + R` (no hard hit:
  `t_hard = extent + R`). `t_tree` / `t_zone` are the first tree / zone tile's `t` if it comes
  before the stop, else `t_hard`.
- `extent = min(half_width / |sin a|, half_length / |cos a|)` is the hull's reach along the ray
  (the `vehicle_half_*` features). Each feature is `clamp(t - extent, 0, R) / R`: 0 = touching
  or inside, 1 = nothing within R tiles of the hull.

Route: the polyline from the vehicle center through the tile centers of the episode field's
steepest descent to the target point (the A* baseline path, `baseline_path()` in wasm). The point
at arc length `s` along it, or its end when it is shorter than `s`, goes to the vehicle frame and
is divided by `s`, so a straight route ahead gives (0, 1) and the target closer than `s` gives a
shorter vector. In game, `mod/factorio-autodrive` computes the same bounded, hull-dilated,
zone-aware Dijkstra field and descent polyline in Lua. Plain `request_path`
waypoints do not preserve the student's zone avoidance; see `docs/student-results.md`
and `docs/autodrive-mod.md`.

Lua port notes: keep a per-tile 3-bit grid (hard, tree, zone) around the vehicle, e.g. 80x80
tiles, rebuilt from `find_entities_filtered` / `get_tile` when the vehicle moves 8 tiles from the
grid's center or an entity in it dies; rays read only that grid. `time_fraction` and
`stall_fraction` need the mod's own time limit and stall timer (as `retarget` in wasm, both start
at 0). Rust cost on one sample state (`rl-benchmark --quick`, Linux box): 9.0 us per student
observation (1371 ray tiles) against 47.1 us for `local + coarse + vector`.

### Student map

`student_map = true` appends a coarse map to each student row (903 features, `spec["student"]["map"]`
with its own `spec_hash`): 16x16 cells of 8x8 tiles centered on the vehicle in its frame (row 0
ahead, column 0 left), channel-major in the ray channels (hard, tree, zone). A cell holds the
fraction of its 4x4 sample points (2 tiles apart) that are of that kind. It is an experiment in
replacing the route features with a wider view: `model.student_route = false` drops the route
from the actor and `model.map_conv` runs stride-2 convs over the map (`configs/student_map.toml`).
`export-student` doesn't support it yet.

### Any-angle field route

`route_source = "field_smooth"` gives the student cost-aware arbitrary-angle shortcuts over
its episode field. Segment traversal checks the vehicle-inflated obstacle grid, including
corner/border touches, and integrates tree/zone costs. A shortcut cannot exceed the cost of
the raw span it replaces. Blocked spans retain closer waypoints; open routes can connect
start and target directly. The field and sampling/progress rules remain unchanged.

The vector still has 135 features, but route semantics use `student.smooth_spec_hash`
(`d1c084cc679f1f71`), or the corresponding map hash when a map is appended. Dense grid students
may initialize smooth training explicitly with `--init-from`; a resumed run must match its
contract. Exported model metadata includes route source and field parameters. See
[smooth-route-results.md](smooth-route-results.md) for the current model and measurements.

### Pathfinder route

`route_source = "pathfinder"` (needs `student_map`) takes the student's route features from a
stand-in for Factorio's `LuaSurface.request_path` (`pathfinder.rs`) instead of the episode field:
a target-rooted field with trees blocking (costing, if they wall in a tank) and no nest-zone
cost, its descent from the request point simplified to straight segments (Douglas-Peucker,
1 tile), requested again when the vehicle is more than 16 tiles from the waypoints or reaches
their end. Requests are instant here; in game they take some ticks. Spec hash:
`spec["student"]["map"]["pathfinder_spec_hash"]`; config `configs/student_map_path.toml`.

## Students

`fsim-rl distill --teacher <ckpt> --student-config configs/student_<size>.toml` trains a student
(`model.arch = "student"`: an MLP actor `student_hidden`, optional circular 1D convs `ray_conv`
over the rays, a separate training-only critic `critic_hidden`) by DAgger: per iteration it rolls
out `distill.num_envs` envs (observation `both`) for `rollout_steps`, executing the teacher's
argmax with probability beta (linear from `beta_start` to 0 over `beta_iterations`) and the
student's sampled action otherwise, labels every state with the teacher's log-probs and value,
and fits `KL(teacher || student) + value_coef * (V - V_teacher)^2` on minibatches from the
aggregated buffer (the last `buffer_size` samples). Standard episodes use the curriculum stage the
teacher reached. It stops after `iterations` or when the smoothed KL on fresh rollouts (measured
before training on them) hasn't improved 2% in `patience` iterations; `best/model.pt` has the
lowest one. `fsim-rl train --config <student config> --init-from <distilled>` fine-tunes with
PPO; `ppo.teacher_kl_coef > 0` adds `coef * KL(teacher || policy)` with the teacher from
`ppo.teacher_checkpoint` (the env then returns both observations). Student checkpoints store the
student `spec_hash`; `eval`, `suite` and `train --init-from` check it.

`fsim-rl export-student --checkpoint <ckpt> --out student.json` writes the actor for the mod:
`{format: "fsim-student-v1", spec_hash, obs (spec()["student"] plus ray_count, ray_channels_count),
actions, decision_ticks, normalizer: {mean, std, clip}, ray_conv: [{in, out, kernel, stride,
length_in, length_out, weight[out][in][k], bias}], dense: [{in, out, weight[out][in], bias}],
cost}` with weights rounded to 6 significant digits. `python/src/fsim_rl/student_ref.py` is the
reference forward pass in plain loops (the computation is documented there); tests check it
against torch. The command prints the file size, parameter and multiply-add counts and a Lua time
estimate (assumption: 20 M simple ops/s; 2 per multiply-add, 1 per normalized input or ReLU, 8 per
ray tile, worst-case ray tiles unless `--ray-tiles` is given).

## Action modes

`decision_ticks` (default 4) ticks per decision. The 9 discrete actions are
`index = (throttle + 1) * 3 + (steer + 1)`, throttle and steer in {-1, 0, 1}.

### Partial steering (`steer_levels`)

`steer_levels = L` (default 1; must divide `decision_ticks`) gives each side L steering strengths.
There are `3 * (2L + 1)` actions, `index = (throttle + 1) * (2L + 1) + steer + L` with steer in
-L..L. Strength k steers the first `k * decision_ticks / L` ticks of the decision and holds
straight for the rest, so L = 4 turns in 1-tick steps instead of the 4-tick step (about 15 degrees
for the car at 104 km/h). `steer_change` is scaled by 1/L; the previous-action features and the
exported `spec_hash` keep the classic 9-way layout (throttle and the sign of the steering).
`widen_steer_head` warm-starts an L-level student from a classic checkpoint. The game still
snaps the heading to the 45-degree axes when not steering, which undoes about half of the
1-tick turns made near an axis. The viewer and the mod only support `steer_levels = 1`.

| Mode | Action per env step | What the env executes |
|---|---|---|
| `reactive` | `int64 [N]` | one decision |
| `direct_plan` | `int64 [N, H]` (H = `horizon`, default 24 = 1.6 s) | the first `execute` (default 4) decisions, fewer if the episode ends |
| `path_planner` | `float32 [N, K, 2]` waypoints (K default 8) in the vehicle frame, x = right, y = forward, tiles, clipped to `max_reach` (default 100) | pure-pursuit controller (Rust) picks one of the 9 actions per decision until `replan_ticks` (default 30) elapsed, the vehicle drifts more than `drift_tiles` (default 3) from the path, the last waypoint is reached, or the episode ends |

`path_planner` reaches its last waypoint within 2 tiles. Waypoints are clipped to `max_reach` by
length, toward the origin. The controller steers toward a lookahead point 2.5 + 12 * |speed| tiles
(at most 8) along the path. It picks a speed the vehicle can turn at, capped at 0.3 tiles/tick.
After a contact it reverses for up to 30 ticks while turning toward the path.

For macro steps (`direct_plan`, `path_planner`) the env returns the discounted reward sum
`sum(gamma^i * r_i)` over the executed decisions and `discount = gamma^k` (k decisions executed),
so the trainer bootstraps with `discount * V(next)`. For `reactive`, k = 1.

## Python bindings (`import fsim`, crate `crates/fsim-py`, built with maturin)

```python
fsim.spec() -> dict             # shapes, channel names, vector_features, action info, spec_hash
env = fsim.VecEnv(config_json: str, num_envs: int, seed: int)
env.reset() -> obs               # dict: local u8 [N,C,64,64], coarse u8 [N,C,32,32], vector f32 [N,V]
                                 # (and/or student f32 [N,S], per the config's "observation")
env.step(actions) -> (obs, reward f32 [N], discount f32 [N], terminated bool [N], truncated bool [N], info)
env.set_curriculum(min_distance: float, max_distance: float, max_tortuosity: float)
env.baseline_actions() -> actions   # A* + pure pursuit for the current state, in the env's mode
env.episode_records() -> list[dict] # finished-episode records since last call, each with "env_index"; ordered by finish step, then env index
env.set_recording(enabled: bool)
```

- Envs auto-reset. `obs` after a done step is the new episode's first observation; the finished
  episode's last observation is in `info["final_local"]`, `info["final_coarse"]`,
  `info["final_vector"]` (rows valid where done).
- `info` arrays `[N]`: `success, destroyed, timeout, stalled, path_distance, optimal_distance,
  path_length, ticks, damage, collisions, map_index, vehicle_index, x, y, orientation`, and
  `reward_<term>` per reward term.
  - `reward_<term>` is this step's amount (same discounting as `reward`), not an episode total.
  - On done rows, the episode fields (`path_distance`, `path_length` = distance actually driven,
    `ticks`, `damage`, `collisions`, `map_index`, `vehicle_index`) describe the finished episode.
  - `x, y, orientation` are the vehicle pose for the returned `obs` (the new episode's start on
    done rows), so the caller knows the pose at its next policy call.
  - `vehicle_index` indexes `spec()["vehicles"]`.
- Stepping runs envs in parallel (rayon) and writes straight into NumPy buffers.
- Additional `info` arrays: `fuel_index` (indexes `spec()["fuels"]`) and `decisions` (decisions
  executed this step, k). Bool arrays are `bool`, counts and indices `int64`, others `float32`.
- Episode fields for evaluation (like the other episode fields, they describe the finished episode
  on done rows): `scenario` (indexes `spec()["scenarios"]`), `optimal_length`,
  `collisions_<kind>` for `tree, rock, cliff, nest, water` (collision ticks by kind; they sum to
  `collisions`, the map edge counts as water), `nest_zone_ticks` and `nest_zone_decisions` (ticks
  and decisions ending with the vehicle center in a live nest's penalty radius),
  `reverse_distance` (tiles moved against the heading) and `reversed` (`reverse_distance >= 0.5`),
  `speed` (\|speed\| in tiles/tick) and `target_distance` (vehicle center to target point, tiles)
  at the end of the step.
- `actions` may be any array-like; it is converted to contiguous `int64` / `float32` and must
  have exactly the mode's shape. Every `direct_plan` entry must be 0..8, executed or not.
- Errors (bad config, actions or curriculum) raise `ValueError`. Unknown config keys are errors.
  `map_pool` is resolved against the current directory.
- `env.num_envs`, `env.mode` and `env.map_names` (`[(pool_index, name)]`) are read-only properties.
- A VecEnv holds every map of its split in memory, about 13 MB per 1056 x 1056 map (about 1.2 GiB
  for the 96 train maps).

`config_json` (Python fills defaults from its TOML config):

```json
{
  "map_pool": "data/game/maps", "map_pools": [], "split": "train",
  "mode": "reactive", "decision_ticks": 4, "gamma": 0.99,
  "vehicle_mix": [{"vehicle": "car", "fuel": "coal", "weight": 1.0}],
  "target": {"min_distance": 16, "max_distance": 64, "max_tortuosity": 1.5, "radius": 3},
  "time_limit": {"base_ticks": 600, "per_tile_ticks": 12, "stall_ticks": 1200},
  "reward": {"progress": 0.1, "time": -0.005, "damage": -5.0, "collision": -0.2,
             "nest_zone": -0.15, "success": 10.0, "destroyed": -10.0,
             "arrival": 2.0, "target_speed": -1.0},
  "distance_field": {"margin": 48, "tank_tree_cost": 4.0, "nest_zone_cost": 3.0, "nest_zone_margin": 2,
                     "friction_cost": 0.0},
  "direct_plan": {"horizon": 24, "execute": 4},
  "path_planner": {"waypoints": 8, "max_reach": 100, "replan_ticks": 30, "drift_tiles": 3},
  "scenarios": [{"kind": "standard", "weight": 1.0}],
  "observation": "full",
  "arrival": {"mode": "enter", "stop_radius": 1.0, "stop_speed": 0.01}
}
```

## Episode records and replays

An episode record (and a replay file, a JSON list of records) contains: map name, vehicle, fuel,
start pose, target, radius, mode, `decision_ticks`, the executed per-decision actions, and per
policy call an overlay: `{decision, probs?, value?, plan_actions?, waypoints?}` (waypoints in world
coordinates).

Rust records are JSON objects with `map, map_index, env_index, seed, scenario, vehicle, fuel, start,
start_speed, target, radius, mode, decision_ticks, gamma, reward, time_limit, distance_field,
arrival, actions, overlays, result` (`start_speed` only when nonzero; records without `arrival`
replay with `enter`). `result` is `{status, ticks, decisions, reward, path_distance, optimal_distance, damage,
collisions, path_length, optimal_length, collisions_by_kind: {tree, rock, cliff, nest, water},
nest_zone_ticks, nest_zone_decisions, reverse_distance, final: {x, y, orientation, speed, health}}`,
where `reward` holds undiscounted totals by term. Records written before `nest_zone_cost` existed
replay with today's default field: actions and the final pose match, but rewards and the stall
timer follow the new field. While recording, Rust writes one overlay per policy call with
`decision`, plus `plan_actions` (the full `direct_plan` plan) or `waypoints` (the clipped
`path_planner` waypoints in world coordinates). A record is valid `start_episode` input. Python adds model outputs to the overlays and converts waypoints to world coordinates using the
pose from `info` (or the record's `start` for the first call). The viewer replays by starting the same
episode in wasm and applying the actions; determinism makes it match.

## WASM additions (`Sim`)

Same Rust code as the env:

- `start_episode(json)`: `{start: {x, y, orientation}, start_speed?, target: {x, y}, radius, mode, decision_ticks, reward?, time_limit?, distance_field?, arrival?}`; computes the distance field.
  Every key except `start` and `target` is optional: `radius` defaults to 3, rules to the
  config defaults, and `vehicle`/`fuel` to the current vehicle. Record-only keys (`map_index`,
  `env_index`, `scenario`, `actions`, `overlays`, `result`) are accepted and ignored. `gamma`, `direct_plan` and
  `path_planner` are also accepted. If `map` is given, it must equal the loaded map's name.
  Restores destroyed obstacles first. Records and the returned episode JSON are valid input.
- `sample_episode(json)`: same sampler as training (`{seed, min_distance, max_distance, max_tortuosity, radius}`); starts and returns the episode JSON.
  Uses the current vehicle and fuel. Range keys default to the config defaults; the optional rule
  keys of `start_episode` are accepted.
- Both return the episode JSON: setup (`map, vehicle, fuel, start, start_speed?, target, radius,
  seed?`), rules (`mode, decision_ticks, gamma, reward, time_limit, distance_field, direct_plan,
  path_planner, arrival`),
  `optimal_distance` and `time_limit_ticks`. They also set `target_state()`.
- `retarget(x, y, radius?, rules_json?)`: the viewer's live session. Starts an episode toward
  `(x, y)` from the vehicle's **current** state: pose, speed, health and destroyed obstacles are
  kept. The distance field is recomputed from the current position, and the bookkeeping starts
  over: `ticks`, time limit (`base_ticks + per_tile_ticks * optimal_distance`), stall timer,
  reward totals, decisions and previous action. `start` in the episode JSON is the current pose and
  there is no `seed`. `radius` defaults to 3. `rules_json` holds optional rule keys (as in
  `start_episode`); missing keys keep the current episode's rules, or the config defaults.
  Returns the episode JSON plus `"reachable": true`. An unreachable target (blocked, off the map,
  or cut off) returns `{"reachable": false}` and leaves the current episode, target and vehicle
  unchanged. Throws for a destroyed vehicle, non-finite input or bad rules.
  Observations, `decide`, `follow_path`, `plan_waypoints`, `predict_path` and `baseline_path`
  work as in a sampled episode. The `time_fraction` and `stall_fraction` features restart at 0,
  which keeps a retargeted episode inside the training distribution (an episode's start state
  differs only in nonzero speed).
- `place_random(seed: u32)`: moves the vehicle to a random start pose with the sampler's start
  rules (map margin, open tile and dilation neighborhood, collision-free hull, not in a nest zone,
  random quantized heading), full health and zero speed. Restores destroyed obstacles, ends the
  episode and clears the target. Returns `{x, y, orientation}`. Deterministic in `seed`; it draws
  up to 100,000 poses and throws only if none is valid.
- `episode_state()`: JSON with reward totals by term, `path_distance`, `optimal_distance`, status.
  Full shape: `{status, ticks, decisions, time_limit_ticks, path_distance, optimal_distance,
  reward: {<term>: total}, reward_total, damage, collisions, prev_action}`, or `null` without an episode.
- `policy_local()`, `policy_coarse()` (`Uint8Array`), `policy_vector()`, `policy_student()` (`Float32Array`), `spec()` (JSON incl. `spec_hash`).
- `decide(action_index)`: run one decision (`decision_ticks` ticks) with reward bookkeeping.
  Returns `{action, reward, terms: {<term>: value}, ticks, status, done}`.
- `follow_path(waypoints: Float32Array)`: one pure-pursuit decision; returns JSON `{action, replan}`.
  Waypoints are **world** x,y pairs. Calling again with an identical array continues the plan and
  its replan timer; a different array starts a new plan from the current pose. The result also
  has the `decide` fields. `replan` uses the env's rules.
- `plan_waypoints(local: Float32Array)`: vehicle-frame waypoints (a `path_planner` action from the
  server, flattened) to world x,y at the current pose, clipped to `max_reach`. Feed the result to
  `follow_path`.
- `predict_path(actions: Uint8Array)`: roll the 9-way actions forward on a copy of the vehicle (world unchanged); returns `Float32Array` of x,y per decision, stopping at the first collision.
  The colliding decision is not included. Uses the episode's `decision_ticks`, or 4 without an episode.
- `baseline_path()`: `Float32Array` of A* waypoints (world x,y) from the vehicle to the target.
  These are tile centers down the field from the vehicle, then the target point.
- `distance_field()`: JSON `{x_min, y_min, width, height}` plus `distance_field_values()` `Float32Array` (inf = blocked).

Episode methods throw without an active episode. `decide`/`follow_path` throw once it is done.
`reset`, `set_vehicle`, `load_map` and `place_random` end the episode. `step` still works during an episode but
bypasses its bookkeeping.

## Policy server

`fsim-rl serve --checkpoint <path> [--port 8765]` runs a WebSocket server (JSON messages).

- On connect, server sends `{"type": "hello", "mode", "decision_ticks", "spec_hash", "model": {...}, "mode_config": {...}, "arrival": {...}}`.
  The viewer refuses to drive if `spec_hash` differs from its wasm `spec()`. `arrival` is the
  checkpoint's `env.arrival`; the viewer passes it to `retarget` with the mode, so a `stop` model's
  episodes succeed only when it stops in the center.
- Bad messages get `{"type": "error", "message"}`.
- `hello.observation` is `"full"` or `"student"`. For a student checkpoint, `spec_hash` is the
  student layout's hash (`spec()["student"]["spec_hash"]`) and the client sends
  `{"type": "act", "student": [f32...]}` (wasm `policy_student()`) instead of the grids.
- Client: `{"type": "act", "local": <base64 u8>, "coarse": <base64 u8>, "vector": [f32...]}`.
- Server: `{"type": "action", "action": int | [int] | [[x, y]], "probs": [9 floats] | null, "value": float}`.
  `action` shape follows the mode table above (waypoints in the vehicle frame).
  `probs` is the first plan step's distribution for `direct_plan` and `null` for `path_planner`.
- The viewer executes like the env: `decide` for each executed decision, `follow_path` until
  `replan`, `predict_path` to draw `direct_plan` plans. For `path_planner`, convert the server's
  waypoints once with `plan_waypoints`, then pass that same array to `follow_path` until `replan`.
