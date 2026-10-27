# Factorio 2.1.23 vehicle calibration

The driving update in `crates/fsim-core/src/physics.rs` is calibrated against live, base-only Factorio 2.1.23 recordings made by `mod/fsim-export`. Prototype values come from the installed game and `data/prototypes.json`. The engine source is unavailable; the rules below are experimentally recovered behavior, not claims about its C++ implementation.

The original 80 recordings contain 30 acceleration runs of 6000 ticks, two coast runs of 9000 ticks, two brake/reverse runs, 20 steering runs, 22 entity-contact runs and four shoreline runs. Another 261 short probes test axis snapping, mixed acceleration/reverse steering, orientation quantization and rotational contact. All 341 traces currently reproduce every sampled position and orientation exactly. Speed differs only by floating-point roundoff except at diagonal shorelines. Vehicle health is exact; surviving trees' later regeneration is not simulated.

## Units and tick order

There are 60 ticks per second. Speed is signed tiles/tick, and `216 * speed` gives km/h. Orientation is clockwise turns, with north at zero. Velocity follows the heading; there is no lateral velocity state.

Each tick:

1. Average terrain friction under the starting hull.
2. Apply engine or braking energy, then drag, then the low-speed cutoff.
3. Compute the proposed steering/snap orientation from this updated speed.
4. Compute translation using the **starting** orientation. Truncate each displacement component toward zero to a multiple of 1/256 tile.
5. Check the destination center against water and the destination hull, at its proposed orientation, against entities.
6. Commit both position and orientation if clear. Any contact cancels both for the whole tick, including a contact that destroys the obstacle. Damage and speed changes still apply.

There are no translation or rotation substeps. The previous substep solver both overestimated movement on destructive contacts and placed stopped vehicles incorrectly.

For example, car/coal/grass tick 1 has speed `0.034392867477622935` but moves only `8/256` tile. Truncating the displacement, rather than accumulating continuous movement and rounding the absolute position, accounts for the long straight-track drift. Using the previous heading accounts for the steering paths. These rules reproduce all samples of all recordings, including positive/negative movement and oblique contacts.

## Engine, drag and reversing

Let `w` be vehicle weight, `v` its old signed speed, `P` consumption in watts, `eta` effectivity, `a` fuel acceleration multiplier, and `t` fuel top-speed multiplier:

```text
E = 0.5 * w * (60 * v)^2
engine_energy_per_tick = P * eta * a * t^2 / 60
reverse_engine_energy_per_tick = engine_energy_per_tick * 0.5
braking_energy_per_tick = braking_power / 60
```

Opposite throttle subtracts braking energy down to zero. Reverse acceleration starts on the next tick, with no leftover braking energy carried into it. Holding forward while reversing follows the same transition. The two complete brake/reverse recordings match in speed to less than `9e-15` tiles/tick and exactly in position.

With mean underlying tile friction `F`:

```text
f = vehicle.friction * (1 + (F - 1) * terrain_friction_modifier)
r = clamp(1 - f, 0, 1)
v_candidate = direction * sqrt(2 * E_after_power / w) / 60 * r
```

The tank's terrain modifier is the exported `0.20000000298023224`, rather than a rounded 0.2. Keeping exported precision matters for exact quantized trajectories.

Contrary to the old wiki-based model, the recorded game applies fuel top-speed bonuses to cars and tanks. Multiplying engine energy by `t^2` reproduces both the first tick and every subsequent acceleration sample for all 30 terrain/fuel/vehicle combinations. This multiplies equilibrium speed by `t`; it is not a hard cap. Car grass terminal speeds are approximately 92.935 km/h on coal/wood, 106.896 on solid fuel, 143.388 on rocket fuel and 168.985 on nuclear fuel. The old 101.8/124.7/146.9 expectations omitted this bonus.

Speeds below `1/128` tile/tick stop before translation. A tank actively steering bypasses this cutoff, so it can rotate and retain tiny translational speed. Releasing steering restores the cutoff. The low-speed tank traces keep speed near 0.003 until tick 240, then stop on tick 241. A car at 0.005 stops immediately. Coast stops occur at ticks 1228 and 1756 for car and tank respectively.

For homogeneous terrain, with engine power `Q` including fuel/reverse multipliers, equilibrium speed is:

```text
v_terminal = sqrt((2 * Q / (60 * w)) * r^2 / (1 - r^2)) / 60
```

Fuel is unlimited. Fuel insertion does not add a startup delay in these recordings. Fuel exhaustion, equipment, quality bonuses and researched braking bonuses remain outside this model.

## Steering and orientation representation

The proposed turn is:

```text
car_turn = steer * rotation_speed * sqrt(abs(v_candidate)) * sign(v_candidate)
tank_turn = steer * rotation_speed
```

The car reverses its steering direction when backing up; the tank does not. Tank steering works at rest. Steering has no additional speed loss away from contact.

With straight steering and nonzero speed, headings within `rotation_snap_angle` of a multiple of 1/8 turn snap to that axis. This includes diagonals. Stationary vehicles retain their heading when steering is straight. Forty short probes test both signs of low speed, rest, headings on either side of snap thresholds, and car/tank thresholds separately.

Simple rounding of turns cannot reproduce the recorded orientation. This equivalent quantizer does:

1. Wrap the requested heading and cast it to `f32` before computing its sine and cosine.
2. Round the smaller absolute component times 32767 to an integer `minor`.
3. Compute `major = round(sqrt(32767^2 - minor^2))`.
4. Compute the first-octant angle `atan2(minor, major) / (2*pi)`, truncating it to an integer multiple of `1/2^32` turn.
5. Reflect that angle into its octant using exact quarter/half turns.

The major component is derived from the rounded minor component. Independently rounding both sine and cosine fails. Quantizing an `atan2` result after reflecting also fails by one orientation bit in alternate octants. Omitting the `f32` conversion fails at several half-integer circle boundaries.

A 201-point grid from 0.016 to 0.01602 turn in steps of `1e-7` isolated the circle quantization. The recovered rule also reproduces all transitions in the original 20 steering recordings and eight mixed throttle/steering probes at other starting headings. Orientation error is exactly zero over complete traces, including after releasing steering and crossing zero speed. `physics::quantize_orientation` implements this representation.

## Contact, movement energy and health

Collision geometry uses oriented prototype boxes and separating-axis tests. Asymmetric boxes rotate their center offset as well as their axes. Entity contact checks the complete proposed translation and rotation together. A blocked tick keeps the previous position and heading; there is no observed bounce, slide or push-back in this corpus. Surviving obstacles set speed to zero. Destroyed obstacles cost speed and the vehicle resumes moving on the following tick.

Collision energy is separate from the engine's SI-unit kinetic energy. Let `dv = v_candidate - v_old`, and let `turn` be the proposed turn before orientation quantization:

```text
K = weight * (v_old^2 + 0.5 * dv^2)
if tank_driving:
    K += 0.5 * weight * turn^2
available_raw_damage = K / energy_per_hit_point
raw_delivered = min(available_raw_damage, raw_damage_needed_to_kill_obstacle)
```

Both entities receive the capped raw damage through their own impact resistances. Prototype vehicle immunity suppresses vehicle damage, without suppressing obstacle damage or slowing/stopping. Cliffs are indestructible and their vehicle immunity is preserved.

This formula explains the apparently tiny extra energy beyond `weight * old_speed^2` without a fitted scale. For example, car/rock at 0.2 hits at tick 32 with 22.95398 observed obstacle damage after float rounding. Tank/rock at 0.1 hits at tick 63 and leaves 197.10217 health. A turning stationary tank delivers 0.245 raw damage per blocked tick: `0.5 * 20000 * 0.0035^2 / 0.5`. Twelve additional contact-turn probes verify that angular energy applies to the tank, that blocked rotation is canceled, and that destruction still cancels that tick's motion.

For a destroyed entity:

```text
v_after = v_candidate * sqrt(max(0, 1 - raw_delivered * energy_per_hit_point / K))
```

Scaling the candidate speed by the energy ratio, rather than subtracting from its kinetic energy, is necessary to match accelerating tanks plowing through three trees. No retained-energy coefficient is needed. The original straight/oblique tree and rock recordings, plus the rotational contacts, now match speed to floating-point roundoff.

Health and applied health damage use `f32` arithmetic. Resistance to positive raw damage `D`, with decrease `R` and percent `p`, is:

```text
if D > R + 1: M = D - R
else if D > 1: M = 1 / (R - D + 2)
else: M = D / (R + 1)
health_damage = M * (1 - p/100)
```

In particular, the last branch scales by `D`; it is not a constant minimum hit. Capping raw tree damage at 50 gives the car 0.35 resisted damage when it destroys a healthy tree. The original implementation's missing energy and small-hit rules caused the large car/rock health error.

## Water and mixed terrain

Water blocks the destination **center**, rather than the hull. A vehicle may overhang water. It keeps normal movement until its center's proposed tile is water; that tick's movement is canceled and speed becomes zero. Holding throttle repeatedly accelerates until another blocked tick. The traces approach the boundary to within 1/256 tile, with exact positions reproduced throughout.

Friction uses multiple tiles under the starting hull, rather than just the center tile. Equal averaging over intersecting tiles matches both straight shoreline recordings exactly, including speed. For a car at `(0, -7.1171875)`, two of six intersecting tiles are water, giving mean friction 1.4 instead of grass's 1.6.

The remaining diagonal shoreline speed discrepancy appears to be the engine's tile-enumeration rule at rotated corners. For example, the game implies mean friction 1.48 at car position `(6.83984375, -6.83984375)`, while exact box/tile overlap gives 1.525. The precise engine enumeration remains unresolved. The current model uses geometric overlap, with no fitted corner adjustment. Across the two diagonal runs, maximum speed errors are `0.0000590679756` for car and `0.0000164509092` for tank. These differences do not change any quantized position or orientation sample.

## Calibration results

These are maxima over each group of the original 80 traces, comparing the checked-in pre-calibration report with a complete replay after calibration. Position is tiles, speed tiles/tick. Every group's final orientation and vehicle-health error is zero.

| Scenario group | Traces | Position before | Position after | Speed before | Speed after |
| --- | ---: | ---: | ---: | ---: | ---: |
| Car accelerate | 15 | 637.242017 | 0 | 0.109122 | <2e-14 |
| Tank accelerate | 15 | 316.800065 | 0 | 0.056962 | <3e-15 |
| Coast | 2 | 6.838846 | 0 | 0.007811 | <9e-16 |
| Brake/reverse | 2 | 3.587076 | 0 | <9e-15 | <9e-15 |
| Car steer | 10 | 11.996363 | 0 | 0.004984 | <9e-16 |
| Tank steer | 10 | 0.442196 | 0 | 0.002912 | <3e-16 |
| Car/tree | 6 | 13.918321 | 0 | 0.178777 | <2e-15 |
| Car/rock | 6 | 0.390055 | 0 | 0.181665 | <3e-16 |
| Car/cliff | 6 | 0.353408 | 0 | 0.033927 | <3e-16 |
| Tank/trees | 2 | 2.260346 | 0 | 0.008413 | <6e-16 |
| Tank/rock | 2 | 4.940996 | 0 | 0.087020 | <2e-16 |
| Water | 4 | 2.196054 | 0 | 0.331554 | 0.000059068 |

Car steering's worst orientation error fell from 0.499947 turn to zero. Car/rock vehicle-health error fell from 41.945207 to zero. Maximum obstacle-health errors fell from 59.921720 to zero for car/rock, from 151.449300 to zero for tank/rock, and from 50 to zero for tank/trees.

The two car/tree runs that leave the tree alive have a remaining maximum obstacle-health error of 0.29999924. Initial impact damage is exact. Factorio heals the tree by approximately 0.1 health every 60 ticks, starting 59 ticks after the recorded hit; the simulator currently has no world-level regeneration clock. Healing was not added to vehicle-local state because it belongs to the shared world and must not be multiplied by the number of vehicles. Multiple simultaneous obstacles, other cliff orientations, extreme script-set speeds and broader glancing contacts have not been exhaustively calibrated.

There are no fitted fudge constants. The `physics` section retains measured `reverse_power_multiplier=0.5` and `stop_speed=0.0078125`, plus unrelated episode settings. Obsolete linear steering, collision energy scale/retention and substep tuning fields were removed. Position, orientation and integer-circle resolutions are representation constants in code.

## Regression fixtures and reproduction

`crates/fsim-core/tests/calibration.rs` replays 38 recordings from `tests/fixtures/traces`, totaling 2,843,001 bytes. The fixture set covers all fuel multiplier behaviors, coast cutoffs for both vehicles, brake/reverse, forward/reverse/low-speed steering, diagonal snapping, direct/oblique impacts, repeated trees, water, and tank rotation into rocks. Acceleration recordings retain their first 800 ticks. Coast recordings end shortly after stopping, omitting only the thousands of identical stationary samples. Headers' input lengths are trimmed to match. Other selected recordings are complete; full-length verification uses `data/game/traces`.

Tests require exactly zero position, orientation and vehicle-health error. The only relaxed bounds are the two documented diagonal-water speed errors and the surviving tree's later regeneration. Other speed errors must stay below `1e-12`; other obstacle health must be exact.

```sh
export DYLD_LIBRARY_PATH="$(rustc --print sysroot)/lib"
scripts/collect-game-data.sh --skip-maps --scenarios calibration
cargo test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run --release -p fsim-core --bin replay-trace -- data/game/traces/*.json
```

The small 201-point angle grid was used only to identify the quantizer and remains in `data/game/traces/car-calibration-angle-grid-*.json`; its construction was a one-tick 0.05-speed car coast at `0.016 + i*1e-7`, for `i=0..200`, on grass. The retained scenario catalog includes the other short probes.

The previous `behavior.rs` fuel expectations need 106.896, 143.388 and 168.985 km/h for solid/rocket/nuclear fuel on grass. Its former whole-hull water expectation must instead allow hull overhang and require only the center to remain on land. Those tests are owned by the map implementation agent; they were not edited by this calibration work. The current concurrently reconciled `behavior.rs` suite passes.
