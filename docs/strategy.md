# Meridian strategy

What the bot does, why each number is what it is, and what the numbers have
actually shown. Everything below is measured on this wallet unless marked
otherwise — where a setting rests on a guess rather than data, it says so.

Baseline for the current measurement run: **2.0974 SOL**
(`management.baselineSol`). Judge the strategy against the wallet, never
against the bot's own bookkeeping.

---

## 1. The trade

Single-side SOL liquidity on Meteora DLMM, memecoin–SOL pools.

The bot places **only SOL**, in bins **below** the current price. Nothing is
placed above. As price falls into the range, that SOL is progressively bought
into the token — so the position is mechanically a **dip-buyer** that gets paid
fees for the service. It profits when price falls into the range and recovers,
and loses when the token keeps falling after conversion.

That framing matters more than any parameter here: the strategy is not
directionally neutral. It is long the token from the moment price enters the
range.

**Range:** `bins_below = round(minBinsBelow + (volatility/5) × (maxBinsBelow −
minBinsBelow))`, clamped. In practice any pool with volatility ≥ 5 pins to the
maximum, and nearly all memecoin pools do, so `maxBinsBelow` is effectively the
only setting that matters. At 48 bins and bin_step 100 that covers about 38%
below entry.

---

## 2. Screening — which pools qualify

All filters are pushed into the Meteora pool-discovery query, so the local pass
rate is not meaningful; the API has already done the work.

| Setting | Value | Why |
|---|---|---|
| `timeframe` | `4h` | The binding constraint on universe size. At `1h` only ~18 pools qualify across all of Meteora, which is what forced the bot to recycle 24 tokens across 105 positions. `4h` roughly doubles it and favours sustained activity over a one-hour spike that is dead by deploy time. |
| `minFeeActiveTvlRatio` | `0.8` | Scaled to the 4h window. At the old `0.25` on `1h` the filter never rejected anything — observed pools ran 11–30× the threshold. |
| `minVolume` | `15000` | Same rescaling. |
| `minTvl` / `maxTvl` | `12000` / `500000` | Below the floor there is nothing to earn; above the ceiling our 0.5 SOL is too thin a share of the active bin to collect meaningful fees. |
| `minHolders` | `180` | Rug proxy. |
| `minMcap` / `maxMcap` | `20000` / `20000000` | Rug proxy at the bottom, dead-momentum proxy at the top. |
| `minBinStep` / `maxBinStep` | `20` / `250` | Below 20 the range is too tight to survive memecoin volatility; above 250 each bin is a large price jump. |
| `minTokenAgeHours` | `6` | Rug proxy, and the most expensive filter we run — it excludes the first hours where fee flow is heaviest. A deliberate risk trade, not a tuned number. |
| `maxBundlersPct` / `maxTop10Pct` | `20` / `42` | Concentration limits. |
| quote must be **wSOL** | enforced locally | The API has no quote filter. A USDC-quoted pool passes every check, reaches deploy, fails with "SOL-quoted pools only", and is picked again next cycle — one such pool burned 25 consecutive attempts. |

**Known limitation.** The funnel holds roughly 10 distinct tokens at a time,
often several pools of the same token at different bin steps. Cooldowns bite
into that quickly. When the bot idles, this is usually why, and idling is the
correct behaviour — an idle slot costs nothing, forcing entry into a known
bleeder costs about 0.13 SOL per 14 entries.

---

## 3. Sizing

| Setting | Value |
|---|---|
| `deployAmountSol` / `maxDeployAmount` | `0.5` |
| `maxPositions` | `3` |
| `gasReserve` | `0.15` |

Real all-in cost per position is **0.5574 SOL** — 0.5 of liquidity plus ~0.0574
of rent and fees, measured as the median of 104 deploy transactions. Rent
returns on close.

Three slots is arithmetic, not preference: at ~2.1 SOL the fourth is
impossible, and `compute_deploy_amount` refuses it once the gas reserve cannot
be preserved. Setting it to 4 anyway just burns a screening cycle per minute on
a slot that can never fill.

Prefer **fewer, larger** positions over more, smaller ones. Fee income depends
on our share of the *active bin*, so splitting the same capital across more
positions thins every one of them. Roughly a quarter of all positions have
earned zero fees; more slots would add to that count.

---

## 4. Exit ladder

Rules are evaluated on the live PnL poll (`pnlPollIntervalSecs: 15`).

| Exit | Setting | Measured result |
|---|---|---|
| **Over-extended TP** | `%B ≥ 1.0` (RSI disabled at `101`) | **Best exit we have.** Captures the peak almost exactly — 11.5%→11.4%, 13.5%→13.45%. Largest net contributor. |
| **Trailing TP** | arm at `+6%`, sell on `−3%` from peak | Works, but gives back more than the 3% it promises: observed 3.0–9.3pp from peak, ~5pp average. The threshold is not the problem; price passes through it between polls. |
| **Hard TP** | `+25%` | Effectively decorative — one position in all of history has reached it. |
| **OOR in profit** | out of range and `≥ exitMinProfitPct (2%)` | Net positive across many closes. Out-of-range *above* means the capital is idle, so banking is right. |
| **Stop loss** | `−6%` | The only exit path that loses money. Discussed below. |
| **Low yield** | `minFeePerTvl24h`, checked after `minAgeBeforeYieldCheck: 30` min | Rarely reached — positions usually go OOR first. |

### On the stop-loss

Across sessions the stop-loss accounts for essentially all realised losses
while every other path is net positive. It is tempting to read that as "the
stop-loss is the problem" and widen or remove it. **That reading is wrong**, and
it was made and retracted during development: the stop-loss is simply where
losses get *realised*. Removing it does not remove the loss, it relocates it —
MANLET went from +22% to −40% precisely because it could not be cut.

The threshold stays at −6% until there is evidence about what happens to those
tokens *after* a cut, which we do not have.

### Trigger vs settled

Every close now records both the reading the exit fired on and what the
position actually settled at (`settled_pnl_pct`, read back from Meteora's
`status=closed` endpoint). The brief prints the running drift.

This exists because a Tanisha-SOL stop-loss triggered at −7.04% and settled at
−3.35% — a position cut for breaching a threshold it never reached. The
suspicion was that trigger readings were systematically pessimistic and every
threshold was therefore being applied to the wrong number.

**Measured over 22 samples: average drift +0.13pp.** Scattered both directions,
most near zero. The readings are accurate; the outliers are genuine price
movement in the seconds around a close. The hypothesis is dead, and the
instrument stays because it is what killed it.

---

## 5. Guards against re-entry

The failure mode this protects against: the bot returning again and again to a
token that has already cut it. STONK was entered 14 times, KIO 8, MANLET 7.
Across 105 closes, **94% of positions were repeat entries**, and those repeats
carried the entire drawdown (−0.3265 SOL) while single entries were net
positive (+0.0423 SOL).

| Guard | Setting | Notes |
|---|---|---|
| Loss cooldown | `cooldownLossPct −5%`, `cooldownDurationMin 60` | Routine. Fires on any risk-cut close, or on a loss past the threshold. Falls back to `pnl_sol` when `pnl_pct` is unavailable (permanent for some Token-2022 positions). |
| **Repeat-loss** | `REPEAT_LOSS_TRIGGER 2`, `REPEAT_LOSS_THRESHOLD_PCT −1.0`, `REPEAT_LOSS_COOLDOWN_HOURS 4` | Token-level, across every pool of that mint. |
| Repeated OOR | `oorCooldownTriggerCount 4`, `oorCooldownHours 8` | For pools that keep leaving range without losing materially. |
| Repeat deploy | `repeatDeployCooldownTriggerCount 3`, `8h` | Rotation rule for tokens that *worked*, not loss protection. |

**Trigger of 2** is measured, not chosen: across 105 closes, entries taken once
a token already had two losing closes returned −1.64% on average over 9 tries;
after three losses, −2.40% over 5. Both buckets lose net even though nearly half
win individually.

**Four hours** is a deliberate weakening of a 24-hour lock. The guard was
starving the funnel — seven tokens locked at once out of a ten-token universe.
Four hours still breaks a same-session re-entry streak, which is the pattern
that costs money.

Two bugs made this guard invisible for a long time, both worth knowing about:

1. Cooldown setters overwrote `cooldown_until` unconditionally. The repeat-loss
   guard armed a 24-hour lock inside `record_deploy`, then the routine
   60-minute `loss_close` a few lines later silently shortened it. The guard
   fired every time and was erased every time. Cooldowns can now be extended,
   never cut short.
2. `is_fee_generating_deploy` needs a non-zero fee figure, and the fields it
   read were populated on 3 of 105 closes. `record_claim` only ever recorded
   the SOL leg of a claim, and on single-side SOL most fee income arrives as
   base token. Pool memory now reads `all_time_fees_usd` instead.

---

## 6. Costs — the part that decides everything

Measured across 1,186 wallet transactions:

| | |
|---|---|
| Gas, all transactions | **0.0168 SOL** — negligible |
| Rent held in leftover ATAs | 0.0041 SOL — negligible |
| Failed transactions | 47, costing 0.0003 SOL |
| **Swap slippage on exit** | **~0.093 SOL**, roughly a quarter of the drawdown |

**Every position exits through a Jupiter swap.** Not one close returns SOL
directly — the withdrawal comes back as base token, which is then sold. So each
round trip pays slippage on the way out, and slippage is set at
`100 / 200 / 300 bps` escalating across three attempts: take the tight price
when the market allows, widen only for tokens that refuse to fill, never leave a
token unswapped.

The number that matters: over a 26-close stretch, position PnL was **+0.1174
SOL** while the wallet moved **−0.0148 SOL**. About **1% per round trip**
disappears into cost.

That is the central problem. The strategy has a gross edge; the edge is
currently smaller than the cost of collecting it. **The lever is fewer
rotations, not more trades.** Anything that increases turnover — a
minutes-long scalping variant, more slots, tighter ranges — has to clear that
1% before it earns anything.

---

## 7. Accounting — read the wallet

Meteora's `pnl` **already includes fees**. Confirmed against a GOBLIN-SOL close:
deposit $36.91, withdraw $33.20, fees $1.42, reported PnL −$2.30, which is
exactly −3.71 + 1.42.

Adding `all_time_fees_usd` on top of `pnl_usd` therefore double-counts, and it
is why a session scored at +$40 left the wallet up about $2. The fee figure
itself is accurate — the bot read $1.3533 against Meteora's $1.42. It is worth
printing next to a pool for ranking, never summed into a result.

The brief leads with wallet balance against `baselineSol` for this reason. Every
other number on the page is the bot describing itself; that one is the chain's
answer.

---

## 8. Infrastructure

**RPC.** A dead RPC does not degrade this bot, it blinds it. Quota exhaustion
once left the poller detecting exits it could not execute for seven hours; one
position drifted +22% → −40% while its close was retried 505 times, and nothing
reached the operator. A heartbeat now probes `getHealth` every 30s, and three
consecutive failures send one Telegram alert and rotate to the next endpoint.
Endpoints come from `RPC_FALLBACK_URLS`, primary first.

**Token-2022** pools deploy, claim and close through the `commons` path, with
automatic fallback when the wp path hits `AccountNotInitialized`. All three
legs are proven live.

**Trading pause** persists to `trading-enabled.json` and is restored at boot. A
`/stop` that evaporates on restart is worse than no pause — the operator
believes the bot is halted while it opens positions unattended.

**Telegram** skips the update backlog at startup. Polling from offset 0 replays
up to 24 hours of unconfirmed updates, which once re-executed a day-old `/stop`
seconds after a deliberate start.

**Pool memory cannot be edited by CLI while the bot runs.** The process holds it
in memory and overwrites the file on its next save; `pool-memory clear-cooldown`
reports success and changes nothing. Stop the bot first.

---

## 9. Quickflip — the untested variant

`quickflip` is a separate, isolated scalping mode: volume-spike entry, ~10
minute maximum hold, 0.05 SOL per position, exits when volume fades by half.
It is enabled and has **never opened a position** — `minVolPerMin: 30000`
demands $1.8M/hour, and observed candidates top out around $570k.

It corresponds closely to the publicly circulated "tight range / quick in and
out" approach, which prescribes 1–3% ranges and 1–5 minute holds. Before
running it seriously, note that it multiplies turnover — and turnover is
exactly what the 1% round-trip cost punishes. It is a cheap experiment at 0.05
SOL, not a replacement for the main strategy.

---

## 10. Open questions

- **Does the 1% round-trip cost come down?** Escalating slippage is in place but
  unproven at scale. If it does not, turnover has to fall.
- **What happens to a token after a stop-loss?** Without this we cannot judge
  whether −6% is too tight, too loose, or right.
- **Is `minTokenAgeHours: 6` worth what it costs?** It excludes the heaviest fee
  flow. Lowering it trades rug exposure for opportunity, and nothing here
  measures that trade.
- **Does the four-hour repeat-loss lock hold the line?** It was shortened from 24
  to keep the funnel alive; whether it still deters re-entry is unmeasured.
