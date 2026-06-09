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
- [x] Agent Meridian / LPAgent relay support or documented replacement
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

- [x] Discord signal queue and pre-check pipeline
- [x] PVP/rival-pool risk detection
- [x] Launchpad allow/block filters
- [x] Timeframe-scaled screening thresholds
- [x] Chart indicator presets for entry/exit confirmation
- [x] Token audit, holder, smart-wallet, narrative enrichment parity
- [x] Detailed reject reasons for filtered candidates

### Phase 6 — Control surface parity

- [x] Web UI replaces Telegram control surface for local usage
- [x] Live positions, balances, candidates, cycle logs, and decisions in Web UI
- [x] Manual screen/manage/deploy/claim/close controls in Web UI
- [x] Config editor in Web UI
- [x] Lessons/performance/blacklist views in Web UI
- [x] Optional Telegram notifications/commands intentionally deferred; Web UI is the supported replacement control surface

### Phase 7 — Production operations parity

- [x] `.env.example` parity with original project
- [x] Encrypted env flow or documented alternative
- [x] launchd/systemd/PM2-equivalent deployment guide
- [x] Startup checks for repo/cwd/config/wallet/API keys
- [x] Duplicate process and port conflict guards
- [x] Claude Code slash-command compatibility or Rust-native replacement
- [x] HiveMind/shared lessons support or documented replacement

### Phase 8 — Runtime operator polish

- [x] Interactive REPL `screen`/`manage` commands execute the real one-shot cycles and print JSON output
- [ ] PnL poller exit signals dispatch or queue close actions through the guarded close flow

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
cargo run -- discord-signals
cargo run -- discord-signals queue --pool <pool> --base-mint <mint> --symbol <symbol>
cargo run -- deploy --pool <pool> --amount <sol> --bins-below 35 --bins-above 0 --strategy spot --dry-run
cargo run -- claim --position <position>
cargo run -- close --position <position> --reason "low yield" --skip-swap
cargo run -- swap --from <mint> --amount <tokens>
```

Omitting a subcommand starts the long-running agent runtime; passing a subcommand prints JSON output and exits. In an interactive terminal, the runtime REPL supports `status`, `screen`, `manage`, and `quit`; manual `screen`/`manage` commands execute the same Rust one-shot cycle handlers and print their JSON envelopes.

## Config

Copy `user-config.example.json` to `user-config.json` and modify as needed.

Copy `.env.example` to `.env` for local development or to `~/.meridian/.env` for the default runtime profile, then fill at least:

- `WALLET_PRIVATE_KEY` — base58 Solana keypair or Solana CLI JSON byte array
- `MERIDIAN_WALLET` — public wallet address used for reads
- `RPC_URL` or `HELIUS_RPC_URL` — Solana RPC endpoint
- `LLM_BASE_URL` plus `OPENROUTER_API_KEY` or `LLM_API_KEY` — OpenAI-compatible LLM access
- `LLM_MODEL`, or per-cycle `MANAGEMENT_MODEL`, `SCREENING_MODEL`, `GENERAL_MODEL`

The Rust port accepts both the nested Rust config format and the original Node.js Meridian flat `user-config.json` keys. Runtime secrets should live in `.env` or `~/.meridian/.env`; original Meridian env names such as `RPC_URL`, `OPENROUTER_API_KEY`, `LLM_MODEL`, `HELIUS_API_KEY`, `JUPITER_API_KEY`, and Telegram/Agent Meridian keys are mapped into the Rust config at startup.

Agent Meridian / LPAgent mutable relay execution is documented as replaced by native Rust execution for deploy/claim/close/swap paths; read-only LPAgent analytics remain available for top-LPer study. See [`docs/agent-meridian-relay.md`](docs/agent-meridian-relay.md).

Production operations now include trackable env templates, startup checks, duplicate process guards, port conflict checks, encrypted-env alternatives, launchd/systemd/PM2-equivalent deployment examples, Claude Code slash-command replacements, and HiveMind/shared lessons replacement guidance. See [`docs/production-operations.md`](docs/production-operations.md).

Discord signal queue/pre-check parity is Rust-native and data-dir isolated through `discord-signals.json`; see [`docs/discord-signals.md`](docs/discord-signals.md).

PVP/rival-pool risk detection flags exact-symbol mint conflicts in screening candidates, and can optionally hard-block them with `screening.blockPvpSymbols`; see [`docs/pvp-risk.md`](docs/pvp-risk.md).

**Never commit your real `user-config.json` or API keys.**

## Runtime State

By default, mutable runtime files are isolated under `~/.meridian/` instead of the repository root:

- `~/.meridian/meridian-state.json` — tracked positions and recent position events
- `~/.meridian/pool-memory.json` — pool notes, history, and cooldown memory
- `~/.meridian/discord-signals.json` — pending/processed Discord signal queue for screening enrichment
- `~/.meridian/.env` — optional global runtime environment file

Overrides:

- `MERIDIAN_DATA_DIR=/path/to/data` changes the directory for default state files.
- `MERIDIAN_STATE_PATH=/path/to/meridian-state.json` overrides only the position state file.
- Repo-local `.env` is still loaded for development runs.

## License

MIT

---

**Note**: This is a work in progress. Many features are still being implemented to match the original Node.js version.