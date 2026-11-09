#!/usr/bin/env python3
"""Hits by vehicle and fuel from `fsim-rl eval --replays` episodes.

usage: python3 scripts/impact-report.py replays.json [--json out.json]

A hit is a decision after which the speed fell by more than DROP tiles/tick from above MOVING
tiles/tick (an impact, not braking: braking removes at most ~0.02 tiles/tick per decision).
Per vehicle/fuel: episodes, success, destroyed, mean damage, episodes with damage, mean top
speed, hits per episode above 50 km/h, and hits per 1000 tiles of route.
"""
import argparse
import collections
import json
import statistics
from pathlib import Path

DROP, MOVING, KMH = 0.08, 0.1, 216  # tiles/tick -> km/h


def episode_row(episode):
    speeds = [o["pose"]["speed"] for o in episode["overlays"] if o.get("pose") and "speed" in o["pose"]]
    hits = [abs(a) * KMH for a, b in zip(speeds, speeds[1:]) if abs(a) > MOVING and abs(b) < abs(a) - DROP]
    result = episode["result"]
    return dict(status=result["status"], damage=result["damage"], route=result["optimal_distance"],
                top_kmh=max((abs(s) for s in speeds), default=0) * KMH, hits=hits)


def summary(rows):
    n = len(rows)
    return dict(episodes=n,
                success=sum(r["status"] == "success" for r in rows) / n,
                destroyed=sum(r["status"] == "destroyed" for r in rows) / n,
                stalled=sum(r["status"] == "stalled" for r in rows) / n,
                mean_damage=statistics.mean(r["damage"] for r in rows),
                damaged=sum(r["damage"] > 0 for r in rows) / n,
                top_kmh=statistics.mean(r["top_kmh"] for r in rows),
                hits_over_50kmh=statistics.mean(sum(h > 50 for h in r["hits"]) for r in rows),
                hits_per_1000_tiles=1000 * sum(len(r["hits"]) for r in rows) / sum(r["route"] for r in rows))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("replays", type=Path)
    parser.add_argument("--json", type=Path, help="also write the table as JSON")
    args = parser.parse_args()
    groups = collections.defaultdict(list)
    for episode in json.loads(args.replays.read_text()):
        row = episode_row(episode)
        groups[f"{episode['vehicle']}/{episode['fuel']}"].append(row)
        groups["all"].append(row)
    table = {key: summary(rows) for key, rows in sorted(groups.items())}
    print("| vehicle/fuel | n | success | destroyed | stalled | damage | damaged | top km/h | hits >50 km/h | hits /1k tiles |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for key, s in table.items():
        print(f"| {key} | {s['episodes']} | {s['success']:.1%} | {s['destroyed']:.1%} | {s['stalled']:.1%} | "
              f"{s['mean_damage']:.1f} | {s['damaged']:.0%} | {s['top_kmh']:.0f} | {s['hits_over_50kmh']:.2f} | "
              f"{s['hits_per_1000_tiles']:.2f} |")
    if args.json:
        args.json.write_text(json.dumps(table, indent=1))


if __name__ == "__main__":
    main()
