# Discord signal queue and pre-check pipeline

The original Node.js Meridian project used a Discord selfbot listener to watch configured LP Army channels, extract Solana addresses, run pre-checks, and write accepted opportunities to `discord-signals.json`.

The Rust port keeps the trading/runtime side safe and local:

- Discord signals are stored in the runtime data directory as `discord-signals.json` (`MERIDIAN_DATA_DIR` / state-file directory), not in the repo root.
- The CLI can inspect, clear, and manually/import-queue signals without network access:
  - `meridian discord-signals`
  - `meridian discord-signals clear`
  - `meridian discord-signals queue --pool <pool> --base-mint <mint> [--symbol <sym>]`
- The pre-check pipeline has Rust-native deterministic stages for:
  - 10-minute deduplication,
  - local token blacklist rejection,
  - pool-resolution pass/fail result handling,
  - global-fee threshold checks.
- The screening pipeline honors `screening.useDiscordSignals` and merges pending local queue entries into pool discovery. `discordSignalMode = "only"` restricts screening to signal pools; any other value uses merge mode.

## Queue schema

`discord-signals.json` remains a JSON array for JS compatibility. Each record uses the original snake_case shape:

```json
{
  "id": "Pool111-1780000000000",
  "pool_address": "Pool111...",
  "base_mint": "Mint111...",
  "base_symbol": "TOKEN",
  "signal_source": "discord",
  "discord_guild": "LP Army",
  "discord_channel": "alpha",
  "discord_author": "Metlex Pool Bot",
  "discord_message_snippet": "message excerpt",
  "queued_at": "2026-06-09T00:00:00Z",
  "rug_score": 42,
  "total_fees_sol": 31,
  "token_age_minutes": 15,
  "status": "pending"
}
```

If an importer/listener has a fresh pool-discovery snapshot, it may add `discovery_pool`; Rust will preserve it and use it when merging signal candidates into screening.

## Config

Nested Rust config:

```json
{
  "screening": {
    "useDiscordSignals": true,
    "discordSignalMode": "merge"
  }
}
```

Original JS flat `user-config.json` compatibility is also supported:

```json
{
  "useDiscordSignals": true,
  "discordSignalMode": "only"
}
```

## Operational note

The Rust port does **not** require committing or storing Discord tokens. If a live listener/importer is used, keep values in `.env` only:

- `DISCORD_USER_TOKEN`
- `DISCORD_GUILD_ID`
- `DISCORD_CHANNEL_IDS`
- `DISCORD_MIN_FEES_SOL`

Do not commit real token values.
