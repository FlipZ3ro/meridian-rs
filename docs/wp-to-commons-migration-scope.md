# Scope: migrate `meteora_native.rs` from wp-solana → official MeteoraAg `commons`

Read-only scoping. No code changed. Goal: replace the third-party
`wp-solana-meteora-dlmm-*` (waterpump, motosan-dev) transaction layer with
Meteora's own Rust building blocks (`MeteoraAg/dlmm-sdk`, git) for auditability.

## Current dependency footprint

All wp usage lives in **one file**: `backend/src/tools/meteora_native.rs`
(8 import lines, 4 public entry points). Everything else in the bot calls these
4 functions — so the blast radius is contained to this file's tx-building layer.

### 4 entry points that touch wp

| Fn | wp call | What wp does under the hood |
|----|---------|------------------------------|
| `deploy_position` | `add_liquidity_one_shot(AddLiquidityParams{ new_position, amount_y, active_id, strategy_parameters, slippage })` | init position + init bin array(s) + create ATAs + wrap wSOL + `add_liquidity_by_strategy` → 1 tx |
| `close_position` | `close_position_one_shot(ClosePositionParams)` (+ our own `unwrap_wsol` after) | remove 100% liquidity + claim fee + close position → 1 tx |
| `claim_fees` | `claim_fee_one_shot(ClaimFeeParams)` | claim fee → 1 tx |
| `quote_position_state` | `fetch_close_position_snapshot` + `plan_close_position` (read-only) | fetch position + bin arrays, run quote math → liquidity + pending fees |

### Shared wp types used across the file
- `LbPair::from_bytes` — decode pool account (deploy active_id, base_mint, discovery). 4 sites.
- `StrategyParameters` / `StrategyType` — generated Anchor types.
- `RpcContext` — wp RPC wrapper.
- `WorkspacePlanConfig` — wp plan config (defaults).
- `wp_solana_core::token` — wSOL/token handling.

## Official `commons` surface (MeteoraAg/dlmm-sdk @ git)

`commons/src`: `pda.rs`, `quote.rs` (**34KB — the quote math**), `math/`,
`token_2022.rs`, `extensions/`, `conversions/`, `seeds.rs`, `constants.rs`,
`account_filters.rs`, `rpc_client_extension.rs`, `typedefs.rs`.

**Key gap:** `commons` is LOW-LEVEL (PDAs, quote math, token2022). It has **no
high-level `*_one_shot` orchestration**. The one-shots must be re-assembled
ourselves, using the official reference **`market_making/core.rs` (34KB)** which
already wires: `initialize_position`, bin-array init (`bin_array_manager.rs`),
`add_liquidity_by_strategy`, `remove_liquidity`, `claim_fee`, `close_position`.

## Mapping

| wp | Official replacement | Effort |
|----|----------------------|--------|
| `LbPair::from_bytes`, `StrategyParameters/Type` | generated `lb_clmm` program types (from dlmm-sdk) | 🟢 direct swap |
| `RpcContext` | plain `RpcClient` + `commons::rpc_client_extension` | 🟢 direct |
| `quote` (`fetch_close_position_snapshot`+`plan_close_position`) | `commons::quote` + fetch bin arrays | 🟡 medium — math EXISTS in commons |
| `claim_fee_one_shot` | generated `claim_fee` ix (single) | 🟡 medium |
| `close_position_one_shot` | assemble `remove_liquidity(100%)` + `claim_fee` + `close_position` (per market_making/core.rs) | 🟠 high |
| `add_liquidity_one_shot` | assemble init-position + bin-array init + `add_liquidity_by_strategy` + wSOL wrap (per market_making/core.rs) | 🔴 highest — the money path |
| `wp_solana_core` wSOL / `WorkspacePlanConfig` | `commons::token_2022` + manual wSOL (we already `unwrap_wsol` ourselves) | 🟡 medium |

## Risks

1. **Dependency-version conflict (biggest unknown).** `commons`/`lb_clmm` pin
   their own anchor/solana versions; our tree already juggles `solana-sdk` v2 +
   v3. A git dep that won't co-resolve could block the whole thing cheaply-or-not.
   → **Must be tested FIRST (Phase 0) before any porting.**
2. **`add_liquidity_by_strategy` correctness** — bin-array init, strategy→amount
   distribution, slippage. wp hides it; we'd own it. This is the real-money path.
3. **Regression on a live, funded bot.** wp works today. Any parity gap = lost SOL.
4. **git dep, not crates.io** — must pin a commit; supply-chain trust shifts from
   "small third-party crate" to "Meteora repo @ pinned commit" (net better, but
   still not a registry release).

## Recommended phasing (branch, dry-run parity, no rush)

- **Phase 0 — build spike ✅ DONE (PASSED):** branch `spike/commons-migration`.
  Added `commons` (git, `MeteoraAg/dlmm-sdk` @ `fb02e51a`, v0.3.3, anchor 0.31.1).
  Dependency resolution CLEAN (56 pkgs locked, no conflict — commons' solana 2.1
  caret unified to our 2.3.0) and it COMPILES alongside wp + solana 2.3/3.1 in one
  build (`cargo check` 53s, exit 0). Biggest risk cleared. No separate `lb_clmm`
  crate needed — program bindings live inside `commons`. For Phase 1+, pin
  `rev = "fb02e51a"` instead of `branch = "main"`.
- **Phase 1 — `quote_position_state`:** read-only, zero money risk; validates the
  dep + quote parity vs wp on live positions.
- **Phase 2 — `claim_fees`:** single ix, low risk.
- **Phase 3 — `close_position`:** must return principal correctly; devnet + tiny live.
- **Phase 4 — `deploy_position`:** highest; devnet + tiny-size live parity vs wp
  before cutover. Keep wp behind a feature flag until proven.

## Verdict
No version *update* exists to grab (wp is on latest 0.1.2; no official crate on
crates.io). This is a **migration for auditability**, not a bump. It's contained
to one file but the deploy path is real-money-critical. **Do Phase 0 first** —
the dependency-resolution result determines whether this is a ½-day swap or a
multi-day rewrite. Recommended: not urgent; execute deliberately on a branch.
