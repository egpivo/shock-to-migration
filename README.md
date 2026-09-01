# Shocktrace

Deterministic Rust toolkit for measuring market responses and directional flows around financial shocks.

- Keeps **market response**, **route evidence**, and **directional flow** separate.
- Never treats `A activity ↓ + B activity ↑` as proof that capital moved from A to B.
- Missing flow stays missing; it is never encoded as zero.
- Every reported number carries an evidence boundary.

## What it measures

- Event-day shock response
- Reference-to-token response gaps
- Post-event persistence
- Cross-asset divergence
- Linked and directional flows
- Cross-project comparisons

## Build

```bash
git clone <repo-url> shock-to-migration
cd shock-to-migration
cargo build --release
cargo test
```

- Requires Rust 1.75+.

## Examples
```bash
# Validate a project
cargo run -p shocktrace-cli -- validate projects/paxg_wtic_reference_2026_07_08

# Standardized shock
cargo run -p shocktrace-cli -- measure shock \
  projects/paxg_wtic_reference_2026_07_08 \
  --asset PAXG --format summary

# Reference-to-token response gap
cargo run -p shocktrace-cli -- measure response-gap \
  projects/paxg_wtic_reference_2026_07_08 \
  --asset PAXG --reference GOLD_SPOT --format summary

# Cross-token divergence
cargo run -p shocktrace-cli -- measure divergence \
  projects/paxg_wtic_reference_2026_07_08 \
  --asset-a PAXG --asset-b WTIC --format summary

# Linked-flow case
cargo run -p shocktrace-cli -- flows projects/spacex --format summary

# Compare projects
cargo run -p shocktrace-cli -- compare \
  projects/spacex \
  projects/paxg_wtic_reference_2026_07_08 \
  --format summary
```

## License

MIT — see [LICENSE](LICENSE).
