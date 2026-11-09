#!/usr/bin/env python3
"""Summarize `fsim-rl suite` JSON files as one markdown table per pool.

Each argument is LABEL=PATH. Rows: scenario kinds plus an "all" row (episode-weighted
means). Columns per label: success, SPL, mean time on success-weighted episodes, damage.
"""
import argparse
import json
from pathlib import Path

METRICS = (("success_rate", "success", "{:.1%}"), ("spl", "SPL", "{:.3f}"),
           ("mean_time_s", "time s", "{:.1f}"), ("mean_damage", "damage", "{:.2f}"),
           ("destroyed_rate", "destroyed", "{:.1%}"))


def load(spec):
    label, path = spec.split("=", 1)
    data = json.loads(Path(path).read_text())
    # Suites hold model and (optionally) baseline reports per scenario kind.
    columns = {}
    for scenario, reports in data["rows"].items():
        for policy, report in reports.items():
            name = label if policy == "model" else f"{label} (A*)" if label else "A*"
            columns.setdefault(name, {})[scenario] = report
    return columns


def overall(reports):
    total = sum(r["episodes"] for r in reports.values())
    return {key: sum(r[key] * r["episodes"] for r in reports.values()) / total
            for key, _, _ in METRICS} | {"episodes": total}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("suites", nargs="+", help="LABEL=suite.json")
    parser.add_argument("--metrics", default="success_rate,spl,mean_time_s",
                        help="comma-separated keys among " + ",".join(m[0] for m in METRICS))
    args = parser.parse_args()
    columns = {}
    for spec in args.suites:
        for name, rows in load(spec).items():
            columns.setdefault(name, rows)
    keys = [m for m in METRICS if m[0] in args.metrics.split(",")]
    scenarios = list(next(iter(columns.values())))
    header = ["scenario"] + [f"{name} {label}" for name in columns for _, label, _ in keys]
    print("| " + " | ".join(header) + " |")
    print("|---" + "|---:" * (len(header) - 1) + "|")
    for scenario in scenarios + ["all"]:
        cells = [scenario]
        for rows in columns.values():
            report = overall(rows) if scenario == "all" else rows[scenario]
            cells += [fmt.format(report[key]) for key, _, fmt in keys]
        print("| " + " | ".join(cells) + " |")


if __name__ == "__main__":
    main()
