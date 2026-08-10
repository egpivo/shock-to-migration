# Source: projects/paxg_wtic_reference_2026_07_08

**Primary empirical objects:** on-chain RWA tokens **PAXG** and **WTIC** only.
The project also freezes two source-reported event-day commodity returns for a same-date pass-through comparison. It does **not** store off-chain price history.

## Canonical identities

### Tokenized gold — PAXG

| Field | Value |
|---|---|
| Symbol | PAXG |
| Chain | Ethereum |
| ERC-20 | `0x45804880de22913dafe09f4980848ece6ecbaf78` |
| Product | Paxos Gold |
| Role | `primary_onchain_gold` |

### Tokenized oil — WTIC

| Field | Value |
|---|---|
| Symbol | WTIC |
| Chain | Ethereum |
| ERC-20 | `0x709ab533D18e652eCd56423d71c0241A0ee56a3b` |
| Name (on-chain) | WTI Coin |
| Decimals | 6 |
| Total supply (freeze check) | 4150 WTIC |
| Issuer | Energy Substantiation Partners, LLC |
| Public secondary surface | Uniswap v3 WTIC/USDC |
| Role | `primary_onchain_oil` |

Independently verified on-chain: name, symbol, decimals, bytecode present, GeckoTerminal token+pools.
**Reserve backing** (1:1 WTI via energy receipts) is an **issuer claim** unless a named attestation is attached — not treated as proven in this freeze.

## Event

- `event.id = us_iran_renewed_escalation_2026_07_08`
- Renewed U.S.–Iran escalation after an interim truce faltered — **not** war onset.
- After WTIC deployment (pool created 2026-03-11), so a paired on-chain case is feasible.

## Event-day reference observations

Reuters' 8 July cross-market close report states that spot gold fell `0.52%` to `$4,084.19` per ounce and front-month WTI rose `4.4%` to `$73.52` per barrel. These reported returns are frozen in `data/reference_returns.csv` as `GOLD_SPOT` and `WTI_FRONT_MONTH`.

They are comparison points, not additional research subjects. Their cutoffs are not synchronized to the UTC pool candles, so `measure passthrough` reports a same-date response gap—not beta, causal transmission, or a synchronized tracking error. Source metadata lives in `data/REFERENCE_META.json`.

## Feasibility gates (passed before drafting)

1. Both contracts confirmed from chain/GeckoTerminal primary reads.
2. Pool universes frozen before examining event z-scores (`pools_frozen.json`, `wtic_pools_frozen.json`).
3. WTIC traded before and on 2026-07-08 (event-day pool USD volume ~ $3.5k; priced observation retained).
4. The `$100` minimum traded-notional rule was frozen before computing the event metrics. Dust / near-no-trade days have **price left missing** — not converted into zero return.
5. A daily return requires prices on adjacent UTC calendar days. A missing row or missing price breaks the chain; the engine does not relabel a multi-day move as daily.
6. No-trade / dust ≠ fabricated zero activity for the price series.
7. Baseline windows checked for adequacy; thin WTIC history remains part of the result (`low_baseline` when applicable).
8. On-chain candle day = UTC date of GeckoTerminal timestamp.
9. Unsupported surfaces (mint/burn, depth, attestation, off-chain history) stay **unavailable** / out of repo.

### WTIC pool freeze rule

Only pools **created before 2026-07-08** enter the universe.
Frozen: `0xdd109a10e918ed6bb51aef0f5650493f552ff0aa` (WTIC/USDC 0.3%, created 2026-03-11).
Excluded: `0x95e707ca…` (WTIC/USDC 0.05%, created **2026-08-05**, after the event).

## Market-quality asymmetry (result, not bug)

| | PAXG | WTIC |
|---|---|---|
| Pre-event pools in freeze | 20 Ethereum pools | 1 Uniswap v3 pool |
| Event-day pool USD volume | ~$5.2M | ~$3.5k |
| Token supply scale | large | ~4150 tokens |
| Dust/unpriced days in surface | rare | 23 / 113 under $100 rule |

Do **not** compare PAXG and WTIC as if market quality were equal. Thin evidence for WTIC is part of the measurement.

## Dust-threshold sensitivity

The article reruns WTIC at `$0`, `$50`, `$100`, and `$250` minimum daily pool volume. Across those settings, the WTIC event score remains `+1.56` to `+1.61` standard deviations and PAXG−WTIC divergence remains `−1.57` to `−1.63`. Sample size changes; direction and interpretation do not. See `artifacts/articles/2026-09-01/threshold_sensitivity.csv`.

## Rejected oil substitutes

Not used as the oil RWA leg: `CVXx`, `USOX`, `OILX`, `XOPx`, GCEX WTI/USD, oil perpetuals.

## Do not claim

- Capital moved between gold and oil tokens.
- Public pools are the whole PAXG or WTIC market.
- Cross-asset divergence is migration.
- Issuer backing is proven without attestation.
- WTIC liquidity is comparable to PAXG.
- A full off-chain gold/WTI history or synchronized intraday marks were analyzed.
