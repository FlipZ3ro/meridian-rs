# PVP / rival-pool risk detection

Meridian treats exact-symbol token conflicts as a major screening risk. A candidate is flagged when another active candidate uses the same normalized base symbol but a different mint, and that rival has meaningful traction:

- TVL >= 5,000 USD
- holders >= 500
- global fees >= 30 SOL

The Rust port adds this as a deterministic no-network screening policy after normal candidate filtering and scoring. It preserves JS-style candidate metadata for the screener LLM:

```json
{
  "is_pvp": true,
  "pvp_risk": "high",
  "pvp_symbol": "MOON",
  "pvp_rival_name": "MOON/SOL",
  "pvp_rival_mint": "...",
  "pvp_rival_pool": "...",
  "pvp_rival_tvl": 15000,
  "pvp_rival_holders": 800,
  "pvp_rival_fees": 35
}
```

## Config

PVP warning mode is enabled by default, matching the original Node.js behavior where conflicts are shown to the agent as a major negative:

```json
{
  "screening": {
    "avoidPvpSymbols": true,
    "blockPvpSymbols": false
  }
}
```

Set `blockPvpSymbols` to `true` when you want PVP conflicts hard-filtered before candidates reach the LLM:

```json
{
  "screening": {
    "avoidPvpSymbols": true,
    "blockPvpSymbols": true
  }
}
```

The original flat `user-config.json` keys are also mapped:

```json
{
  "avoidPvpSymbols": true,
  "blockPvpSymbols": false
}
```

## Operational behavior

- `avoidPvpSymbols = false`: no PVP metadata is added.
- `avoidPvpSymbols = true`, `blockPvpSymbols = false`: candidates remain visible but include `is_pvp=true` / `pvp_risk=high` metadata.
- `avoidPvpSymbols = true`, `blockPvpSymbols = true`: flagged candidates are removed from the final candidate list.

This implementation is deliberately offline/deterministic for the Rust screening path. It detects exact-symbol rival pools already present in the screened candidate set and keeps candidate JSON stable for prompt consumption and future external token enrichment.
