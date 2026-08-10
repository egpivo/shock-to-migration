# Source: projects/oil

**Not a fabricated/synthetic sample.** Real daily settle price and contract
volume for **CL=F** (NYMEX WTI crude oil, continuous front-month future),
downloaded from the public Yahoo Finance chart API on 2026-08-10.

- Endpoint: `https://query2.finance.yahoo.com/v8/finance/chart/CL=F`
- Fetched by: `cargo run -p fetch-gold-oil`
- Range: 2024-05-01 .. 2025-10-15 (367 usable rows after dropping days with
  a null price or volume field)
- Event under study: Israel strikes Iran, 2025-06-13 (see
  `.local/docs/GOLD_OIL_CASE.md` for full candidate-selection reasoning,
  instrument-choice reasoning, and limitations)

## Why CL=F (kept as a continuous future, unlike gold)

Volume field for this window is a plausible order of magnitude throughout
(min/median/max = 0 / 291,260 / 728,868 contracts/day, 4 zero-volume days
otherwise unremarkable), so it was kept as-is rather than switched to an
ETF proxy (USO). Gold was switched to an ETF (GLD) for a data-quality
reason specific to `GC=F`'s volume feed — see `projects/gold/SOURCE.md`.

## Independent spot-check

`2025-06-12 -> 2025-06-13` settle: `68.0400 -> 72.9800` (+7.26%). Reuters
("Oil settles up 7% as Israel, Iran trade air strikes") and CNBC both
report WTI settled 13 Jun 2025 at **$72.98/bbl, up 7.26%** (or "$4.94"
higher) — an exact match to the frozen series, strong evidence this is the
real historical tape rather than a fabricated or mismatched series.

## Limitations

- Free retail feed (Yahoo), not a licensed/audit-grade CME/NYMEX tape.
- `instrument_id = "CL"` denotes the continuous front-month roll, not one
  fixed contract month; roll-day price/volume discontinuities are not
  separately flagged in the frozen series.
- 4 zero-volume days in the raw feed were dropped (missing, not
  zero-filled) and were not individually investigated.
- No route/flow claim is made anywhere in this project; this is
  response-only data.
