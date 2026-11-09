#!/usr/bin/env python3
"""Drive the trips of a `test-autodrive.py --suite planets` report in the simulator.

Each trip starts from the engine's initial pose, speed, vehicle and fuel on the pool map of
the same planet and seed (data/game/maps-<planet>/<planet>-seed-<seed>.fsmap), with the
model's own field settings, and runs the exported student until the episode ends. The output
pairs the engine status with the simulator status, to separate model weaknesses (both fail)
from simulator/game differences (only the engine fails).
"""
import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path, help="driving-report.json from the planets suite")
    parser.add_argument("--model", type=Path, required=True, help="exported student JSON")
    parser.add_argument("--seed", type=int, default=12345)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    results = json.loads(args.report.read_text())["results"]
    cases = []
    for r in results:
        planet = r["planet"]
        path = ROOT / f"data/game/maps-{planet}/{planet}-seed-{args.seed}.fsmap"
        entry = next(e for e in json.loads((path.parent / "index.json").read_text())
                     if e["file"] == path.name)
        ox, oy = entry["source_offset"]
        i = r["initial"]
        cases.append(dict(map=str(path), drive=True, vehicle=i["vehicle"], fuel=i["fuel"], request=dict(
            start=dict(x=i["start"]["x"] - ox, y=i["start"]["y"] - oy, orientation=i["orientation"]),
            target=dict(x=i["target"]["x"] - ox, y=i["target"]["y"] - oy), start_speed=i["speed"],
            vehicle=i["vehicle"], fuel=i["fuel"], radius=3,
            arrival=dict(mode="stop", stop_radius=1, stop_speed=0.01))))
    out = subprocess.run(["node", "scripts/autodrive-sim.mjs", str(args.model)], cwd=ROOT,
                         input=json.dumps(cases), capture_output=True, text=True, check=True)
    rows = []
    for r, s in zip(results, json.loads(out.stdout)):
        final = s.get("final") or dict(status="start-failed: " + s["error"][:60], ticks=0,
                                       collisions=0, damage=0)
        rows.append(dict(name=r["name"], engine=r["status"], simulator=final["status"],
                         engine_seconds=r.get("seconds"), simulator_seconds=final["ticks"] / 60,
                         simulator_collisions=final["collisions"], simulator_damage=final["damage"]))
    agree = sum((r["engine"] == "arrived") == (r["simulator"] == "success") for r in rows)
    summary = dict(trips=len(rows), engine_arrived=sum(r["engine"] == "arrived" for r in rows),
                   simulator_success=sum(r["simulator"] == "success" for r in rows),
                   outcome_agreement=agree, rows=rows)
    if args.out:
        args.out.write_text(json.dumps(summary, indent=2) + "\n")
    for r in rows:
        mark = "" if (r["engine"] == "arrived") == (r["simulator"] == "success") else "  <-- differs"
        print(f"{r['name']:18} engine {r['engine']:32} sim {r['simulator']:9} "
              f"{r['simulator_seconds']:6.1f}s coll {r['simulator_collisions']:4}{mark}")
    print(f"engine {summary['engine_arrived']}/{len(rows)}, simulator {summary['simulator_success']}/{len(rows)}, "
          f"agreement {agree}/{len(rows)}")


if __name__ == "__main__":
    main()
