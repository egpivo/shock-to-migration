#!/usr/bin/env python3
"""Re-run the PAXG/WTIC measures under nearby WTIC dust thresholds."""

from __future__ import annotations

import csv
import json
import shutil
import subprocess
import tempfile
from pathlib import Path


HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
PROJECT = REPO / "projects" / "paxg_wtic_reference_2026_07_08"
THRESHOLDS = (0, 50, 100, 250)


def run_json(binary: Path, project: Path, args: list[str]) -> dict:
    completed = subprocess.run(
        [str(binary), *args, str(project), "--format", "json"],
        cwd=REPO,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def main() -> None:
    subprocess.run(
        ["cargo", "build", "-q", "-p", "shocktrace-cli"],
        cwd=REPO,
        check=True,
    )
    binary = REPO / "target" / "debug" / "shocktrace"

    response_path = PROJECT / "data" / "response_daily.csv"
    with response_path.open(newline="", encoding="utf-8") as handle:
        paxg_rows = [
            row for row in csv.DictReader(handle) if row["asset_key"] == "PAXG"
        ]

    raw_path = PROJECT / "data" / "wtic_pool_ohlcv_raw.json"
    candles = json.loads(raw_path.read_text(encoding="utf-8"))["candles"]

    writer = csv.writer(__import__("sys").stdout, lineterminator="\n")
    writer.writerow(
        [
            "threshold_usd",
            "wtic_priced_rows",
            "wtic_baseline_n",
            "wtic_event_return",
            "wtic_z",
            "divergence_baseline_n",
            "divergence_event_gap",
            "divergence_z",
        ]
    )

    for threshold in THRESHOLDS:
        wtic_rows = [
            {
                "asset_key": "WTIC",
                "day": candle["day"],
                "price": candle["close"] if candle["volume_usd"] >= threshold else "",
                "volume": candle["volume_usd"],
            }
            for candle in candles
        ]
        rows = sorted(paxg_rows + wtic_rows, key=lambda row: (row["asset_key"], row["day"]))

        with tempfile.TemporaryDirectory(prefix="shocktrace-threshold-") as temp:
            project = Path(temp)
            (project / "data").mkdir()
            shutil.copy2(PROJECT / "project.toml", project / "project.toml")
            shutil.copy2(
                PROJECT / "data" / "reference_returns.csv",
                project / "data" / "reference_returns.csv",
            )
            with (project / "data" / "response_daily.csv").open(
                "w", newline="", encoding="utf-8"
            ) as handle:
                out = csv.DictWriter(
                    handle, fieldnames=["asset_key", "day", "price", "volume"]
                )
                out.writeheader()
                out.writerows(rows)

            wtic = run_json(
                binary,
                project,
                ["measure", "shock", "--asset", "WTIC"],
            )
            divergence = run_json(
                binary,
                project,
                [
                    "measure",
                    "divergence",
                    "--asset-a",
                    "PAXG",
                    "--asset-b",
                    "WTIC",
                ],
            )

        writer.writerow(
            [
                threshold,
                sum(candle["volume_usd"] >= threshold for candle in candles),
                wtic["shock"]["baseline_n"],
                wtic["shock"]["event_return"],
                wtic["shock"]["z_score"],
                divergence["baseline_n"],
                divergence["event_divergence"],
                divergence["z_score"],
            ]
        )


if __name__ == "__main__":
    main()
