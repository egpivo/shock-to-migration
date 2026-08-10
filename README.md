# Shocktrace

Deterministic Rust toolkit for measuring market responses and directional flows around financial shocks.

It does **not** treat `A activity ↓ + B activity ↑` as proof that capital moved from A to B. Response, route evidence, and directional flow stay separate; missing flow is never encoded as zero.

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

cargo run -p shocktrace-cli -- validate tests/synthetic_conduit
cargo run -p shocktrace-cli -- flows tests/synthetic_conduit --format summary
cargo run -p shocktrace-cli -- analyze tests/synthetic_conduit --format json

cargo run -p shocktrace-cli -- validate tests/gold_fixture
cargo run -p shocktrace-cli -- respond tests/gold_fixture --format summary
cargo run -p shocktrace-cli -- flows tests/gold_fixture --format summary   # not_declared (exit 0)
cargo run -p shocktrace-cli -- analyze tests/oil_fixture --format json
```

Exit codes: `0` on successful measurement **or** structured absence (`not_declared` / `not_observable`); `1` on validation/ingest/accounting errors.

`respond` / `flows` / `analyze` each print an evidence boundary for the sections they show. A number without coverage context is incomplete output.

Fixtures under `tests/` are synthetic samples for architecture checks, not historical claims.

MIT — see `LICENSE`.
