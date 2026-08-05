//! LP Agent (api.lpagent.io) read-only screening signals.
//!
//! Uses the pool's on-chain top-LPers as a real-money quality signal: are the
//! people already providing liquidity in this pool actually making money? A pool
//! whose established LPers are net-profitable is a better place to deploy than
//! one where they're bleeding. Read-only — never generates or signs anything.

use crate::config::Config;
use serde_json::Value;

const LPAGENT_BASE: &str = "https://api.lpagent.io/open-api/v1";

/// LP Agent API key from env (`LPAGENT_API_KEY`). None disables the signal.
pub fn lpagent_api_key() -> Option<String> {
    std::env::var("LPAGENT_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
}

pub fn has_lpagent_api_key() -> bool {
    lpagent_api_key().is_some()
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Position-weighted win rate (0.0–1.0) for a pool: of all the individual LP
/// positions opened by the pool's largest liquidity providers, what fraction
/// ended profitable *in SOL terms* (`win_lp_native / total_lp`)?
///
/// Sampled by `total_inflow` — the biggest LPers — deliberately. The previous
/// implementation asked for the top LPers by `total_pnl` and then measured how
/// many of them had `total_pnl > 0`, which is a tautology: it returned ~100% for
/// every pool measured (three live pools all scored 20/20), so the score boost
/// was a constant with zero ranking power, and it stamped "100% profitable" on
/// pools that promptly stopped us out. Sorting by size instead samples whoever
/// actually committed capital, win or lose, and the SOL-denominated win rate is
/// the right lens for a single-side-SOL strategy.
///
/// `None` on missing key / error / no data.
pub async fn get_pool_lper_win_rate(pool: &str, _config: &Config) -> Option<f64> {
    let key = lpagent_api_key()?;
    if pool.is_empty() {
        return None;
    }
    let url = format!(
        "{}/pools/{}/top-lpers?order_by=total_inflow&sort_order=desc&page=1&limit=20",
        LPAGENT_BASE, pool
    );
    let client = reqwest::Client::new();
    let resp: Value = client
        .get(&url)
        .header("x-api-key", key)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let list = resp.get("data").and_then(Value::as_array)?;

    let (mut winning_positions, mut total_positions) = (0.0f64, 0.0f64);
    for lp in list {
        // Some pools carry a synthetic row with an empty owner that buckets
        // hundreds of unattributed positions. Counting it as one LPer badly
        // skews any per-LPer statistic, so drop it.
        if lp.get("owner").and_then(Value::as_str).unwrap_or("").is_empty() {
            continue;
        }
        // Skip dust / near-instant LPers — they add noise, not signal.
        let inflow = lp.get("total_inflow").and_then(num).unwrap_or(0.0);
        let age_hours = lp.get("avg_age_hour").and_then(num).unwrap_or(0.0);
        if inflow < 50.0 || age_hours < 0.5 {
            continue;
        }
        let positions = lp.get("total_lp").and_then(num).unwrap_or(0.0);
        if positions <= 0.0 {
            continue;
        }
        // Weight by positions, not by LPer: one whale with 200 positions says
        // more about the pool than one tourist with a single lucky entry.
        total_positions += positions;
        winning_positions += lp
            .get("win_lp_native")
            .and_then(num)
            .or_else(|| lp.get("win_lp").and_then(num))
            .unwrap_or(0.0);
    }
    if total_positions <= 0.0 {
        return None;
    }
    Some((winning_positions / total_positions).clamp(0.0, 1.0))
}
