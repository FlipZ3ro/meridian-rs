# Meridian RS

**Meridian RS** is a Rust rewrite of the autonomous DLMM liquidity provider agent for Meteora on Solana.

## Features

- Config system with LLM integration
- Solana wallet connection
- DLMM position management (deploy/close)
- Screening engine
- ReAct-style agent loop with LLM
- Management & Screening cycles
- Web UI (Terminal + Cycle Log)

## Current Status

This project is still in early development. Core modules are implemented, but full parity with the original Node.js Meridian agent is being built phase-by-phase. Checked items are already implemented and verified; unchecked items are the active backlog.

## JS Meridian Parity Roadmap

Reference target: [`yunus-0x/meridian`](https://github.com/yunus-0x/meridian)

### Phase 0 — Baseline Rust skeleton and verification

- [x] Rust project boots with `cargo run`
- [x] Config loader with sane defaults
- [x] LLM client and ReAct-style loop skeleton
- [x] Screening and management cycle modules exist
- [x] Basic Web UI and health/status endpoints exist
- [x] Baseline quality gates pass: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`

### Phase 1 — Runtime/config compatibility foundation

- [x] Nested Rust `user-config.json` format loads successfully
- [x] Missing nested `strategy` defaults correctly for older Rust configs
- [x] Original JS flat `user-config.json` format loads successfully
- [x] Original `.env` keys map into Rust runtime consistently
- [x] Dry-run mode is first-class and blocks all transaction submission
- [x] Runtime state files are isolated and documented

### Phase 2 — Core trading parity

- [x] Wallet private key loading and Solana transaction signing
- [x] Rust base64 transaction signer supports versioned and legacy Solana transactions
- [x] Real Meteora DLMM deploy position flow
- [x] Real claim fees flow
- [x] Real close position flow
- [x] Real close + optional swap-to-SOL flow
- [x] Jupiter swap signing/submission parity
- [x] Agent Meridian relay transaction signing adapter for zap-in/zap-out order responses
- [x] Meteora Rust SDK compatibility spike validates native claim/close/deploy adapter path
- [ ] Agent Meridian / LPAgent relay support or documented replacement
- [x] Regression tests for dry-run vs live execution guardrails

### Phase 3 — CLI and setup parity

- [x] `meridian` CLI using Rust subcommands
- [x] `setup` wizard generating `.env` and `user-config.json`
- [x] JSON output parity for `balance`, `positions`, `pnl`, `candidates`, `deploy`, `claim`, `close`, `swap`
- [x] One-shot `screen` and `manage` commands
- [x] `config get/set`
- [x] `lessons`, `performance`, `evolve`, `pool-memory`, `blacklist` commands

### Phase 4 — Agent intelligence parity

- [x] Structured `decision-log.json`
- [x] `get_recent_decisions` tool and prompt injection
- [x] Rich lessons/performance history
- [x] Darwin signal weighting and threshold evolution
- [x] Strategy library with active strategy presets
- [x] Study top LPers / behavior-pattern analysis
- [x] Pool memory cooldown logic matching original behavior

### Phase 5 — Screening enrichment parity

- [ ] Discord signal queue and pre-check pipeline
- [ ] PVP/rival-pool risk detection
- [ ] Launchpad allow/block filters
- [ ] Timeframe-scaled screening thresholds
- [ ] Chart indicator presets for entry/exit confirmation
- [ ] Token audit, holder, smart-wallet, narrative enrichment parity
- [ ] Detailed reject reasons for filtered candidates

### Phase 6 — Control surface parity

- [ ] Web UI replaces Telegram control surface for local usage
- [ ] Live positions, balances, candidates, cycle logs, and decisions in Web UI
- [ ] Manual screen/manage/deploy/claim/close controls in Web UI
- [ ] Config editor in Web UI
- [ ] Lessons/performance/blacklist views in Web UI
- [ ] Optional Telegram notifications/commands if exact JS parity is required

### Phase 7 — Production operations parity

- [ ] `.env.example` parity with original project
- [ ] Encrypted env flow or documented alternative
- [ ] launchd/systemd/PM2-equivalent deployment guide
- [ ] Startup checks for repo/cwd/config/wallet/API keys
- [ ] Duplicate process and port conflict guards
- [ ] Claude Code slash-command compatibility or Rust-native replacement
- [ ] HiveMind/shared lessons support or documented replacement

## Project Structure

```
src/
├── main.rs
├── cycle.rs          # Management & Screening cycles
├── config/           # Config loader + types
├── tools/            # DLMM, Wallet, Screening, Executor
├── agent/            # ReAct Agent Loop
├── llm.rs            # OpenAI-compatible LLM client
├── state/            # Position tracking
├── web.rs            # Web UI (Terminal + Cycle Log)
└── utils/
```

## Run

```bash
cargo run
```

Web UI will be available at `http://localhost:3000`.

The same binary also supports Rust-native one-shot subcommands:

```bash
cargo run -- status
cargo run -- balance --wallet <wallet>
cargo run -- positions --wallet <wallet>
cargo run -- pnl --pool <pool> --position <position> --wallet <wallet>
cargo run -- candidates --limit 3
cargo run -- deploy --pool <pool> --amount <sol> --bins-below 35 --bins-above 0 --strategy spot --dry-run
cargo run -- claim --position <position>
cargo run -- close --position <position> --reason "low yield" --skip-swap
cargo run -- swap --from <mint> --amount <tokens>
```

Omitting a subcommand starts the long-running agent runtime; passing a subcommand prints JSON output and exits.

## Config

Copy `user-config.example.json` to `user-config.json` and modify as needed.

Copy `.env.example` to `.env` for local development or to `~/.meridian/.env` for the default runtime profile, then fill at least:

- `WALLET_PRIVATE_KEY` — base58 Solana keypair or Solana CLI JSON byte array
- `MERIDIAN_WALLET` — public wallet address used for reads
- `RPC_URL` or `HELIUS_RPC_URL` — Solana RPC endpoint
- `LLM_BASE_URL` plus `OPENROUTER_API_KEY` or `LLM_API_KEY` — OpenAI-compatible LLM access
- `LLM_MODEL`, or per-cycle `MANAGEMENT_MODEL`, `SCREENING_MODEL`, `GENERAL_MODEL`

The Rust port accepts both the nested Rust config format and the original Node.js Meridian flat `user-config.json` keys. Runtime secrets should live in `.env` or `~/.meridian/.env`; original Meridian env names such as `RPC_URL`, `OPENROUTER_API_KEY`, `LLM_MODEL`, `HELIUS_API_KEY`, `JUPITER_API_KEY`, and Telegram/Agent Meridian keys are mapped into the Rust config at startup.

**Never commit your real `user-config.json` or API keys.**

## Runtime State

By default, mutable runtime files are isolated under `~/.meridian/` instead of the repository root:

- `~/.meridian/meridian-state.json` — tracked positions and recent position events
- `~/.meridian/pool-memory.json` — pool notes, history, and cooldown memory
- `~/.meridian/.env` — optional global runtime environment file

Overrides:

- `MERIDIAN_DATA_DIR=/path/to/data` changes the directory for default state files.
- `MERIDIAN_STATE_PATH=/path/to/meridian-state.json` overrides only the position state file.
- Repo-local `.env` is still loaded for development runs.

## License

MIT

---

**Note**: This is a work in progress. Many features are still being implemented to match the original Node.js version.