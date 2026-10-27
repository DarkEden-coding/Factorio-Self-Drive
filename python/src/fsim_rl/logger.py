"""TensorBoard scalars plus a CSV with one row per iteration."""

from __future__ import annotations

import csv
import math
from pathlib import Path

from torch.utils.tensorboard import SummaryWriter


class RunLogger:
    def __init__(self, run_dir: Path):
        self.writer = SummaryWriter(log_dir=str(run_dir / "tb"))
        self.csv_path = run_dir / "metrics.csv"
        self.fieldnames: list[str] | None = None
        if self.csv_path.exists():
            with self.csv_path.open(newline="") as f:
                self.fieldnames = next(csv.reader(f), None)

    def log(self, step: int, metrics: dict[str, float]) -> None:
        for key, value in metrics.items():
            if not math.isnan(value):
                self.writer.add_scalar(key, value, step)
        row = {"step": step, **metrics}
        new_file = self.fieldnames is None
        if new_file:
            self.fieldnames = list(row)
        with self.csv_path.open("a", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=self.fieldnames, extrasaction="ignore")
            if new_file:
                writer.writeheader()
            writer.writerow(row)

    def close(self) -> None:
        self.writer.close()
