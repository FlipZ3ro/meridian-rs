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
use std::time::Duration;

const TG_API: &str = "https://api.telegram.org";
const MAX_TG_LEN: usize = 3800; // Telegram caps messages at 4096 chars

const HELP: &str = "🤖 *Meridian control*\n\
/status — agent state + open positions\n\
/positions — open positions detail\n\
/pnl — portfolio PnL (realized + unrealized)\n\
/balance — wallet SOL balance\n\
/candidates [n] — top screening candidates\n\
/brief — daily brief + anomaly check\n\
/start — resume trading (new deploys)\n\
/stop — pause new deploys (still manages open)\n\
/dryrun [on|off] — toggle simulated vs live execution\n\
/quickflip [on|off] — toggle volume-spike scalper\n\
/close <pool|position> — close a position\n\
/help — this message";

/// Spawned from `main`. Never returns; loops on getUpdates.
pub async fn run(
    config: Config,
    state_path: String,
) {
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
    set_commands(&client, &token).await;
    send_keyboard(
        &client,
        &token,
        &admin,
        "🤖 Meridian control online — tap a button below or /help",
    )
    .await;
    info("telegram", "interactive control online");

    let mut offset: i64 = 0;
    loop {
        match get_updates(&client, &token, offset).await {
            Ok(updates) => {
                for upd in updates {
                    let id = upd.get("update_id").and_then(Value::as_i64).unwrap_or(offset);
                    offset = id + 1;

                    // A tap on an inline "Refresh" button arrives as a
                    // callback_query, not a message. Re-run the command it
                    // carries and edit the original message in place, so the
                    // chat isn't buried under a new copy on every refresh.
                    if let Some(cb) = upd.get("callback_query") {
                        let cb_id = cb.get("id").and_then(Value::as_str).unwrap_or("");
                        let data = cb.get("data").and_then(Value::as_str).unwrap_or("");
                        let cb_chat = cb
                            .get("message")
                            .and_then(|m| m.get("chat"))
                            .and_then(|c| c.get("id"))
                            .and_then(Value::as_i64)
                            .map(|i| i.to_string())
                            .unwrap_or_default();
                        let msg_id = cb
                            .get("message")
                            .and_then(|m| m.get("message_id"))
                            .and_then(Value::as_i64)
                            .unwrap_or(0);

                        // Always answer, even when rejecting: an unanswered
                        // callback leaves a spinner stuck on the button.
                        answer_callback(&client, &token, cb_id).await;
                        if cb_chat != admin {
                            warn("telegram", &format!("rejected non-admin callback {cb_chat}"));
                            continue;
                        }
                        if let Some(cmd) = data.strip_prefix("r:") {
                            let body = handle(cmd, &config, &state_path).await;
                            edit_with_refresh(&client, &token, &admin, msg_id, &body, cmd).await;
                        }
                        continue;
                    }

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

                    let reply = handle(text, &config, &state_path).await;
                    match refreshable(text) {
                        Some(cmd) => {
                            send_with_refresh(&client, &token, &admin, &reply, cmd).await
                        }
                        None => {
                            let _ =
                                crate::tools::telegram::send_message_safe(&token, &admin, &reply)
                                    .await;
                        }
                    }
                }
            }
            Err(e) => {
                warn("telegram", &format!("getUpdates error: {e}"));
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// Register the bot's command menu (the "Menu" button / `/` list shown at the
/// bottom of the chat). `/start` is intentionally omitted — it stays usable if
/// typed, but doesn't clutter the menu.
async fn set_commands(client: &reqwest::Client, token: &str) {
    let body = serde_json::json!({
        "commands": [
            { "command": "status",     "description": "Agent state + open positions" },
            { "command": "positions",  "description": "Open positions detail" },
            { "command": "pnl",        "description": "Portfolio PnL (realized + unrealized)" },
            { "command": "balance",    "description": "Wallet SOL balance" },
            { "command": "candidates", "description": "Top screening candidates" },
            { "command": "brief",      "description": "Daily brief + anomaly check" },
            { "command": "dryrun",     "description": "Toggle simulated vs live execution" },
            { "command": "quickflip",  "description": "Toggle volume-spike scalper" },
            { "command": "stop",       "description": "Pause new deploys" },
            { "command": "close",      "description": "Close a position" },
            { "command": "help",       "description": "List commands" }
        ]
    });
    let url = format!("{TG_API}/bot{token}/setMyCommands");
    if let Err(e) = client.post(&url).json(&body).send().await {
        warn("telegram", &format!("setMyCommands failed: {e}"));
    }
}

/// Persistent reply keyboard shown below the chat input — tap a button to run
/// the command. `/start` is intentionally not a button.
fn keyboard() -> Value {
    serde_json::json!({
        "keyboard": [
            [{ "text": "📊 Status" }, { "text": "📋 Positions" }],
            [{ "text": "📈 PnL" }, { "text": "💰 Balance" }],
            [{ "text": "🎯 Candidates" }, { "text": "📋 Brief" }],
            [{ "text": "🧪 Dry-run" }, { "text": "❓ Help" }],
            [{ "text": "⏸️ Stop" }]
        ],
        "resize_keyboard": true,
        "is_persistent": true
    })
}

/// Reply-keyboard buttons send their label, not a command — translate.
fn map_button_label(text: &str) -> &str {
    match text.trim() {
        "📊 Status" => "/status",
        "📋 Positions" => "/positions",
        "📋 Brief" => "/brief",
        "📈 PnL" => "/pnl",
        "💰 Balance" => "/balance",
        "🎯 Candidates" => "/candidates",
        "🧪 Dry-run" => "/dryrun",
        "⏸️ Stop" => "/stop",
        "❓ Help" => "/help",
        other => other,
    }
}

/// Bare command name: strips the leading '/' and any '@botname' suffix.
fn normalize(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('/')
        .split('@')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

/// Commands whose answer goes stale immediately and is worth re-reading in
/// place. Everything else (help, start/stop confirmations) is a one-shot reply
/// and gets no button.
fn refreshable(text: &str) -> Option<&'static str> {
    match normalize(map_button_label(text)).as_str() {
        "positions" => Some("positions"),
        "brief" => Some("brief"),
        "balance" => Some("balance"),
        "status" => Some("status"),
        "pnl" => Some("pnl"),
        _ => None,
    }
}

/// Inline keyboard carrying the command to re-run. `r:` prefix keeps the
/// callback namespace open for other button types later.
fn refresh_markup(cmd: &str) -> Value {
    serde_json::json!({
        "inline_keyboard": [[{ "text": "🔄 Refresh", "callback_data": format!("r:{cmd}") }]]
    })
}

/// A refreshed message must differ from the previous one or Telegram rejects
/// the edit ("message is not modified"). The timestamp guarantees that and
/// doubles as the answer to "how fresh is this?".
fn stamped(text: &str) -> String {
    format!(
        "{text}\n\n🕒 {}",
        chrono::Utc::now().format("%H:%M:%S UTC")
    )
}

async fn send_with_refresh(
    client: &reqwest::Client,
    token: &str,
    chat: &str,
    text: &str,
    cmd: &str,
) {
    let url = format!("{TG_API}/bot{token}/sendMessage");
    let body = serde_json::json!({
        "chat_id": chat,
        "text": stamped(text),
        "parse_mode": "Markdown",
        "reply_markup": refresh_markup(cmd),
    });
    if let Err(e) = client.post(&url).json(&body).send().await {
        warn("telegram", &format!("sendMessage(refresh) failed: {e}"));
    }
}

async fn edit_with_refresh(
    client: &reqwest::Client,
    token: &str,
    chat: &str,
    message_id: i64,
    text: &str,
    cmd: &str,
) {
    let url = format!("{TG_API}/bot{token}/editMessageText");
    let body = serde_json::json!({
        "chat_id": chat,
        "message_id": message_id,
        "text": stamped(text),
        "parse_mode": "Markdown",
        "reply_markup": refresh_markup(cmd),
    });
    if let Err(e) = client.post(&url).json(&body).send().await {
        warn("telegram", &format!("editMessageText failed: {e}"));
    }
}

/// Clears the loading spinner on the tapped button.
async fn answer_callback(client: &reqwest::Client, token: &str, callback_id: &str) {
    if callback_id.is_empty() {
        return;
    }
    let url = format!("{TG_API}/bot{token}/answerCallbackQuery");
    let body = serde_json::json!({ "callback_query_id": callback_id });
    let _ = client.post(&url).json(&body).send().await;
}

/// Send a message that also (re)attaches the persistent reply keyboard.
async fn send_keyboard(client: &reqwest::Client, token: &str, chat: &str, text: &str) {
    let url = format!("{TG_API}/bot{token}/sendMessage");
    let body = serde_json::json!({
        "chat_id": chat,
        "text": text,
        "reply_markup": keyboard(),
    });
    if let Err(e) = client.post(&url).json(&body).send().await {
        warn("telegram", &format!("sendMessage(keyboard) failed: {e}"));
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
) -> String {
    let mapped = map_button_label(text);
    let mut it = mapped.trim().split_whitespace();
    let cmd = normalize(it.next().unwrap_or(""));
    let rest: Vec<String> = it.map(|s| s.to_string()).collect();

    match cmd.as_str() {
        "" | "help" => HELP.to_string(),
        "start" => {
            crate::cycle::set_trading_enabled(true);
            format!(
                "▶️ Trading ENABLED · {} — bot will deploy on valid candidates.",
                mode_label(config)
            )
        }
        "stop" => {
            crate::cycle::set_trading_enabled(false);
            "⏸️ Trading PAUSED — no new deploys. Open positions still managed & closed.".to_string()
        }
        "dryrun" | "dry" => {
            let target = match rest.first().map(|s| s.to_lowercase()).as_deref() {
                Some("on" | "true" | "1") => true,
                Some("off" | "false" | "0") => false,
                _ => !crate::tools::dlmm::is_dry_run(config), // no arg → toggle
            };
            std::env::set_var("DRY_RUN", if target { "true" } else { "false" });
            if target {
                "🧪 DRY-RUN ON — deploys are simulated, no real transactions.".to_string()
            } else {
                "🔴 LIVE MODE — real transactions will be sent!\nUse /dryrun on to return to simulation."
                    .to_string()
            }
        }
        "quickflip" | "qf" | "flip" => {
            let target = match rest.first().map(|s| s.to_lowercase()).as_deref() {
                Some("on" | "true" | "1") => true,
                Some("off" | "false" | "0") => false,
                _ => !crate::tools::quickflip::is_enabled(), // no arg → toggle
            };
            crate::tools::quickflip::set_enabled(target);
            let qf = &config.quickflip;
            if target {
                format!(
                    "⚡ Quick-flip ON · {} — volume-spike scalper armed.\n  entry vol/min ≥ ${:.0}k · hold ≤ {}m · fade ×{:.2} · {:.3} SOL/pos",
                    mode_label(config),
                    qf.min_vol_per_min / 1000.0,
                    qf.max_hold_min,
                    qf.vol_fade_ratio,
                    qf.deploy_amount_sol,
                )
            } else {
                "⚪ Quick-flip OFF — scalper paused (open flip position still managed).".to_string()
            }
        }
        "pnl" => portfolio_text(config, state_path).await,
        "status" => {
            let flag = if crate::cycle::is_trading_enabled() {
                "▶️ Trading ENABLED"
            } else {
                "⏸️ Trading PAUSED"
            };
            match run_json("status", &[], config, state_path).await {
                Ok(v) => {
                    // Keep only the headline line (Open | Closed | Fees); the
                    // full state summary dumps per-position + event detail.
                    let headline = v
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .lines()
                        .next()
                        .unwrap_or("")
                        .replace(" | ", "\n");
                    format!("{flag} · {}\n\n{headline}", mode_label(config))
                }
                Err(e) => format!("⚠️ {e}"),
            }
        }
        "balance" => match run_json("balance", &[], config, state_path).await {
            Ok(v) => fmt_balance(&v),
            Err(e) => format!("⚠️ {e}"),
        },
        "positions" => fmt_state_positions(state_path),
        "brief" => crate::tools::brief::render(state_path),
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

/// Current execution mode label for status/start messages.
fn mode_label(config: &Config) -> &'static str {
    if crate::tools::dlmm::is_dry_run(config) {
        "🧪 DRY-RUN"
    } else {
        "🔴 LIVE"
    }
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

/// Open positions from the bot's TRACKED state (works for real AND dry-run,
/// since dry-run positions never exist on-chain). Marks simulated ones with 🧪.
fn fmt_state_positions(state_path: &str) -> String {
    use crate::state::positions::{PositionState, PositionStatus};
    let state = match PositionState::load(state_path) {
        Ok(s) => s,
        Err(e) => return format!("⚠️ could not read state: {e}"),
    };
    let active = state.get_active();
    if active.is_empty() {
        return "📊 No open positions.".to_string();
    }
    // Totals first: on a phone the portfolio answer matters more than any
    // single row, and fees are shown alongside PnL because they routinely
    // cover most of a loss — PnL alone reads worse than the position is.
    let total_pnl: f64 = active.iter().filter_map(|p| p.pnl_sol).sum::<f64>() + 0.0;
    let total_fees: f64 = active.iter().filter_map(|p| p.all_time_fees_usd).sum::<f64>() + 0.0;
    let mut out = format!(
        "📊 Open positions ({})\n◎{:+.4} SOL · fees ${:.2}",
        active.len(),
        total_pnl,
        total_fees
    );

    for p in active {
        let name = p
            .pool_name
            .clone()
            .or_else(|| p.base_symbol.clone())
            .unwrap_or_else(|| "?".to_string());
        let dry = if p.id.starts_with("dryrun-") { " 🧪" } else { "" };
        let status = match p.status {
            PositionStatus::Active => "in-range",
            PositionStatus::OutOfRange => "⚠️ out-of-range",
            PositionStatus::Closed => "closed",
        };
        // A position that has never been polled has no PnL yet; say so rather
        // than printing a misleading 0.00%.
        let pnl = match p.pnl_pct {
            Some(pct) => {
                let mark = if pct >= 0.0 { "🟢" } else { "🔴" };
                format!("{mark} {pct:+.2}%")
            }
            None => "⏳ awaiting first poll".to_string(),
        };
        let fees = p.all_time_fees_usd.unwrap_or(0.0);
        let fee_str = if fees > 0.0 {
            format!(" · fees ${fees:.2}")
        } else {
            String::new()
        };
        out.push_str(&format!(
            "\n\n{name}{dry}\n  {pnl}{fee_str}\n  ◎{:.3} SOL · {status}",
            p.amount_sol
        ));
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
        let smart = c.get("smart_money_count").and_then(Value::as_u64).unwrap_or(0);
        let smart_str = if smart > 0 {
            format!(" · 🧠{smart}")
        } else {
            String::new()
        };
        // fee/TVL is the metric the strategy actually lives on — it is what
        // screening filters against and what decides whether fees can outrun
        // impermanent loss. The raw `score` is an internal ranking number and
        // means nothing on a phone, so it is not shown.
        let fee_tvl = numf(c, "fee_active_tvl_ratio");
        out.push_str(&format!(
            "\n{}. {name} · fee/TVL {:.2} · TVL ${} · vol ${}{smart_str}",
            i + 1,
            fee_tvl,
            compact(numf(c, "tvl")),
            compact(numf(c, "volume")),
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
async fn portfolio_text(config: &Config, state_path: &str) -> String {
    use crate::state::positions::{PositionState, PositionStatus};
    let _ = config;

    let state = match PositionState::load(state_path) {
        Ok(s) => s,
        Err(e) => return format!("⚠️ tidak bisa baca state: {e}"),
    };
    let all: Vec<_> = state.positions.values().collect();
    let open: Vec<_> = all
        .iter()
        .filter(|p| p.status != PositionStatus::Closed)
        .collect();
    let closed: Vec<_> = all
        .iter()
        .filter(|p| p.status == PositionStatus::Closed)
        .filter(|p| p.pnl_usd.is_some() || p.all_time_fees_usd.is_some())
        .collect();

    // Realized comes from the bot's own closed positions, not the pool API.
    // The API lags indexing by minutes and misses positions it never saw, so it
    // reported \/usr/bin/bash.31 against \.52 of open PnL — a portfolio view that
    // disagrees with /positions is worse than none.
    let r_pnl: f64 = closed.iter().map(|p| p.pnl_usd.unwrap_or(0.0)).sum::<f64>() + 0.0;
    let r_fees: f64 = closed
        .iter()
        .map(|p| p.all_time_fees_usd.unwrap_or(0.0))
        .sum::<f64>()
        + 0.0;
    let u_pnl: f64 = open.iter().map(|p| p.pnl_usd.unwrap_or(0.0)).sum::<f64>() + 0.0;
    let u_fees: f64 = open
        .iter()
        .map(|p| p.all_time_fees_usd.unwrap_or(0.0))
        .sum::<f64>()
        + 0.0;
    let wins = closed
        .iter()
        .filter(|p| p.pnl_usd.unwrap_or(0.0) + p.all_time_fees_usd.unwrap_or(0.0) > 0.0)
        .count();

    // Position rent is locked, not spent: closing returns it to the wallet.
    // Shown so the gap between wallet balance and deployable capital is
    // obvious, but deliberately kept out of the net — counting refundable rent
    // as a cost would understate performance by roughly \ per open position.
    let sol_price = crate::tools::wallet::get_sol_price().await.unwrap_or(0.0);
    let rent_locked = open.len() as f64 * 0.0574 * sol_price;

    let net = r_pnl + r_fees + u_pnl + u_fees;

    // Fixed-width rows inside a monospace fence: label left, figure right, so
    // every number lands in the same column whatever the label length. Built a
    // row at a time — a single multi-line format string kept its own source
    // indentation and produced a ragged block.
    let row = |label: &str, value: f64| format!("{label:<16}{value:>10.2}\n");
    let mut body = String::new();
    body.push_str(&format!("REALIZED   {} closed\n", closed.len()));
    body.push_str(&row("  pnl", r_pnl));
    body.push_str(&row("  fee", r_fees));
    body.push_str(&row("  subtotal", r_pnl + r_fees));
    body.push_str(&format!("\nUNREALIZED {} open\n", open.len()));
    body.push_str(&row("  pnl", u_pnl));
    body.push_str(&row("  fee", u_fees));
    body.push_str(&row("  subtotal", u_pnl + u_fees));
    if rent_locked > 0.0 {
        body.push('\n');
        body.push_str(&row("rent locked", rent_locked));
        body.push_str("  (refunded on close)\n");
    }
    body.push_str(&"-".repeat(26));
    body.push('\n');
    body.push_str(&row("NET", net));
    if !closed.is_empty() {
        body.push_str(&format!(
            "{:<16}{:>9.0}%  {}/{}\n",
            "win rate",
            wins as f64 / closed.len() as f64 * 100.0,
            wins,
            closed.len()
        ));
    }
    format!("💰 *PORTFOLIO*\n\n```\n{body}```")
}
