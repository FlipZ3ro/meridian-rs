//! Quick-flip mode — a deterministic, fast, volume-spike DLMM strategy.
//!
//! Separate from the LLM agent. The edge (per LP Army "Quick In & Out"): enter a
//! pool on a big organic volume spike into a high-fee pool, farm the fee burst
//! for a very short hold, and exit the moment volume fades — before impermanent
//! loss can bite. Volume is the only real signal; PnL comes from fees, not price.
//!
//! MVP scope + honesty: this is a SINGLE-SIDE SOL adaptation (the article runs
//! two-sided spot, which needs a swap/zap we don't do yet). It reuses our
//! single-side deploy but applies the quick-flip SELECTION (volume spike + high
//! fee generation + clean GMGN), TIMING (fast), and EXIT (volume fade / max
//! hold). One position at a time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::utils::logger::module::{info, warn};

/// Process-wide quick-flip toggle. The web control surface (`/api/control`
/// action `quickflip`) flips it; the loop reads it every poll. A module static
/// (rather than an injected Arc) because the web server task doesn't share
/// state with `main`'s spawned loops on this branch.
pub static QUICKFLIP_ENABLED: AtomicBool = AtomicBool::new(false);

/// Arm/disarm the scalper at runtime.
pub fn set_enabled(on: bool) {
    QUICKFLIP_ENABLED.store(on, Ordering::SeqCst);
}

/// Whether the scalper is currently armed.
pub fn is_enabled() -> bool {
    QUICKFLIP_ENABLED.load(Ordering::SeqCst)
}

/// A live quick-flip position (at most one at a time), persisted so a restart
/// doesn't lose track of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct QfPosition {
    id: String,
    pool: String,
    symbol: String,
    entered_at: String,
    entry_vol_per_min: f64,
    amount_sol: f64,
}

fn qf_path() -> std::path::PathBuf {
    crate::config::meridian_data_path("quickflip.json")
}

/// Best-effort Telegram push (no-op unless a bot token + chat are configured).
/// On this branch the primary UX is the web dashboard, so this is optional.
async fn notify(config: &Config, text: &str) {
    let token = config
        .api
        .telegram_bot_token
        .as_deref()
        .filter(|s| !s.is_empty());
    let chat = config
        .api
        .telegram_chat_id
        .as_deref()
        .filter(|s| !s.is_empty());
    if let (Some(token), Some(chat)) = (token, chat) {
        let _ = crate::tools::telegram::send_message_safe(token, chat, text).await;
    }
}

fn load_qf() -> Option<QfPosition> {
    let p = qf_path();
    let data = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_qf(pos: &QfPosition) {
    if let Ok(data) = serde_json::to_string_pretty(pos) {
        let _ = std::fs::write(qf_path(), data);
    }
}

fn clear_qf() {
    let _ = std::fs::remove_file(qf_path());
}

/// Minutes represented by a screening timeframe string ("1h", "5m", …).
fn timeframe_minutes(tf: &str) -> f64 {
    let tf = tf.trim().to_lowercase();
    let digits: String = tf.chars().filter(|c| c.is_ascii_digit()).collect();
    let n: f64 = digits.parse().unwrap_or(60.0);
    if tf.contains('h') {
        n * 60.0
    } else if tf.contains('d') {
        n * 1440.0
    } else {
        n.max(1.0) // minutes (or a bare number)
    }
}

/// Current volume-per-minute for a pool, from its live discovery detail.
async fn vol_per_min(pool: &str, config: &Config) -> Option<f64> {
    let detail = crate::tools::screening::Screener::new()
        .get_pool_detail(pool, &config.screening.timeframe)
        .await
        .ok()
        .flatten()?;
    let vol = detail.volume.unwrap_or(0.0);
    let mins = timeframe_minutes(&config.screening.timeframe);
    Some(vol / mins)
}

/// The quick-flip loop. Spawned from `main`; never returns. Arm/disarm at
/// runtime via `set_enabled` (wired to the web control surface).
pub async fn run(config: Config, wallet: String) {
    let qf = &config.quickflip;
    let poll = Duration::from_secs(qf.poll_secs.max(15));
    info(
        "quickflip",
        &format!(
            "loop ready (enabled={}, min_vol/min=${:.0}, hold≤{}m, fade×{:.2})",
            is_enabled(),
            qf.min_vol_per_min,
            qf.max_hold_min,
            qf.vol_fade_ratio
        ),
    );

    loop {
        tokio::time::sleep(poll).await;
        if !is_enabled() {
            continue;
        }

        match load_qf() {
            Some(pos) => monitor(&pos, &config, &wallet).await,
            None => scan_and_enter(&config, &wallet).await,
        }
    }
}

/// Monitor the open position and exit on volume fade / bounce / max hold.
async fn monitor(pos: &QfPosition, config: &Config, wallet: &str) {
    let qf = &config.quickflip;
    let age_min = minutes_since(&pos.entered_at);
    let cur_vol = vol_per_min(&pos.pool, config).await.unwrap_or(0.0);
    let faded = pos.entry_vol_per_min > 0.0 && cur_vol < pos.entry_vol_per_min * qf.vol_fade_ratio;

    // Live unrealized PnL (USD) — 0.0 for a dry-run position not on-chain, or
    // when we have no wallet to query positions for.
    let pnl_usd = if wallet.is_empty() {
        0.0
    } else {
        crate::tools::dlmm::get_pool_open_pnl(&pos.pool, wallet).await
    };

    let reason = if age_min >= qf.max_hold_min {
        Some("max hold reached")
    } else if pnl_usd > qf.take_profit_usd {
        Some("first bounce (in profit)")
    } else if faded {
        Some("volume faded")
    } else {
        None
    };

    let Some(reason) = reason else {
        return; // hold — still spiking, within hold window
    };

    info(
        "quickflip",
        &format!(
            "EXIT {} — {} (age {}m, vol/min ${:.0}→${:.0})",
            pos.symbol, reason, age_min, pos.entry_vol_per_min, cur_vol
        ),
    );
    match crate::tools::dlmm::close_position(&pos.id, Some(reason), config).await {
        Ok(_) => {
            notify(
                config,
                &format!("⚡ Quick-flip EXIT {} — {}", pos.symbol, reason),
            )
            .await;
            clear_qf();
        }
        Err(e) => warn("quickflip", &format!("close failed: {} — will retry", e)),
    }
}

/// Scan for a qualifying volume spike and enter one position.
async fn scan_and_enter(config: &Config, wallet: &str) {
    let qf = &config.quickflip;
    let screener = crate::tools::screening::Screener::new();
    let pools = match screener.discover_pools(&config.screening, 100).await {
        Ok(p) => p,
        Err(e) => {
            warn("quickflip", &format!("discovery failed: {}", e));
            return;
        }
    };
    let mins = timeframe_minutes(&config.screening.timeframe);

    // Rank qualifying pools by volume-per-minute (volume is king).
    let mut ranked: Vec<(f64, &crate::models::pool::RawPool)> = pools
        .iter()
        .filter_map(|p| {
            let addr = p.pool_address.as_deref()?;
            if addr.is_empty() {
                return None;
            }
            let vpm = p.volume.unwrap_or(0.0) / mins;
            let tvl = p.tvl.or(p.active_tvl).unwrap_or(0.0);
            let fee_tvl = p.fee_active_tvl_ratio.unwrap_or(0.0);
            let ok = vpm >= qf.min_vol_per_min
                && fee_tvl >= qf.min_fee_tvl_ratio
                && tvl >= qf.min_tvl
                && tvl <= qf.max_tvl
                && p.base_token_has_critical_warnings != Some(true)
                && p.base_token_has_high_single_ownership != Some(true);
            if ok {
                Some((vpm, p))
            } else {
                None
            }
        })
        .collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let Some((vpm, pool)) = ranked.first().copied() else {
        return; // no spike right now
    };
    let pool_addr = pool.pool_address.clone().unwrap_or_default();
    let symbol = pool
        .name
        .clone()
        .or_else(|| pool.token_x.as_ref().and_then(|t| t.symbol.clone()))
        .unwrap_or_else(|| pool_addr.chars().take(8).collect());

    // Final safety gate: GMGN security on the base token (same as the agent).
    if crate::tools::gmgn::has_gmgn_api_key(config) {
        if let Ok(base_mint) =
            crate::tools::meteora_native::pool_base_mint(config, &pool_addr).await
        {
            if let Some(sec) = crate::tools::gmgn::get_token_security(&base_mint, config).await {
                if sec.honeypot || sec.cannot_sell || sec.blacklist || !sec.renounced_mint {
                    info(
                        "quickflip",
                        &format!("skip {} — failed GMGN security", symbol),
                    );
                    return;
                }
                // Holder-distribution gate (same as the LLM agent's pre-flight).
                let thr = config.gmgn.max_top10_holder_rate;
                if thr > 0.0 && thr < 1.0 {
                    let rate = if sec.top_10_holder_rate > 1.0 {
                        sec.top_10_holder_rate / 100.0
                    } else {
                        sec.top_10_holder_rate
                    };
                    if rate > thr {
                        info(
                            "quickflip",
                            &format!(
                                "skip {} — top-10 holders {:.0}% > {:.0}% limit",
                                symbol,
                                rate * 100.0,
                                thr * 100.0
                            ),
                        );
                        return;
                    }
                }
            }
        }
    }

    info(
        "quickflip",
        &format!(
            "SPIKE {} — vol/min ${:.0} (deploying {:.3} SOL, {} bins)",
            symbol, vpm, qf.deploy_amount_sol, qf.bins_below
        ),
    );
    match crate::tools::dlmm::deploy_position(
        &pool_addr,
        qf.deploy_amount_sol,
        Some(qf.bins_below),
        Some(qf.bins_above),
        Some("spot"),
        config,
    )
    .await
    {
        Ok(result) => {
            let id = result
                .position
                .clone()
                .unwrap_or_else(|| format!("qf-{}", &pool_addr[..pool_addr.len().min(6)]));
            save_qf(&QfPosition {
                id,
                pool: pool_addr,
                symbol: symbol.clone(),
                entered_at: crate::utils::time::now_iso(),
                entry_vol_per_min: vpm,
                amount_sol: qf.deploy_amount_sol,
            });
            let _ = wallet; // reserved (single-side SOL uses the env keypair)
            notify(
                config,
                &format!("⚡ Quick-flip ENTER {} — vol/min ${:.0}", symbol, vpm),
            )
            .await;
        }
        Err(e) => warn("quickflip", &format!("deploy failed: {}", e)),
    }
}

fn minutes_since(iso: &str) -> u32 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|start| {
            (chrono::Utc::now()
                .signed_duration_since(start.with_timezone(&chrono::Utc))
                .num_seconds() as f64
                / 60.0)
                .max(0.0) as u32
        })
        .unwrap_or(0)
}
