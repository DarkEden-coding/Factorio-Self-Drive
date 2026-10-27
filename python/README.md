# fsim-rl

PPO training, evaluation and the viewer's policy server for the Factorio driving simulator.
The environment contract is [docs/rl.md](../docs/rl.md).

## Setup

```sh
cd python
uv sync
```

This installs Python 3.12, PyTorch, and builds the `fsim` extension from `crates/fsim-py` with maturin.
After changing Rust code, rebuild it with `uv sync --reinstall-package fsim`.

## Commands

```sh
uv run fsim-rl train --config configs/reactive.toml [--run-name NAME] [--iterations N]
uv run fsim-rl train --config configs/reactive.toml --run-name NAME --resume        # resume latest.pt
uv run fsim-rl train --config configs/reactive.toml --resume runs/NAME/checkpoints/iter_000100.pt
uv run fsim-rl train --config configs/finetune_moderate.toml --init-from runs/reactive-1/best/model.pt \
    [--set ppo.num_envs=256 --set device=cuda]                                       # fine-tune, see below
uv run fsim-rl eval --checkpoint runs/NAME/checkpoints/latest.pt --split holdout --episodes 200 [--replays out.json] [--scenario nest]
uv run fsim-rl baseline --config configs/reactive.toml --split holdout --episodes 200   # A* + pure pursuit
uv run fsim-rl suite --checkpoint runs/NAME/checkpoints/latest.pt [--baseline] --split holdout --episodes 200
uv run fsim-rl suite --baseline --split holdout --episodes 200                       # A* only
uv run fsim-rl serve --checkpoint runs/NAME/checkpoints/latest.pt [--port 8765] [--sample]
uv run fsim-rl smoke [reactive direct_plan path_planner]   # 2 tiny PPO iterations + 2 eval episodes each
uv run pytest
```

`eval` and `baseline` print success rate, SPL, mean time to target, damage, destroyed/timeout/stalled
rates, collision ticks per episode by kind, the share of episodes touching a tree, the share of time in
nest zones, the share of episodes that reversed, and per-vehicle and per-scenario breakdowns.
`--min-distance/--max-distance/--max-tortuosity` override the standard target range (default:
`env.target` from the config). `--scenario KIND` runs only that scenario kind (docs/rl.md, "Scenarios").
`--sample` samples actions instead of taking the mode.

`suite` runs every scenario kind (`--episodes` each) with the checkpoint and, with `--baseline`, the A*
baseline on the same episodes, prints one markdown table and writes the full reports as JSON (`--out`,
default `<checkpoint>-suite-<split>.json` next to the checkpoint, or `runs/suite-baseline-<split>.json`).

`train --set KEY=VALUE` overrides any config value (dotted path, TOML value), e.g. per machine:
`--set ppo.num_envs=256 --set device=cuda`. `eval`, `baseline` and `suite` accept `--set` as well;
`--set env.distance_field.nest_zone_cost=0` evaluates on the old zone-blind field.

## Configs

`configs/base.toml` holds every setting with comments. `reactive.toml`, `direct_plan.toml` and
`path_planner.toml` set `base = "base.toml"` and override a few keys (tables merge, lists replace).

- `[env]`: sent to `fsim.VecEnv` as config JSON. `mode` comes from `model.kind`. `map_pool` is relative to
  the repo root.
- `[model]`: `kind` (`reactive`, `direct_plan`, `path_planner`), encoder sizes, and kind-specific settings
  (plan consistency loss weight, waypoint segment bounds).
- `[ppo]`: rollout size (`num_envs` x `rollout_steps`), optimizer and loss settings.
- `[curriculum]`: target-distance stages for `standard` episodes, advanced when their rolling success
  rate passes `success_threshold`. Other scenario kinds have their own ranges and don't count.
- `env.scenarios`: weighted scenario kinds and their parameters. Weights count episodes.
- `device = "auto"` picks CUDA, then MPS, then CPU. bf16 autocast runs on CUDA and MPS.

## Fine-tuning

`--init-from CHECKPOINT` starts a new run from a checkpoint's weights and vector normalizer with a fresh
optimizer, curriculum and step count. It refuses a checkpoint whose model kind or `spec_hash` differs,
or whose weights don't fit `[model]`. The run dir records the source in `init_from.json`.

`configs/finetune_moderate.toml` and `configs/finetune_strong.toml` fine-tune a reactive policy on trees,
backing up, nest zones and long routes: a scenario mix weighted toward those kinds, the standard
curriculum resuming at 16-128 tiles, learning rate 1e-4 and 20M steps. Moderate uses the default
penalties (collision -0.2, nest zone -0.15, damage -5, `nest_zone_cost` 3); strong raises them (-0.5,
-0.3, -10, `nest_zone_cost` 4). Training logs `scenario/<kind>/success_rate`, `/count` and `/length`,
plus `episode/spl`, `episode/collisions_<kind>`, `episode/nest_zone_fraction` and
`episode/reversed_rate`.

## Runs

Each run writes to `runs/<run-name>/` (gitignored): `config.json`, `metrics.csv` (one row per iteration),
`tb/` (TensorBoard) and `checkpoints/` (`latest.pt`, plus `iter_NNNNNN.pt` every `checkpoint_interval`
iterations). Checkpoints hold the model, optimizer, config, env spec and `spec_hash`, curriculum state
and step counters. Resume and eval refuse a checkpoint whose `spec_hash` differs from the built `fsim`.

Watch training:

```sh
uv run tensorboard --logdir runs
```

## Viewer

Start `uv run fsim-rl serve --checkpoint ...` and connect the viewer to `ws://127.0.0.1:8765`. The server
sends a `hello` with the mode, `decision_ticks` and `spec_hash`; the viewer refuses to drive if the hash
differs from its wasm build. Replay files from `eval --replays` load in the viewer as well.
