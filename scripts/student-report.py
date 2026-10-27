"""Builds the categorized teacher / A* / student comparison tables from suite JSONs.

Usage (from python/): uv run python ../scripts/student-report.py runs/students
Reads <dir>/teacher-suite.json (teacher as "model", A* as "baseline"), and for each student
<name>: <name>-distill-suite.json, <name>-rl-suite.json and <name>-<stage>-export.json (the output
of `fsim-rl export-student`). Prints Markdown: one table per scenario kind, then a summary.
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

from fsim_rl.config import SCENARIO_KINDS
from fsim_rl.evaluate import SUITE_COLUMNS

STUDENTS = ("mlp16", "mlp32", "mlp64", "mlp128", "conv")
STAGES = (("distill", "distilled"), ("rl", "+ PPO"))


def load(path: Path) -> dict | None:
    return json.loads(path.read_text()) if path.exists() else None


def cell(value, fmt: str) -> str:
    return "-" if value is None or (isinstance(value, float) and math.isnan(value)) else fmt.format(value)


def main(directory: str) -> None:
    root = Path(directory)
    teacher = load(root / "teacher-suite.json")
    policies: list[tuple[str, dict]] = [
        ("teacher", {k: teacher["rows"][k]["model"] for k in SCENARIO_KINDS}),
        ("A* baseline", {k: teacher["rows"][k]["baseline"] for k in SCENARIO_KINDS}),
    ]
    exports: dict[str, dict] = {}
    for name in STUDENTS:
        for stage, label in STAGES:
            suite = load(root / f"{name}-{stage}-suite.json")
            if suite is not None:
                policies.append((f"{name} {label}", {k: suite["rows"][k]["model"] for k in SCENARIO_KINDS}))
            export = load(root / f"{name}-{stage}-export.json")
            if export is not None:
                exports[f"{name} {label}"] = export

    header = ["policy", *(name for name, _, _ in SUITE_COLUMNS)]
    for kind in SCENARIO_KINDS:
        print(f"\n### {kind}\n")
        print("| " + " | ".join(header) + " |")
        print("|---|" + "---:|" * len(SUITE_COLUMNS))
        for name, rows in policies:
            cells = [cell(value(rows[kind]), fmt) for _, value, fmt in SUITE_COLUMNS]
            print(f"| {name} | " + " | ".join(cells) + " |")

    print("\n### Summary: % of teacher success (SPL) per category\n")
    print("| policy | " + " | ".join(SCENARIO_KINDS) + " | mean | params | MACs | export KB | Lua ms (net / +rays) |")
    print("|---|" + "---:|" * (len(SCENARIO_KINDS) + 5))
    teacher_rows = policies[0][1]
    for name, rows in policies[1:]:
        ratios, cells = [], []
        for kind in SCENARIO_KINDS:
            success = rows[kind]["success_rate"] / teacher_rows[kind]["success_rate"] * 100
            spl = rows[kind]["spl"] / teacher_rows[kind]["spl"] * 100
            ratios.append(success)
            cells.append(f"{success:.0f} ({spl:.0f})")
        export = exports.get(name)
        if export is None:
            cost = ["-"] * 4
        else:
            lua = export["lua"]
            cost = [f"{export['params']:,}", f"{export['macs']:,}", f"{export['kb']:.0f}",
                    f"{lua['network_ms']:.2f} / {lua['total_ms']:.2f}"]
        print(f"| {name} | " + " | ".join(cells) + f" | {sum(ratios) / len(ratios):.1f} | " + " | ".join(cost) + " |")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "runs/students")
