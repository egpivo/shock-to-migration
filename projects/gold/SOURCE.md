# Source: projects/gold

**Not a fabricated/synthetic sample.** Real daily close price and share
volume for **GLD** (SPDR Gold Shares, NYSE Arca, ISIN `US78463V1070`),
downloaded from the public Yahoo Finance chart API on 2026-08-10.

- Endpoint: `https://query2.finance.yahoo.com/v8/finance/chart/GLD`
- Fetched by: `cargo run -p fetch-gold-oil`
- Range: 2024-05-01 .. 2025-10-15 (365 usable rows after dropping days with
  a null price or volume field)
- Event under study: Israel strikes Iran, 2025-06-13 (see
  `.local/docs/GOLD_OIL_CASE.md` for full candidate-selection reasoning,
  instrument-choice reasoning, and limitations)

## Why GLD and not COMEX GC futures

`GC=F` (COMEX gold continuous front-month future) price data from the same
Yahoo endpoint looked fine, but its **volume** field does not — median ~428
"contracts"/day over this window, with 4 zero-volume days. Real COMEX GC
volume runs in the hundreds of thousands of contracts/day. That is a broken
or partial feed on Yahoo's side for this specific series, not a real
market fact, so `GC=F` was not used. GLD's ETF tape volume (NYSE Arca) is
well-behaved (millions of shares/day, no zero-volume days) and was used
instead. This is documented, not silent — see `project.toml`
`[data_provenance].source_description` and `.local/docs/GOLD_OIL_CASE.md`
§3.

## Independent spot-check

`2025-06-12 -> 2025-06-13` close: `312.20 -> 316.29` (+1.31%). Independently
reported spot gold moved +1.2% to +1.4% the same day per Euronews and BBC
coverage of the Israel-Iran strikes. Consistent (not identical, as expected
for an ETF proxy vs. spot).

## Limitations

- Free retail feed (Yahoo), not a licensed/audit-grade exchange tape.
- GLD tracks spot gold minus trust expenses; it is **not** COMEX gold
  futures and is **not** physical spot gold — treat as an ETF proxy for
  gold exposure, not a historical COMEX fact.
- No route/flow claim is made anywhere in this project; this is
  response-only data.
