#!/usr/bin/env python3
"""Per-planet arrival rates of `test-autodrive.py --suite planets` reports.

Each argument is LABEL=REPORT (a driving-report.json, or a directory holding one); several
reports with the same label add up, e.g. two seeds. Prints a markdown table of arrived/trips
per planet and vehicle for each label, then the failure statuses.
"""
import argparse
import collections
import json
from pathlib import Path

PLANETS = ("vulcanus", "gleba", "fulgora")


def results(path):
    path = Path(path)
    if path.is_dir():
        path = path / "driving-report.json"
    return json.loads(path.read_text())["results"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reports", nargs="+", help="LABEL=driving-report.json")
    args = parser.parse_args()
    labels = {}
    for spec in args.reports:
        label, path = spec.split("=", 1)
        labels.setdefault(label, []).extend(results(path))
    rows = [(p, v) for p in PLANETS for v in ("car", "tank")] + [(p, None) for p in PLANETS] + [(None, None)]
    print("| planet | vehicle | " + " | ".join(labels) + " |")
    print("|---|---|" + "---:|" * len(labels))
    for planet, vehicle in rows:
        cells = []
        for trips in labels.values():
            chosen = [t for t in trips if (planet is None or t["planet"] == planet)
                      and (vehicle is None or t["name"].split("-")[1] == vehicle)]
            arrived = sum(t["status"] == "arrived" for t in chosen)
            cells.append(f"{arrived}/{len(chosen)} ({arrived / max(1, len(chosen)):.0%})")
        print(f"| {planet or 'all'} | {vehicle or 'both'} | " + " | ".join(cells) + " |")
    for label, trips in labels.items():
        failures = collections.Counter(t["status"] for t in trips if t["status"] != "arrived")
        print(f"\n{label} failures: {dict(failures)}")


if __name__ == "__main__":
    main()
