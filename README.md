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

cargo run -p shocktrace-cli -- validate projects/gold
cargo run -p shocktrace-cli -- respond projects/gold --format summary
cargo run -p shocktrace-cli -- flows projects/gold --format summary   # not_declared (exit 0)
cargo run -p shocktrace-cli -- analyze projects/oil --format json

cargo run -p shocktrace-cli -- compare projects/spacex projects/gold projects/oil --format summary

cargo run -p shocktrace-cli -- measure shock projects/gold --asset GLD --format summary
cargo run -p shocktrace-cli -- measure shock projects/oil --asset CL --format summary
cargo run -p shocktrace-cli -- measure divergence projects/gold_oil --asset-a GLD --asset-b CL --format summary
```

Exit codes: `0` on successful measurement **or** structured absence (`not_declared` / `not_observable`); `1` on validation/ingest/accounting errors.

`respond` / `flows` / `analyze` each print an evidence boundary for the sections they show. A number without coverage context is incomplete output.

Real cases live under `projects/`: SpaceX (linked mint-pair flow), Gold/Oil (frozen Yahoo daily response; GLD ETF + CL continuous), and `gold_oil` (paired for divergence). Fixtures under `tests/` remain architecture probes.

To deliberately re-freeze gold/oil Yahoo CSVs (overwrites frozen inputs):

```bash
cargo run -p fetch-gold-oil -- --start 2024-05-01 --end 2025-10-15
```

Claim-gate for article numbers: `artifacts/claim_gate.csv`.

MIT — see `LICENSE`.
