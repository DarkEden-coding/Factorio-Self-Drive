#!/usr/bin/env python3
"""Collect base-only Factorio data in isolated disposable saves, then import/replay it."""
import argparse
import difflib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from map_format import encode_map

ROOT = Path(__file__).resolve().parents[1]
MAC_BINARY = Path.home() / "Library/Application Support/Steam/steamapps/common/Factorio/factorio.app/Contents/MacOS/factorio"
LINUX_BINARY = Path.home() / ".steam/steam/steamapps/common/Factorio/bin/x64/factorio"
DEFAULT_BINARY = MAC_BINARY if MAC_BINARY.exists() else LINUX_BINARY
MODS = {"base": True, "fsim-export": True, "fsim-autorun": True,
        "space-age": False, "quality": False, "elevated-rails": False, "recycler": False}
# Space Age planets the exporter knows (Aquilo is left out on purpose).
PLANETS = ("vulcanus", "gleba", "fulgora")


def mod_set(planets):
    """Base-only for Nauvis; Space Age and its dependencies for planet collection."""
    mods = dict(MODS)
    if planets:
        mods.update({"space-age": True, "quality": True, "elevated-rails": True, "recycler": True})
    return mods


def pool_dir(planet):
    """Nauvis keeps the original pool; each planet gets its own directory."""
    return ROOT / "data/game" / ("maps" if planet == "nauvis" else f"maps-{planet}")


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def run_engine(command, log, timeout, output=None):
    """Wait for a normal CLI exit or a job marker; always reap our own process."""
    started = time.monotonic()
    with log.open("w") as stream:
        process = subprocess.Popen([str(p) for p in command], stdout=stream, stderr=subprocess.STDOUT)
        try:
            while True:
                if output is not None:
                    error = output / "error.json"
                    done = output / "done.json"
                    if error.exists():
                        raise RuntimeError(f"Factorio job failed: {error.read_text()}")
                    if done.exists():
                        return json.loads(done.read_text())
                code = process.poll()
                if code is not None:
                    if code == 0 and output is None:
                        return None
                    raise RuntimeError(f"Factorio exited {code} before completion")
                if time.monotonic() - started > timeout:
                    raise RuntimeError(f"Factorio timed out after {timeout:g}s")
                time.sleep(0.1)
        except (RuntimeError, ValueError) as error:
            tail = "\n".join(log.read_text(errors="replace").splitlines()[-35:])
            raise RuntimeError(f"{error}\nLog: {log}\n{tail}") from error
        finally:
            stop(process)


def run(command, **kwargs):
    return subprocess.run([str(p) for p in command], cwd=ROOT, check=True, **kwargs)


def copy_output(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(destination.suffix + ".tmp")
    shutil.copyfile(source, temporary)
    temporary.replace(destination)


def assign_splits(entries, holdout):
    """The last `holdout` seeds are held out for evaluation; at least one map stays in train."""
    count = min(holdout, len(entries) - 1)
    for index, entry in enumerate(entries):
        entry["split"] = "holdout" if index >= len(entries) - count else "train"
    return entries


def data_dir(binary):
    """The install's `data` directory: Contents/data on macOS, <install>/data on Linux."""
    for parent in binary.parents:
        if (parent / "data/core").is_dir():
            return parent / "data"
    raise ValueError(f"no Factorio data directory above {binary}")


def collect(args, temporary):
    mods, write = temporary / "mods", temporary / "write"
    mods.mkdir()
    write.mkdir()
    for name in ("fsim-export", "fsim-autorun"):
        shutil.copytree(ROOT / "mod" / name, mods / name)
    active_mods = mod_set(args.planets)
    (mods / "mod-list.json").write_text(json.dumps({"mods": [
        {"name": name, "enabled": enabled} for name, enabled in active_mods.items()]}))
    config = temporary / "config.ini"
    config.write_text(f"[path]\nread-data={data_dir(args.factorio)}\nwrite-data={write}\n"
                      "[general]\ncheck-updates=false\nautosave-interval=0\n")
    # No map-gen overrides: --create uses the default preset and the fixed seed. Planet
    # surfaces are created from that save, so their seeds follow from it.
    planets = args.planets or ["nauvis"]
    base = [args.factorio, "-c", config, "--mod-directory", mods, "--disable-audio"]
    output = write / "script-output/fsim"
    destination = ROOT / "data/game"
    runs = []
    entries = []
    for index, seed in enumerate(args.seeds):
        if index > 0 and args.skip_maps:
            break
        if output.exists():
            shutil.rmtree(output)
        traces = index == 0 and not args.skip_traces
        filters = "{" + ",".join(json.dumps(f) for f in args.scenarios) + "}"
        planet_list = "{" + ",".join(json.dumps(p) for p in planets) + "}"
        (mods / "fsim-autorun/config.lua").write_text(
            f"return {{traces = {str(traces).lower()}, maps = {str(not args.skip_maps).lower()}, "
            f"radius = {args.map_radius}, filters = {filters}, planets = {planet_list}}}\n")
        save = temporary / f"seed-{seed}.zip"
        print(f"Seed {seed}: creating default save; planets={planets}, traces={traces}, "
              f"maps={not args.skip_maps}", flush=True)
        started = time.monotonic()
        run_engine(base + ["--create", save, "--map-gen-seed", seed],
                   temporary / f"create-{seed}.log", args.timeout)
        # Autorun omits for_player: benchmark has no server for for_player=0 writes.
        marker = run_engine(base + ["--benchmark", save, "--benchmark-ticks", "1000000"],
                            temporary / f"collect-{seed}.log", args.timeout, output)
        if marker["players"] != 0:
            raise RuntimeError("Expected a playerless collection save")
        active = marker["active_mods"]
        if set(active) != {name for name, enabled in active_mods.items() if enabled}:
            raise RuntimeError(f"Unexpected active mods: {active}")
        copy_output(output / "prototypes.json", destination / "prototypes.json")
        if index == 0 and not args.planets:
            copy_output(output / "graphics.json", destination / "graphics.json")
        for path in sorted((output / "traces").glob("*.json")):
            json.loads(path.read_text())
            copy_output(path, destination / "traces" / path.name)
        for path in sorted((output / "maps").glob("*.json")):
            planet = path.stem.split("-seed-")[0]
            encoded, entry = encode_map(json.loads(path.read_text()), path.stem)
            entry["planet"] = planet
            pool = pool_dir(planet)
            pool.mkdir(exist_ok=True)
            (pool / entry["file"]).write_bytes(encoded)
            entries.append(entry)
        elapsed = time.monotonic() - started
        print(f"Seed {seed}: completed at tick {marker['finished_tick']} in {elapsed:.2f}s", flush=True)
        runs.append({"seed": seed, "wall_seconds": round(elapsed, 3), "marker": marker})
    for planet in planets:
        planet_entries = [entry for entry in entries if entry["planet"] == planet]
        if not planet_entries:
            continue
        assign_splits(planet_entries, args.holdout_count)
        pool = pool_dir(planet)
        (pool / "index.json").write_text(json.dumps(planet_entries, indent=2) + "\n")
        wanted = {entry["file"] for entry in planet_entries} | {"index.json"}
        for path in pool.iterdir():
            if path.suffix in (".json", ".fsmap") and path.name not in wanted:
                path.unlink()
    return runs


def process_outputs(factorio, args):
    if not args.planets:
        run([sys.executable, "scripts/build-game-assets.py", "--factorio", data_dir(factorio).parent])
    current = ROOT / "data/prototypes.json"
    before = current.read_text()
    run([sys.executable, "scripts/import-prototypes.py", "data/game/prototypes.json", current])
    after = current.read_text()
    changes = list(difflib.unified_diff(before.splitlines(), after.splitlines(),
                                      fromfile="previous prototypes", tofile="live prototypes"))
    added = sum(line.startswith("+") and not line.startswith("+++") for line in changes)
    removed = sum(line.startswith("-") and not line.startswith("---") for line in changes)
    print(f"Prototype diff: +{added}/-{removed} lines", flush=True)
    (ROOT / "data/game/prototypes.diff").write_text("\n".join(changes) + ("\n" if changes else ""))
    if not args.skip_wasm:
        run(["scripts/build-wasm.sh"])
    paths = sorted((ROOT / "data/game/traces").glob("*.json"))
    reports = []
    if paths:
        result = run(["cargo", "run", "--release", "-p", "fsim-core", "--bin", "replay-trace", "--",
                      "--prototypes", current, *paths], capture_output=True, text=True)
        reports = [json.loads(line) for line in result.stdout.splitlines()]
    # Store repo-relative paths so reports remain usable after moving the checkout.
    for entry in reports:
        entry["path"] = str(Path(entry["path"]).relative_to(ROOT))
    (ROOT / "data/game/replay-report.json").write_text(json.dumps({
        "schema_version": 1, "first_divergence_threshold_tiles": 0.05, "traces": reports}, indent=2) + "\n")
    print("Scenario                                      position max/rms    speed max/rms       first >.05")
    for entry in reports:
        r = entry["report"]
        first = r["first_position_error_tick"]
        print(f"{r['name']:45} {r['position']['max']:9.4f}/{r['position']['rms']:<9.4f} "
              f"{r['speed']['max']:8.5f}/{r['speed']['rms']:<8.5f} {first if first is not None else '-'}")
    return len(reports)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--factorio", type=Path, default=Path(os.environ.get("FACTORIO_BINARY", DEFAULT_BINARY)),
                        help="Factorio binary, also settable through FACTORIO_BINARY")
    parser.add_argument("--seeds", default=None, help="comma-separated fixed uint32 seeds; overrides --map-count")
    parser.add_argument("--map-count", type=int, default=16, help="number of default Nauvis seeds, default 16")
    parser.add_argument("--holdout-count", type=int, default=8,
                        help="last N seeds get split=holdout in the pool index, default 8")
    parser.add_argument("--skip-traces", action="store_true")
    parser.add_argument("--skip-maps", action="store_true")
    parser.add_argument("--map-radius", type=int, default=16, help="origin-centered chunk radius, 0..31")
    parser.add_argument("--scenarios", default="", help="comma-separated literal name substrings, OR matched")
    parser.add_argument("--planets", default="",
                        help=f"comma-separated Space Age planets to export instead of Nauvis ({', '.join(PLANETS)}); "
                        "enables Space Age and writes data/game/maps-<planet>/")
    parser.add_argument("--skip-wasm", action="store_true", help="don't rebuild the wasm package")
    parser.add_argument("--keep-temp", action="store_true")
    parser.add_argument("--timeout", type=float, default=1800, help="seconds per Factorio invocation")
    args = parser.parse_args()
    try:
        if not 1 <= args.map_count <= 1024:
            raise ValueError("map count must be in 1..1024")
        if args.holdout_count < 0:
            raise ValueError("holdout count must be nonnegative")
        args.seeds = list(dict.fromkeys(int(s) for s in args.seeds.split(","))) if args.seeds is not None else [
            (12345 + i * 2654435761) % 2**32 for i in range(args.map_count)]
        if not args.seeds or any(s < 0 or s > 2**32 - 1 for s in args.seeds):
            raise ValueError("seeds must be uint32 values")
        args.scenarios = [s.strip() for s in args.scenarios.split(",") if s.strip()]
        args.planets = [p.strip() for p in args.planets.split(",") if p.strip()]
        if any(p not in PLANETS for p in args.planets):
            raise ValueError(f"planets must be among {', '.join(PLANETS)}")
        if any(not all(c.isalnum() or c in "._-" for c in f) for f in args.scenarios):
            raise ValueError("scenario filters may contain letters, digits, dots, underscores and hyphens")
        if not 0 <= args.map_radius <= 31 or args.timeout <= 0:
            raise ValueError("map radius must be 0..31 and timeout must be positive")
        args.factorio = args.factorio.expanduser().resolve(strict=True)
    except (ValueError, OSError) as error:
        parser.error(str(error))
    started = time.monotonic()
    temporary = Path(tempfile.mkdtemp(prefix="fsim-collect-"))
    print(f"Isolated Factorio directory: {temporary}", flush=True)
    try:
        runs = collect(args, temporary)
        count = process_outputs(args.factorio, args)
        elapsed = round(time.monotonic() - started, 3)
        files = [{"path": str(p.relative_to(ROOT)), "bytes": p.stat().st_size}
                 for p in sorted((ROOT / "data/game").rglob("*"))
                 if p.is_file() and p.name not in ("collection.json", ".gitignore")]
        manifest = {"schema_version": 1, "wall_seconds": elapsed, "mods": mod_set(args.planets),
                    "map_generation": "default preset, no map-gen overrides",
                    "planets": args.planets or ["nauvis"], "runs": runs,
                    "replayed_traces": count, "files": files}
        (ROOT / "data/game/collection.json").write_text(json.dumps(manifest, indent=2) + "\n")
        if not args.skip_maps:
            name = "map-collection.json" if not args.planets else f"map-collection-{'-'.join(args.planets)}.json"
            (ROOT / "data/game" / name).write_text(json.dumps(manifest, indent=2) + "\n")
        print(f"Collected and processed {count} traces in {elapsed:.2f}s. Files:")
        for entry in files:
            print(f"  {entry['bytes']:>10,} bytes  {entry['path']}")
    except (RuntimeError, OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"collect-game-data: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr, file=sys.stderr)
        return 1
    finally:
        if args.keep_temp:
            print(f"Kept temporary directory: {temporary}", flush=True)
        else:
            shutil.rmtree(temporary)
    return 0


if __name__ == "__main__":
    sys.exit(main())
