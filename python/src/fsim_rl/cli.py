"""`fsim-rl` command line: train, distill, eval, baseline, suite, export-student, serve, smoke."""

from __future__ import annotations

import argparse
import asyncio
import json
import time
import tomllib
from pathlib import Path

from .config import (
    CONFIG_DIR, MODEL_KINDS, PYTHON_DIR, SCENARIO_KINDS, TargetConfig, load_config, with_overrides,
)

SMOKE_OVERRIDES = {
    "device": "auto",
    "checkpoint_interval": 1,
    "env": {"target": {"min_distance": 8, "max_distance": 24, "max_tortuosity": 1.3}},
    "ppo": {"num_envs": 4, "rollout_steps": 16, "epochs": 1, "num_minibatches": 2},
    "curriculum": {"enabled": False},
}
SMOKE_STUDENT_OVERRIDES = {
    "env": {"scenarios": [{"kind": "standard", "weight": 1.0}]},
    "distill": {"num_envs": 4, "rollout_steps": 16, "iterations": 3, "beta_iterations": 1,
                "buffer_size": 1024, "minibatch": 32, "updates_per_iteration": 2},
}
SMOKE_KINDS = (*MODEL_KINDS, "student")


def run_dir_for(config_run_dir: str, name: str) -> Path:
    base = Path(config_run_dir)
    return (base if base.is_absolute() else PYTHON_DIR / base) / name


def parse_overrides(items: list[str] | None) -> dict:
    """`a.b=value` items as a nested dict; values parse as TOML (strings may be bare)."""
    overrides: dict = {}
    for item in items or []:
        key, sep, text = item.partition("=")
        if not sep or not key:
            raise SystemExit(f"--set needs key=value, got {item!r}")
        try:
            value = tomllib.loads(f"v = {text}")["v"]
        except tomllib.TOMLDecodeError:
            value = text
        node = overrides
        *parents, leaf = key.split(".")
        for part in parents:
            node = node.setdefault(part, {})
        node[leaf] = value
    return overrides


def cmd_train(args: argparse.Namespace) -> None:
    from .trainer import Trainer

    config = load_config(args.config, parse_overrides(args.set))
    if args.resume and args.init_from:
        raise SystemExit("use --resume or --init-from, not both")
    resume = None
    if args.resume == "latest":
        if not args.run_name:
            raise SystemExit("--resume without a path needs --run-name")
        run_dir = run_dir_for(config.run_dir, args.run_name)
        resume = run_dir / "checkpoints" / "latest.pt"
    elif args.resume:
        resume = Path(args.resume)
        run_dir = resume.resolve().parent.parent
    else:
        name = args.run_name or f"{Path(args.config).stem}-{time.strftime('%Y%m%d-%H%M%S')}"
        run_dir = run_dir_for(config.run_dir, name)
    if resume is not None and not resume.exists():
        raise SystemExit(f"no checkpoint at {resume}")
    init_from = Path(args.init_from) if args.init_from else None
    if init_from is not None and not init_from.exists():
        raise SystemExit(f"no checkpoint at {init_from}")
    print(f"run dir: {run_dir}")
    Trainer(config, run_dir, resume=resume, init_from=init_from).train(args.iterations)


def _target(args: argparse.Namespace) -> TargetConfig | None:
    if args.min_distance is None and args.max_distance is None and args.max_tortuosity is None:
        return None
    defaults = TargetConfig()
    return TargetConfig(
        min_distance=args.min_distance if args.min_distance is not None else defaults.min_distance,
        max_distance=args.max_distance if args.max_distance is not None else defaults.max_distance,
        max_tortuosity=args.max_tortuosity if args.max_tortuosity is not None else defaults.max_tortuosity,
    )


def _print_report(report: dict, out: str | None) -> None:
    text = json.dumps(report, indent=2)
    print(text)
    if out:
        Path(out).write_text(text)


def cmd_eval(args: argparse.Namespace) -> None:
    from .evaluate import evaluate_checkpoint

    report = evaluate_checkpoint(
        Path(args.checkpoint),
        split=args.split,
        episodes=args.episodes,
        num_envs=args.num_envs,
        seed=args.seed,
        device=args.device,
        deterministic=not args.sample,
        target=_target(args),
        replays=Path(args.replays) if args.replays else None,
        scenario=args.scenario,
        overrides=parse_overrides(args.set),
    )
    _print_report(report, args.out)


def cmd_baseline(args: argparse.Namespace) -> None:
    from .checkpoint import resolve_device
    from .evaluate import evaluate

    config = load_config(args.config, parse_overrides(args.set))
    report = evaluate(
        config,
        split=args.split,
        episodes=args.episodes,
        num_envs=args.num_envs,
        seed=args.seed,
        device=resolve_device("cpu"),
        target=_target(args),
        replays=Path(args.replays) if args.replays else None,
        scenario=args.scenario,
    )
    _print_report(report, args.out)


def cmd_suite(args: argparse.Namespace) -> None:
    from .checkpoint import load_policy, resolve_device
    from .evaluate import suite, suite_table

    device = resolve_device(args.device)
    model, spec_hash = None, None
    overrides = parse_overrides(args.set)
    if args.checkpoint:
        model, config, state = load_policy(args.checkpoint, device)
        config = with_overrides(config, overrides)
        spec_hash = state["spec_hash"]
    else:
        config = load_config(args.config, overrides)
    report = suite(
        config, args.split, args.episodes, args.num_envs, args.seed, device,
        model=model, baseline=args.baseline, spec_hash=spec_hash, kinds=args.scenarios,
    )
    report["checkpoint"] = args.checkpoint
    report["overrides"] = overrides
    table = suite_table(report)
    report["table"] = table
    print(table)
    if args.out:
        out = Path(args.out)
    elif args.checkpoint:
        checkpoint = Path(args.checkpoint)
        out = checkpoint.with_name(f"{checkpoint.stem}-suite-{args.split}.json")
    else:
        out = run_dir_for(config.run_dir, f"suite-baseline-{args.split}.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2))
    print(f"wrote {out}")


def cmd_distill(args: argparse.Namespace) -> None:
    from .distill import Distiller

    config = load_config(args.student_config, parse_overrides(args.set))
    name = args.run_name or f"{Path(args.student_config).stem}-distill-{time.strftime('%Y%m%d-%H%M%S')}"
    run_dir = run_dir_for(config.run_dir, name)
    print(f"run dir: {run_dir}")
    best = Distiller(config, Path(args.teacher), run_dir).run(args.iterations)
    print(f"best: {best}")


def cmd_export_student(args: argparse.Namespace) -> None:
    from .checkpoint import load_policy, resolve_device
    from .student_export import export_student, lua_estimate, worst_case_ray_tiles

    model, config, state = load_policy(args.checkpoint, resolve_device("cpu"))
    if config.model.arch != "student":
        raise SystemExit("export-student needs a student checkpoint")
    exported = export_student(model, config, state)
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(exported, separators=(",", ":")))
    ray_tiles = args.ray_tiles if args.ray_tiles is not None else worst_case_ray_tiles(exported["obs"])
    summary = {
        "out": str(out),
        "kb": round(out.stat().st_size / 1024, 1),
        **exported["cost"],
        "lua": lua_estimate(exported["cost"], ray_tiles),
    }
    print(json.dumps(summary, indent=2))


def cmd_serve(args: argparse.Namespace) -> None:
    from .checkpoint import load_policy, resolve_device
    from .server import PolicyServer, run_server

    device = resolve_device(args.device)
    model, config, state = load_policy(args.checkpoint, device)
    server = PolicyServer(model, config, state["spec_hash"], deterministic=not args.sample, device=device)
    try:
        asyncio.run(run_server(server, args.host, args.port))
    except KeyboardInterrupt:
        pass


def _smoke_student(teacher: Path) -> Path:
    """Distill a tiny student from `teacher`, fine-tune it for 2 PPO iterations, export it."""
    from .distill import Distiller
    from .trainer import Trainer

    if not teacher.exists():
        raise SystemExit(f"student smoke needs the reactive smoke checkpoint {teacher}")
    config = load_config(CONFIG_DIR / "student.toml", SMOKE_OVERRIDES)
    config = with_overrides(config, SMOKE_STUDENT_OVERRIDES)
    run_dir = run_dir_for(config.run_dir, "smoke/student")
    print(f"== smoke student: distill from {teacher}, 2 PPO iterations -> {run_dir}")
    distilled = Distiller(config, teacher, run_dir / "distill").run()
    checkpoint = Trainer(config, run_dir / "rl", init_from=distilled).train(iterations=2)
    cmd_export_student(argparse.Namespace(
        checkpoint=str(checkpoint), out=str(run_dir / "student.json"), ray_tiles=None,
    ))
    return checkpoint


def cmd_smoke(args: argparse.Namespace) -> None:
    from .evaluate import evaluate_checkpoint
    from .trainer import Trainer

    kinds = args.kinds or list(SMOKE_KINDS)
    unknown = set(kinds) - set(SMOKE_KINDS)
    if unknown:
        raise SystemExit(f"unknown model kinds {sorted(unknown)}")
    summary = {}
    for kind in kinds:
        started = time.perf_counter()
        if kind == "student":
            reactive = load_config(CONFIG_DIR / "reactive.toml", SMOKE_OVERRIDES)
            teacher = run_dir_for(reactive.run_dir, "smoke/reactive") / "checkpoints" / "latest.pt"
            checkpoint = _smoke_student(teacher)
            run_dir = checkpoint.parent.parent
        else:
            config = load_config(CONFIG_DIR / f"{kind}.toml", SMOKE_OVERRIDES)
            run_dir = run_dir_for(config.run_dir, f"smoke/{kind}")
            print(f"== smoke {kind}: 2 PPO iterations, 4 envs -> {run_dir}")
            checkpoint = Trainer(config, run_dir).train(iterations=2)
        replays = run_dir / "replays.json"
        report = evaluate_checkpoint(
            checkpoint, split=args.split, episodes=2, num_envs=2, device=SMOKE_OVERRIDES["device"],
            replays=replays,
        )
        summary[kind] = {
            "seconds": round(time.perf_counter() - started, 1),
            "success_rate": report["success_rate"],
            "mean_return": report["mean_return"],
            "replays": str(replays),
        }
        print(json.dumps(report, indent=2))
    print("== smoke summary")
    print(json.dumps(summary, indent=2))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="fsim-rl", description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    train = sub.add_parser("train", help="train with PPO")
    train.add_argument("--config", required=True)
    train.add_argument("--run-name")
    train.add_argument(
        "--resume", nargs="?", const="latest",
        help="checkpoint path, or no value to resume runs/<run-name>/checkpoints/latest.pt",
    )
    train.add_argument("--iterations", type=int, help="stop after this many iterations")
    train.add_argument(
        "--init-from", metavar="CHECKPOINT",
        help="start a new run from this checkpoint's weights and normalizer (fresh optimizer)",
    )
    train.add_argument(
        "--set", action="append", metavar="KEY=VALUE",
        help="override a config value, e.g. --set ppo.num_envs=256 --set device=cuda",
    )
    train.set_defaults(func=cmd_train)

    distill = sub.add_parser("distill", help="DAgger-distill a student from a teacher checkpoint")
    distill.add_argument("--teacher", required=True, help="teacher checkpoint (full observation)")
    distill.add_argument("--student-config", required=True, help="student TOML (model.arch = student)")
    distill.add_argument("--run-name")
    distill.add_argument("--iterations", type=int, help="override distill.iterations")
    distill.add_argument("--set", action="append", metavar="KEY=VALUE", help="override a config value")
    distill.set_defaults(func=cmd_distill)

    def eval_args(p: argparse.ArgumentParser) -> None:
        p.add_argument("--split", default="holdout")
        p.add_argument("--episodes", type=int, default=100)
        p.add_argument("--num-envs", type=int, default=16)
        p.add_argument("--seed", type=int, default=12345)
        p.add_argument("--replays", help="write episode replays (JSON) here")
        p.add_argument("--out", help="write the metrics JSON here")
        p.add_argument("--min-distance", type=float)
        p.add_argument("--max-distance", type=float)
        p.add_argument("--max-tortuosity", type=float)
        p.add_argument("--scenario", choices=SCENARIO_KINDS, help="evaluate only this scenario kind")
        p.add_argument("--set", action="append", metavar="KEY=VALUE", help="override a config value")

    ev = sub.add_parser("eval", help="evaluate a checkpoint")
    ev.add_argument("--checkpoint", required=True)
    ev.add_argument("--device", default="auto")
    ev.add_argument("--sample", action="store_true", help="sample actions instead of the mode")
    eval_args(ev)
    ev.set_defaults(func=cmd_eval)

    base = sub.add_parser("baseline", help="evaluate the A* + pure pursuit baseline")
    base.add_argument("--config", default=str(CONFIG_DIR / "reactive.toml"), help="env settings and mode")
    eval_args(base)
    base.set_defaults(func=cmd_baseline)

    su = sub.add_parser("suite", help="evaluate every scenario kind; prints one table, writes JSON")
    su.add_argument("--checkpoint", help="model to evaluate")
    su.add_argument("--baseline", action="store_true", help="also (or only) run the A* baseline")
    su.add_argument("--config", default=str(CONFIG_DIR / "reactive.toml"),
                    help="env settings without a checkpoint")
    su.add_argument("--split", default="holdout")
    su.add_argument("--episodes", type=int, default=100, help="episodes per scenario kind")
    su.add_argument("--num-envs", type=int, default=16)
    su.add_argument("--seed", type=int, default=12345)
    su.add_argument("--device", default="auto")
    su.add_argument("--scenarios", nargs="+", choices=SCENARIO_KINDS, help="subset of kinds")
    su.add_argument("--out", help="JSON path (default: next to the checkpoint, or runs/)")
    su.add_argument(
        "--set", action="append", metavar="KEY=VALUE",
        help="override a config value, e.g. --set env.distance_field.nest_zone_cost=0",
    )
    su.set_defaults(func=cmd_suite)

    export = sub.add_parser("export-student", help="export a student as JSON for the Lua mod")
    export.add_argument("--checkpoint", required=True)
    export.add_argument("--out", required=True)
    export.add_argument("--ray-tiles", type=float,
                        help="ray tiles per decision for the Lua estimate (default: worst case)")
    export.set_defaults(func=cmd_export_student)

    serve = sub.add_parser("serve", help="WebSocket policy server for the viewer")
    serve.add_argument("--checkpoint", required=True)
    serve.add_argument("--host", default="127.0.0.1")
    serve.add_argument("--port", type=int, default=8765)
    serve.add_argument("--device", default="cpu")
    serve.add_argument("--sample", action="store_true", help="sample actions instead of the mode")
    serve.set_defaults(func=cmd_serve)

    smoke = sub.add_parser("smoke", help="tiny train + eval per model kind")
    smoke.add_argument("kinds", nargs="*", help=f"subset of {', '.join(SMOKE_KINDS)} (default all)")
    smoke.add_argument("--split", default="train")
    smoke.set_defaults(func=cmd_smoke)
    return parser


def main(argv: list[str] | None = None) -> None:
    args = build_parser().parse_args(argv)
    args.func(args)
