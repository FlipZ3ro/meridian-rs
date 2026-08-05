# Dual-side LP — implementation brief

Starting point for the dual-side work. Written 2026-08-05, right after the
single-side bot went live on vps3, so the "current state" below is what you are
branching away from.

## Why

Single-side deposits SOL into bins **below** the active price. Fees only accrue
once price trades down into that range — if price rises, the position earns
nothing and exits as 100% SOL. Dual-side sits around the active bin and earns
from tick one.

The cost is not small: dual-side holds the base token from entry, so
directional exposure and impermanent loss start immediately. Single-side's
"free" upside exit disappears — a live example from 2026-08-05: JLY went
out-of-range **above** at +12.58%, all SOL, zero IL. Dual-side would have been
selling into that move.

## Current state you are branching from

- Live bot: **vps3** (`3k7tWvC9ZnfFjASwb8zJn43E4pF1oi68sXX1cy2hBEGq`), pm2
  process `meridian-tele`, repo at `/root/meridian-build`, branch `master`.
- vps2 (`meridian-dash`) is **stopped** and is the LLM proxy host — do not
  shut it down, vps3 calls it for every screening/management decision.
- Config baseline: `deployAmountSol 0.5`, `maxPositions 4`, `stopLossPct -6`,
  `trailingTriggerPct 6` / `trailingDropPct 3`, `exitMinProfitPct 2.0`,
  `exitRsiThreshold 101` (RSI gate deliberately disabled, %B only),
  `skipToken2022 false`.
- A single-side **baseline measurement** is running for 2–3 days. PnL and fee
  persistence only started working on 2026-08-05, so this is the first valid
  data the project has ever had. Do not deploy dual-side to vps3 until that
  baseline exists, or the comparison is worthless.

## What has to change

### 1. Acquire the base token before depositing
`deploy_position` currently hardcodes `amount_x: 0` — SOL only.

- wp path: `backend/src/tools/meteora_native.rs`, `deploy_position`,
  `AddLiquidityParams { amount_x: 0, amount_y, .. }`
- commons path: same file, `deploy_position_commons`,
  `LiquidityParameterByStrategy { amount_x: 0, amount_y, .. }`

Dual-side needs roughly half the position value swapped SOL → base token first
(`tools::wallet::swap_token`), then both amounts supplied. Budget for slippage:
that swap is a real cost single-side never pays.

### 2. Strategy type
`strategy_type_from_name` (wp) and `strategy_type_commons` return the
`*ImBalanced` variants, which is correct for a one-sided deposit. Balanced
deposits need `SpotBalanced` / `CurveBalanced` / `BidAskBalanced`. Picking the
wrong one is not a soft failure — the program rejects it
(`InvalidStrategyParameters`, 0x17a6).

### 3. The exit ladder — the hard part
This is not an add-on; it changes what the existing rules mean.

`backend/src/state/positions.rs`, `get_deterministic_close_rule`:

- **OOR-below** currently means "IL is being realised, cut it"
  (`OOR_CLOSE_LOSS_PCT = -4.0`, `out_of_range_wait_minutes = 8`, hard stop at
  `OOR_MAX_HOLD_MULT = 3`× that). Under dual-side you are already holding the
  token, so the trigger point is different.
- **OOR-above** currently means "all-SOL winner, safe" — under dual-side it
  means you have sold the whole position into a rally.
- Trailing and the %B over-extended exit were both tuned against single-side
  PnL curves and will need re-checking, not just re-using.

Treat this as the main body of work. Steps 1 and 2 are mechanical.

## Do not

- Deploy this branch to vps3 while the single-side baseline is running.
- Change `master` — it is what the live bot runs.
- Delete or edit `/root/.meridian-keys/tele-bot.json` (wallet key, backed up
  by the operator) or the live `.env` / `user-config.json` on vps3.

## Verify before it touches money

`deploy_position_commons` takes a `simulate_only` flag and there is a CLI for
it. Simulation runs the transaction through a validator and reports the exact
failure without broadcasting:

```bash
ssh vps3 'cd /root/meridian-build/backend && set -a && . ./.env && set +a && \
  ./target/release/meridian-rs deploy-commons --pool <POOL> --amount-sol 0.05'
```

Add `--confirm` to send for real. Build on vps3 (`/root/meridian-build`,
~1 min incremental); vps2's disk is 55% full and Windows cannot build this tree
(openssl).

## Status — 2026-08-06

Steps 1 and 2 are done on `feat/dual-side` and verified by simulation. Step 3
is untouched and still needs a decision before anyone writes code.

Dual-side is off by default (`management.dualSideEnabled`), so master's
behaviour and the running baseline are unchanged. Its knobs:
`dualSideBasePct` (0.5), `dualSideSlippageBps` (100), `dualSideBinsBelow` /
`dualSideBinsAbove` (30/30 — its own range, because the single-side coverage
math is downside-only and clears the 69-bin ceiling once a top half is added).

What simulation proved, against the live `turtleneck-SOL` pool
(`88jWdp2ns8JzyKbwTFxunsPmRMpgAAyX4eJmgSYBTXtD`, Token-2022, wallet already
held 4.41e9 base units):

| Run | Result |
|---|---|
| single-side, 0.05 SOL | `SpotImBalanced`, no error, 107,879 CU — unchanged |
| dual-side, 0.05 SOL, bins 30/30 | `SpotBalanced`, no error, 372,237 CU, amount_x 4,410,933,375 + amount_y 0.025 SOL |
| dual-side, `--bins-above 0` | refused, as intended |
| Jupiter SOL → base quote | routes: 0.025 SOL → 6.43e9 units, price impact 0.017% |

So the balanced strategy type is accepted (no `InvalidStrategyParameters`),
liquidity lands on both sides, and the entry swap has a route. What simulation
cannot cover: the swap and the deposit are two separate transactions, so a
live entry can leave the wallet holding the token if the deposit then fails.
`acquire_base_token` refuses to deposit blind when the SOL leaves and no
balance appears, but the recovery itself is manual.

Before this runs live it still needs one real small-size entry (~0.05 SOL) to
exercise the swap leg, and step 3.

## Background worth reading

`docs/wp-to-commons-migration-scope.md` — why Token-2022 pools go through the
commons path and wp cannot handle them.
