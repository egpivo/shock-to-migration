# Shocktrace

Deterministic Rust toolkit for measuring market responses and directional flows around financial shocks.

It does **not** treat `A activity ↓ + B activity ↑` as proof that capital moved from A to B. Response, route evidence, and directional flow stay separate; missing flow is never encoded as zero.

Measurement rules: `.local/docs/MEASUREMENT_CONTRACT.md` (gitignored local notes).

## Prerequisites

- Rust 1.75+ (`rustup`, stable toolchain)
- Cargo

## Installation

```bash
git clone <repo-url> shock-to-migration
cd shock-to-migration
cargo build --release
```

## Usage

```bash
cargo test

cargo run -p shocktrace-cli -- validate projects/spacex
cargo run -p shocktrace-cli -- flows projects/spacex --format summary
cargo run -p shocktrace-cli -- analyze projects/spacex --format json

cargo run -p shocktrace-cli -- validate projects/paxg_wtic_reference_2026_07_08
cargo run -p shocktrace-cli -- measure shock projects/paxg_wtic_reference_2026_07_08 --asset PAXG --format summary
cargo run -p shocktrace-cli -- measure shock projects/paxg_wtic_reference_2026_07_08 --asset WTIC --format summary
cargo run -p shocktrace-cli -- measure passthrough projects/paxg_wtic_reference_2026_07_08 \
  --asset PAXG --reference GOLD_SPOT --format summary
cargo run -p shocktrace-cli -- measure divergence projects/paxg_wtic_reference_2026_07_08 \
  --asset-a PAXG --asset-b WTIC --format summary

cargo run -p shocktrace-cli -- compare projects/spacex projects/paxg_wtic_reference_2026_07_08 --format summary
```

Exit codes: `0` on successful measurement **or** structured absence (`not_declared` / `not_observable`); `1` on validation/ingest/accounting errors.

`respond` / `flows` / `analyze` each print an evidence boundary for the sections they show. A number without coverage context is incomplete output.

Real cases live under `projects/`: SpaceX (linked mint-pair flow) and `paxg_wtic_reference_2026_07_08` (on-chain PAXG + WTIC public pools for 2026-07-08). The latter stores two source-reported event-day commodity returns for pass-through comparison, but no Yahoo/TradFi price history. Fixtures under `tests/` remain architecture probes.

Claim-gate for article numbers: `artifacts/claim_gate.csv` (verifier: `artifacts/articles/2026-09-01/verify_claims.py`).

MIT — see `LICENSE`.
