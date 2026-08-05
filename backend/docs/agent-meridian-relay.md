# Agent Meridian / LPAgent relay status

The original Node.js Meridian project contains two Agent Meridian / LPAgent integration classes:

1. **Mutable execution relay** — optional zap-in/zap-out order routes such as `/execution/zap-in/order`, `/execution/zap-in/submit`, and related transaction-signing helpers.
2. **Read-only LPAgent enrichment** — cached owner/top-LPer endpoints such as `/top-lp/:pool`, `/study-top-lp/:pool`, plus raw-position enrichment in the legacy JS flow.

## Rust replacement decision

The Rust port intentionally uses native local execution for mutable trading paths instead of making Agent Meridian the primary transaction submitter:

| Capability | Rust path | Why |
| --- | --- | --- |
| Deploy/open DLMM position | Native Meteora SDK adapter (`tools::meteora_native` through `tools::dlmm`) | Keeps wallet signing local and avoids relay-only execution semantics. |
| Claim/close DLMM position | Native Meteora SDK adapter | Mirrors the verified Rust transaction path and preserves dry-run guardrails. |
| Swap / auto-swap to SOL | Native Jupiter signing/submission (`tools::wallet`) | Uses local keypair signing and the configured Jupiter API key/referral settings. |
| Zap-in / zap-out relay order signing | `tools::agent_meridian::{sign_zap_in_order, sign_zap_out_order}` | Kept as a compatibility adapter for order payloads and regression tests. |
| LPAgent top-LPer study | Agent Meridian read-only `/top-lp/:pool` and `/study-top-lp/:pool` | Server-side cached analytics remain useful before screening/deploying. |

This means **`lpAgentRelayEnabled` is accepted for config compatibility, but the verified Rust default is `false` and mutable deploy/claim/close/swap execution is native**. If a future user explicitly needs live relay submission again, it should be reintroduced behind dry-run-first tests for order creation, local signing, submission, and post-submit position refresh.

## Runtime configuration

Supported original JS-compatible keys:

- `agentMeridianApiUrl` / `AGENT_MERIDIAN_API_URL`
- `publicApiKey` / `PUBLIC_API_KEY`
- `LPAGENT_API_KEY` as an Agent Meridian API-key alias
- `lpAgentRelayEnabled` in `user-config.json` for compatibility/documentation

## Verification rule

The README checkbox for “Agent Meridian / LPAgent relay support or documented replacement” should stay checked only while these guarantees are true:

- Native Meteora deploy/claim/close tests pass.
- Jupiter swap signing/submission tests pass.
- Agent Meridian zap-in/zap-out order-signing adapter tests pass.
- LPAgent top-LPer read-only study tests pass.
- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, and `cargo build` pass.
