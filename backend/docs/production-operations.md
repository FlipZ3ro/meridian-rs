# Meridian RS Production Operations

This guide is the Rust-native replacement for the original Node.js production control surface and operational runbooks. It covers environment handling, startup checks, duplicate process guards, deployment options, and replacements for tools that were previously handled through Telegram, Claude Code slash commands, or HiveMind-style shared memory.

## Environment and secret handling

Start from the trackable template:

```bash
cp .env.example .env
# or for a host-level runtime profile:
mkdir -p ~/.meridian
cp .env.example ~/.meridian/.env
```

Keep `DRY_RUN=true` until wallet signing, RPC, LLM, Web UI, and smoke checks are verified. Fill the setup-critical keys first:

- `WALLET_PRIVATE_KEY` or `MERIDIAN_WALLET_PRIVATE_KEY` for Solana transaction signing.
- `MERIDIAN_WALLET` for wallet reads.
- `RPC_URL` or `HELIUS_RPC_URL`, plus `HELIUS_API_KEY` when using Helius.
- `LLM_BASE_URL` plus `OPENROUTER_API_KEY` or `LLM_API_KEY`.
- `LLM_MODEL` or per-cycle `MANAGEMENT_MODEL`, `SCREENING_MODEL`, `GENERAL_MODEL`.
- `MERIDIAN_DATA_DIR`, `MERIDIAN_STATE_PATH`, `MERIDIAN_WEB_ADDR`, `HEALTH_PORT`, and `MERIDIAN_LOCK_PATH` for production runtime placement.

### Encrypted env flow or documented alternative

Do not commit real `.env` files. The repo intentionally ignores `.env` and `.env.*`, while keeping `!.env.example` trackable.

Recommended encrypted alternatives:

1. **1Password CLI** for operator workstations:
   ```bash
   op run --env-file ~/.meridian/.env -- ./target/release/meridian-rs
   ```
   Store the real values in 1Password and render only for the process environment.
2. **sops + age** for server deployments:
   ```bash
   sops -d ops/meridian.env.sops > ~/.meridian/.env
   chmod 600 ~/.meridian/.env
   ./target/release/meridian-rs
   ```
3. **Host secret manager** as an alternative: systemd `EnvironmentFile=`, launchd `EnvironmentVariables`, or a managed vault can inject the same keys from `.env.example` without writing secrets into git.

## Startup checks

The long-running runtime performs startup checks and logs warnings for missing production-critical inputs:

- repo/cwd/config source (`MERIDIAN_CONFIG_PATH`, fallback config behavior)
- wallet private key (`WALLET_PRIVATE_KEY` or `MERIDIAN_WALLET_PRIVATE_KEY`)
- wallet read address (`MERIDIAN_WALLET`)
- LLM API key (`LLM_API_KEY` or `OPENROUTER_API_KEY`)
- RPC URL (`RPC_URL` or `HELIUS_RPC_URL`)
- state path parent directory

Warnings do not stop dry-run development, but production live execution should not proceed until all required checks are green.

## Duplicate process and port conflict guards

Duplicate process protection uses `MERIDIAN_LOCK_PATH`; when unset it defaults to `~/.meridian/meridian.lock`. The runtime creates this file on startup and removes it on clean shutdown. If a second process starts while the lock points at a live PID, startup fails with a duplicate process warning.

Port conflict checks cover:

- Web UI: `MERIDIAN_WEB_ADDR` (default `0.0.0.0:3000`)
- Health endpoint: `HEALTH_PORT` (default `8080`)

If a port is already bound, the startup report logs a Port conflict warning before the runtime tries to bind services.

## launchd deployment guide

Example macOS `launchd` service (`~/Library/LaunchAgents/com.meridian.rs.plist`):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.meridian.rs</string>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/USER/path/to/meridian-rs/target/release/meridian-rs</string>
  </array>
  <key>WorkingDirectory</key><string>/Users/USER/path/to/meridian-rs</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>MERIDIAN_DATA_DIR</key><string>/Users/USER/.meridian</string>
    <key>MERIDIAN_WEB_ADDR</key><string>127.0.0.1:3000</string>
    <key>HEALTH_PORT</key><string>8080</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/Users/USER/.meridian/meridian.out.log</string>
  <key>StandardErrorPath</key><string>/Users/USER/.meridian/meridian.err.log</string>
</dict>
</plist>
```

Load it with:

```bash
launchctl load ~/Library/LaunchAgents/com.meridian.rs.plist
launchctl kickstart gui/$(id -u)/com.meridian.rs
```

## systemd deployment guide

Example Linux unit (`/etc/systemd/system/meridian-rs.service`):

```ini
[Unit]
Description=Meridian RS DLMM Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=/opt/meridian-rs
ExecStart=/opt/meridian-rs/target/release/meridian-rs
EnvironmentFile=/etc/meridian-rs/meridian.env
Restart=always
RestartSec=10
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

Enable it with:

```bash
systemctl daemon-reload
systemctl enable --now meridian-rs
systemctl status meridian-rs
```

## PM2-equivalent deployment guide

Rust does not need PM2, but PM2 can supervise the compiled binary if the host already standardizes on PM2:

```bash
pm2 start ./target/release/meridian-rs --name meridian-rs --cwd /opt/meridian-rs
pm2 save
pm2 logs meridian-rs
```

The Rust-native PM2-equivalent is a compiled binary supervised by `launchd` or `systemd` with `MERIDIAN_LOCK_PATH` and health checks enabled.

## Claude Code slash-command replacement

Claude Code slash-command compatibility is intentionally replaced by Rust-native CLI and Web UI controls:

- `/screen` -> `meridian screen --wallet <wallet> --wallet-sol <amount>` or Web UI **Run Screen**.
- `/manage` -> `meridian manage --wallet <wallet>` or Web UI **Run Manage**.
- `/deploy` -> `meridian deploy --pool <pool> --amount <sol> --dry-run`.
- `/claim` -> `meridian claim --position <position>`.
- `/close` -> `meridian close --position <position> --reason "manual" --skip-swap`.
- `/status` -> `meridian status` or `GET /api/status`.

This preserves operator intent while avoiding a dependency on Claude Code-specific slash-command plumbing.

## HiveMind/shared lessons replacement

HiveMind/shared lessons support is replaced by the data-dir-isolated Rust lesson and performance stores:

- `~/.meridian/lessons.json` stores lessons, pinned memories, and performance outcomes.
- `meridian lessons list|add|pin|unpin|prompt` exposes the store through the CLI.
- `GET /api/lessons` and `GET /api/performance` expose local Web UI views.
- Sharing across hosts is a documented operational choice: sync `MERIDIAN_DATA_DIR` through a secure file-sync/Vault workflow, not through implicit global state.

This is the HiveMind/shared lessons replacement for the Rust port: explicit local JSON state plus optional operator-managed synchronization.

## Verification checklist

Before marking production operations complete:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build
```

Then smoke-check the Web UI and health endpoint with temporary state paths before switching `DRY_RUN=false`.
