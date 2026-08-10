#!/usr/bin/env python3
"""Check the 2026-09-01 article metrics against current shocktrace JSON."""

from __future__ import annotations

import csv
import io
import json
import subprocess
import sys
from datetime import date
from decimal import Decimal
from pathlib import Path


HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
CLAIMS = REPO / "artifacts" / "claim_gate.csv"
PREFIX = "article_2026_09_01_"
PROJECT = "projects/paxg_wtic_reference_2026_07_08"


def run_json(args: list[str]) -> dict:
    completed = subprocess.run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "shocktrace-cli",
            "--",
            *args,
            "--format",
            "json",
        ],
        cwd=REPO,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def main() -> None:
    paxg = run_json(["measure", "shock", PROJECT, "--asset", "PAXG"])
    wtic = run_json(["measure", "shock", PROJECT, "--asset", "WTIC"])
    paxg_pass = run_json(
        [
            "measure",
            "passthrough",
            PROJECT,
            "--asset",
            "PAXG",
            "--reference",
            "GOLD_SPOT",
        ]
    )
    wtic_pass = run_json(
        [
            "measure",
            "passthrough",
            PROJECT,
            "--asset",
            "WTIC",
            "--reference",
            "WTI_FRONT_MONTH",
        ]
    )
    div = run_json(
        [
            "measure",
            "divergence",
            PROJECT,
            "--asset-a",
            "PAXG",
            "--asset-b",
            "WTIC",
        ]
    )

    def horizon(report: dict, sessions: int) -> Decimal:
        row = next(
            item
            for item in report["horizons"]["horizons"]
            if item["horizon_sessions"] == sessions
        )
        return Decimal(row["cumulative_return"])

    project_dir = REPO / PROJECT
    response_path = project_dir / "data" / "response_daily.csv"
    event_volume: dict[str, Decimal] = {}
    wtic_surface_days = 0
    wtic_dust_days = 0
    with response_path.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            day = date.fromisoformat(row["day"])
            if day == date(2026, 7, 8):
                event_volume[row["asset_key"]] = Decimal(row["volume"])
            if row["asset_key"] == "WTIC":
                wtic_surface_days += 1
                if Decimal(row["volume"]) < Decimal(100):
                    wtic_dust_days += 1

    sensitivity_output = subprocess.run(
        [sys.executable, str(HERE / "threshold_sensitivity.py")],
        cwd=REPO,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    sensitivity = list(csv.DictReader(io.StringIO(sensitivity_output)))
    sensitivity_wtic_z = [Decimal(row["wtic_z"]) for row in sensitivity]
    sensitivity_div_z = [Decimal(row["divergence_z"]) for row in sensitivity]

    with (project_dir / "data" / "pools_frozen.json").open(encoding="utf-8") as handle:
        paxg_pool_count = Decimal(json.load(handle)["pool_count"])
    with (project_dir / "data" / "wtic_pools_frozen.json").open(encoding="utf-8") as handle:
        wtic_pool_count = Decimal(len(json.load(handle)["pools"]))

    actual: dict[str, Decimal] = {
        "article_2026_09_01_paxg_return": Decimal(paxg["shock"]["event_return"]),
        "article_2026_09_01_paxg_z": Decimal(paxg["shock"]["z_score"]),
        "article_2026_09_01_wtic_return": Decimal(wtic["shock"]["event_return"]),
        "article_2026_09_01_wtic_z": Decimal(wtic["shock"]["z_score"]),
        "article_2026_09_01_wtic_baseline_n": Decimal(wtic["shock"]["baseline_n"]),
        "article_2026_09_01_gold_reference_return": Decimal(
            paxg_pass["reference_return"]
        ),
        "article_2026_09_01_paxg_passthrough_gap": Decimal(
            paxg_pass["response_gap"]
        ),
        "article_2026_09_01_wti_reference_return": Decimal(
            wtic_pass["reference_return"]
        ),
        "article_2026_09_01_wtic_passthrough_gap": Decimal(
            wtic_pass["response_gap"]
        ),
        "article_2026_09_01_div_paxg_wtic": Decimal(div["event_divergence"]),
        "article_2026_09_01_div_paxg_wtic_z": Decimal(div["z_score"]),
        "article_2026_09_01_paxg_h1": horizon(paxg, 1),
        "article_2026_09_01_paxg_h3": horizon(paxg, 3),
        "article_2026_09_01_paxg_h5": horizon(paxg, 5),
        "article_2026_09_01_paxg_h20": horizon(paxg, 20),
        "article_2026_09_01_wtic_h1": horizon(wtic, 1),
        "article_2026_09_01_wtic_h3": horizon(wtic, 3),
        "article_2026_09_01_wtic_h5": horizon(wtic, 5),
        "article_2026_09_01_div_baseline_n": Decimal(div["baseline_n"]),
        "article_2026_09_01_wtic_event_volume": event_volume["WTIC"],
        "article_2026_09_01_paxg_pool_count": paxg_pool_count,
        "article_2026_09_01_wtic_pool_count": wtic_pool_count,
        "article_2026_09_01_wtic_surface_days": Decimal(wtic_surface_days),
        "article_2026_09_01_wtic_dust_days": Decimal(wtic_dust_days),
        "article_2026_09_01_sensitivity_wtic_z_min": min(sensitivity_wtic_z),
        "article_2026_09_01_sensitivity_wtic_z_max": max(sensitivity_wtic_z),
        "article_2026_09_01_sensitivity_div_z_min": min(sensitivity_div_z),
        "article_2026_09_01_sensitivity_div_z_max": max(sensitivity_div_z),
    }

    with CLAIMS.open(newline="", encoding="utf-8") as handle:
        rows = [
            row
            for row in csv.DictReader(handle)
            if row["claim_id"].startswith(PREFIX) and row["status"] == "PASS"
        ]

    failures: list[str] = []
    for row in rows:
        claim_id = row["claim_id"]
        if claim_id not in actual:
            failures.append(f"{claim_id}: verifier mapping missing")
            continue
        observed = actual[claim_id]
        expected = Decimal(row["expected_value"])
        tolerance = Decimal(row["tolerance"])
        if abs(observed - expected) > tolerance:
            failures.append(
                f"{claim_id}: expected {expected} ± {tolerance}, got {observed}"
            )

    if failures:
        print("CLAIM GATE: FAIL")
        for failure in failures:
            print(f"  - {failure}")
        raise SystemExit(1)

    print(f"CLAIM GATE: PASS — {len(rows)} article metrics match current freeze.")


if __name__ == "__main__":
    main()
