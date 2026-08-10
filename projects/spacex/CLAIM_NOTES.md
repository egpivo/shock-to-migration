# Claim notes: projects/spacex

Documentation-only file. No changes were made to `project.toml`,
`data/*.csv`, or any engine formula in this project — those already
reproduce the article's headline 3.02% net/denominator figure and remain
untouched.

For the full reconciliation of article numbers against engine output
(including two corrected claims — "six days" and the 0.55× event-week
volume ratio — and a table of numbers that already PASS), see:

- [`.local/docs/CLAIM_GATE.md`](../../.local/docs/CLAIM_GATE.md)
- [`artifacts/claim_gate.csv`](../../artifacts/claim_gate.csv) (machine-readable)

Reproduce the authoritative numbers yourself with:

```bash
cargo run -q -p shocktrace-cli -- analyze projects/spacex
```
