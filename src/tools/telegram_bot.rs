//! Interactive Telegram control for the Meridian bot.
//!
//! Long-polls `getUpdates`, authorizes the single admin chat, and dispatches
//! commands to the existing CLI command surface (`parse_cli_args` +
//! `run_cli_command`) so there is one source of truth for bot actions. The
//! `/start` and `/stop` commands flip a shared `trading_enabled` flag that the
//! screening cycle checks before deploying — pausing NEW deploys while still
//! managing/closing open positions. Admin-only; everyone else is rejected.

use crate::cli::{parse_cli_args, run_cli_command, CliOutput};
use crate::config::types::Config;
use crate::utils::logger::module::{info, warn};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const TG_API: &str = "https://api.telegram.org";
const MAX_TG_LEN: usize = 3800; // Telegram caps messages at 4096 chars

const HELP: &str = "🤖 *Meridian control*\n\
/status — agent state + open positions\n\
/positions — open positions detail\n\
/pnl — portfolio PnL (realized + unrealized)\n\
/balance — wallet SOL balance\n\
/candidates [n] — top screening candidates\n\
/start — resume trading (new deploys)\n\
/stop — pause new deploys (still manages open)\n\
/close <pool|position> — close a position\n\
/help — this message";

/// Spawned from `main`. Never returns; loops on getUpdates.
pub async fn run(config: Config, state_path: String, trading_enabled: Arc<AtomicBool>) {
    let token = match config
        .api
        .telegram_bot_token
        .clone()
        .filter(|s| !s.is_empty())
    {
        Some(t) => t,
        None => {
            info(
                "telegram",
                "interactive control disabled (no telegram_bot_token)",
            );
            return;
        }
    };
    let admin = match config.api.telegram_chat_id.clone().filter(|s| !s.is_empty()) {
        Some(c) => c,
        None => {
            info(
                "telegram",
                "interactive control disabled (no telegram_chat_id)",
            );
            return;
        }
    };

    let client = reqwest::Client::new();
    let _ =
        crate::tools::telegram::send_message_safe(&token, &admin, "🤖 Meridian control online — /help")
            .await;
    info("telegram", "interactive control online");

    let mut offset: i64 = 0;
    loop {
        match get_updates(&client, &token, offset).await {
            Ok(updates) => {
                for upd in updates {
                    let id = upd.get("update_id").and_then(Value::as_i64).unwrap_or(offset);
                    offset = id + 1;

                    let Some(msg) = upd.get("message") else {
                        continue;
                    };
                    let from_chat = msg
                        .get("chat")
                        .and_then(|c| c.get("id"))
                        .and_then(Value::as_i64)
                        .map(|i| i.to_string())
                        .unwrap_or_default();
                    let text = msg.get("text").and_then(Value::as_str).unwrap_or("");
                    if text.is_empty() {
                        continue;
                    }

                    if from_chat != admin {
                        warn("telegram", &format!("rejected non-admin chat {from_chat}"));
                        let _ = crate::tools::telegram::send_message_safe(
                            &token,
                            &from_chat,
                            "⛔ Unauthorized.",
                        )
                        .await;
                        continue;
                    }

                    let reply = handle(text, &config, &state_path, &trading_enabled).await;
                    let _ =
                        crate::tools::telegram::send_message_safe(&token, &admin, &reply).await;
                }
            }
            Err(e) => {
                warn("telegram", &format!("getUpdates error: {e}"));
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn get_updates(
    client: &reqwest::Client,
    token: &str,
    offset: i64,
) -> anyhow::Result<Vec<Value>> {
    // Long-poll (30s) so we react promptly without hammering the API.
    let url = format!("{TG_API}/bot{token}/getUpdates?timeout=30&offset={offset}");
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(40))
        .send()
        .await?;
    let body: Value = resp.json().await?;
    Ok(body
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

async fn handle(
    text: &str,
    config: &Config,
    state_path: &str,
    trading_enabled: &Arc<AtomicBool>,
) -> String {
    let mut it = text.trim().split_whitespace();
    let raw = it.next().unwrap_or("");
    // strip leading '/' and any '@botname' suffix
    let cmd = raw
        .trim_start_matches('/')
        .split('@')
        .next()
        .unwrap_or("")
        .to_lowercase();
    let rest: Vec<String> = it.map(|s| s.to_string()).collect();

    match cmd.as_str() {
        "" | "help" => HELP.to_string(),
        "start" => {
            trading_enabled.store(true, Ordering::SeqCst);
            "▶️ Trading ENABLED — bot will deploy on valid candidates.".to_string()
        }
        "stop" => {
            trading_enabled.store(false, Ordering::SeqCst);
            "⏸️ Trading PAUSED — no new deploys. Open positions still managed & closed.".to_string()
        }
        "pnl" => portfolio_text(config).await,
        "status" => {
            let flag = if trading_enabled.load(Ordering::SeqCst) {
                "▶️ Trading ENABLED"
            } else {
                "⏸️ Trading PAUSED"
            };
            match run_json("status", &[], config, state_path).await {
                Ok(v) => {
                    let summary = v
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .replace(" | ", "\n");
                    format!("{flag}\n\n{summary}")
                }
                Err(e) => format!("⚠️ {e}"),
            }
        }
        "balance" => match run_json("balance", &[], config, state_path).await {
            Ok(v) => fmt_balance(&v),
            Err(e) => format!("⚠️ {e}"),
        },
        "positions" => match run_json("positions", &[], config, state_path).await {
            Ok(v) => fmt_positions(&v),
            Err(e) => format!("⚠️ {e}"),
        },
        "candidates" => {
            let lim = rest.first().cloned().unwrap_or_else(|| "8".to_string());
            match run_json("candidates", &["--limit".to_string(), lim], config, state_path).await
            {
                Ok(v) => fmt_candidates(&v),
                Err(e) => format!("⚠️ {e}"),
            }
        }
        "close" => match rest.first() {
            Some(target) => {
                match run_json(
                    "close",
                    &["--position".to_string(), target.clone()],
                    config,
                    state_path,
                )
                .await
                {
                    Ok(v) => {
                        if v.get("success").and_then(Value::as_bool).unwrap_or(false) {
                            format!("✅ Close submitted for {}", short(target))
                        } else {
                            format!(
                                "⚠️ Close failed: {}",
                                v.get("error").and_then(Value::as_str).unwrap_or("unknown")
                            )
                        }
                    }
                    Err(e) => format!("⚠️ {e}"),
                }
            }
            None => "Usage: /close <pool_or_position_address>".to_string(),
        },
        other => format!("Unknown command: /{other}\n\n{HELP}"),
    }
}

/// Run a CLI command via the argv parser and return its raw JSON value.
async fn run_json(
    cmd: &str,
    tail: &[String],
    config: &Config,
    state_path: &str,
) -> Result<Value, String> {
    let mut args = vec!["meridian".to_string(), cmd.to_string()];
    args.extend_from_slice(tail);
    match parse_cli_args(&args) {
        Ok(Some(command)) => match run_cli_command(command, config, state_path).await {
            Ok(CliOutput::Json(v)) => Ok(v),
            Ok(CliOutput::Text(t)) => Ok(serde_json::json!({ "text": t })),
            Err(e) => Err(format!("{cmd} failed: {e}")),
        },
        Ok(None) => Err(format!("could not parse /{cmd}")),
        Err(e) => Err(format!("parse error: {e}")),
    }
}

// ── Output formatters (clean Telegram text instead of raw JSON) ──────

/// Compact number: 8326 → "8.3K", 30458 → "30.5K".
fn compact(n: f64) -> String {
    let a = n.abs();
    if a >= 1e9 {
        format!("{:.1}B", n / 1e9)
    } else if a >= 1e6 {
        format!("{:.1}M", n / 1e6)
    } else if a >= 1e3 {
        format!("{:.1}K", n / 1e3)
    } else {
        format!("{:.0}", n)
    }
}

/// Shorten a long address to `abcd…wxyz`.
fn short(s: &str) -> String {
    if s.len() > 12 {
        format!("{}…{}", &s[..4], &s[s.len() - 4..])
    } else {
        s.to_string()
    }
}

fn numf(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn fmt_balance(v: &Value) -> String {
    let d = v.get("data").unwrap_or(v);
    let sol = numf(d, "sol");
    let usd = numf(d, "totalUsd");
    if usd > 0.0 {
        format!("💰 Wallet\n◎ {sol:.4} SOL  (~${usd:.2})")
    } else {
        format!("💰 Wallet\n◎ {sol:.4} SOL")
    }
}

fn fmt_positions(v: &Value) -> String {
    let empty = Vec::new();
    let list = v
        .get("data")
        .and_then(|d| d.get("positions"))
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    if list.is_empty() {
        return "📊 No open positions.".to_string();
    }
    let mut out = format!("📊 Open positions ({})", list.len());
    for p in list {
        let name = p
            .get("pool_name")
            .and_then(Value::as_str)
            .or_else(|| p.get("base_symbol").and_then(Value::as_str))
            .unwrap_or("?");
        let liq = p
            .get("liquidity_sol")
            .and_then(Value::as_f64)
            .unwrap_or_else(|| numf(p, "amount_sol"));
        let pnl = p.get("live_pnl_pct").and_then(Value::as_f64).unwrap_or(0.0);
        let range = if p.get("in_range").and_then(Value::as_bool).unwrap_or(true) {
            "in-range"
        } else {
            "OUT"
        };
        out.push_str(&format!("\n\n{name}\n  ◎{liq:.3} · PnL {pnl:+.2}% · {range}"));
    }
    out
}

fn fmt_candidates(v: &Value) -> String {
    let empty = Vec::new();
    let list = v
        .get("data")
        .and_then(|d| d.get("candidates"))
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    if list.is_empty() {
        return "🎯 No candidates right now.".to_string();
    }
    let mut out = format!("🎯 Candidates ({})", list.len());
    for (i, c) in list.iter().enumerate().take(12) {
        let name = c
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| {
                c.get("base")
                    .and_then(|b| b.get("symbol"))
                    .and_then(Value::as_str)
            })
            .unwrap_or("?");
        out.push_str(&format!(
            "\n{}. {name} · score {} · TVL ${} · fees ◎{}",
            i + 1,
            compact(numf(c, "score")),
            compact(numf(c, "tvl")),
            compact(numf(c, "fees_sol")),
        ));
    }
    out
}

/// Char-safe truncation to stay under Telegram's message limit.
fn truncate(s: &str) -> String {
    if s.chars().count() > MAX_TG_LEN {
        let cut: String = s.chars().take(MAX_TG_LEN).collect();
        format!("{cut}…")
    } else {
        s.to_string()
    }
}

/// Portfolio PnL summary matching the dashboard: realized (closed) + unrealized
/// (open) across all pools the wallet has touched, sourced from Meteora.
async fn portfolio_text(config: &Config) -> String {
    let wallet = crate::tools::meteora_native::wallet_pubkey_from_env().unwrap_or_default();
    if wallet.is_empty() {
        return "⚠️ wallet not set (MERIDIAN_WALLET)".to_string();
    }
    let _ = config; // wallet comes from env; config reserved for future use
    let pools = crate::tools::dlmm::get_all_wallet_pools(&wallet).await;
    let mut realized = 0.0;
    let mut deposit = 0.0;
    let mut fees = 0.0;
    let mut closed = 0usize;
    let mut wins = 0usize;
    for (pool, name) in &pools {
        if let Some(h) = crate::tools::dlmm::get_pool_history(pool, name, &wallet).await {
            realized += h.pnl_usd;
            deposit += h.deposit_usd;
            fees += h.fees_usd;
            closed += h.closed_count;
            wins += h.win_count;
        }
    }
    let mut unrealized = 0.0;
    for (pool, _) in &pools {
        unrealized += crate::tools::dlmm::get_pool_open_pnl(pool, &wallet).await;
    }
    let total = realized + unrealized;
    let pct = if deposit > 0.0 {
        total / deposit * 100.0
    } else {
        0.0
    };
    let win_rate = if closed > 0 {
        wins as f64 / closed as f64 * 100.0
    } else {
        0.0
    };
    format!(
        "💰 Total PnL: ${total:.2} ({pct:.2}%)\n  realized ${realized:.2} · unrealized ${unrealized:.2}\nFees ${fees:.2} · Win rate {win_rate:.1}% · {closed} closed"
    )
}
