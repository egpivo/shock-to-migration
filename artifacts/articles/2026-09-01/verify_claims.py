#!/usr/bin/env python3
"""Check the 2026-09-01 article metrics against current shocktrace JSON."""

from __future__ import annotations

import csv
import json
import subprocess
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

    actual: dict[str, Decimal] = {
        "article_2026_09_01_paxg_return": Decimal(paxg["shock"]["event_return"]),
        "article_2026_09_01_paxg_z": Decimal(paxg["shock"]["z_score"]),
        "article_2026_09_01_paxg_activity": Decimal(paxg["activity"]["ratio"]),
        "article_2026_09_01_wtic_return": Decimal(wtic["shock"]["event_return"]),
        "article_2026_09_01_wtic_z": Decimal(wtic["shock"]["z_score"]),
        "article_2026_09_01_wtic_activity": Decimal(wtic["activity"]["ratio"]),
        "article_2026_09_01_div_paxg_wtic": Decimal(div["event_divergence"]),
        "article_2026_09_01_div_paxg_wtic_z": Decimal(div["z_score"]),
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
