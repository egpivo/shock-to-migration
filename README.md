# Shocktrace

Deterministic Rust toolkit for measuring market responses and directional flows around financial shocks.

It does **not** treat `A activity ↓ + B activity ↑` as proof that capital moved from A to B. Gross flow, reverse flow, net flow, route evidence, and coverage gaps stay separate empirical objects.

## Prerequisites

- Rust 1.75+ (`rustup`, stable toolchain)
- Cargo

## Installation

```bash
git clone <repo-url> shock-to-migration
cd shock-to-migration
cargo build --release
```

The CLI binary is `shocktrace` (`cargo run -p shocktrace-cli -- …`, or `./target/release/shocktrace` after a release build).

## Usage

```bash
cargo test

cargo run -p shocktrace-cli -- validate tests/synthetic_conduit
cargo run -p shocktrace-cli -- flows tests/synthetic_conduit --format summary
cargo run -p shocktrace-cli -- analyze tests/synthetic_conduit --format json
```

`tests/synthetic_conduit` is a fixture, not a real empirical case. Point the CLI at a directory that contains `project.toml` plus the declared input files.

MIT — see `LICENSE`.
