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

/// Fraction (0.0–1.0) of a pool's established top LPers that are net-profitable
/// (`total_pnl > 0`), considering only LPers with meaningful inflow + age so
/// dust/instant flips don't skew it. `None` on missing key / error / no data.
pub async fn get_pool_profitable_lper_ratio(pool: &str, _config: &Config) -> Option<f64> {
    let key = lpagent_api_key()?;
    if pool.is_empty() {
        return None;
    }
    let url = format!(
        "{}/pools/{}/top-lpers?order_by=total_pnl&sort_order=desc&page=1&limit=20",
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

    let (mut considered, mut profitable) = (0u32, 0u32);
    for lp in list {
        // Skip dust / near-instant LPers — they add noise, not signal.
        let inflow = lp.get("total_inflow").and_then(num).unwrap_or(0.0);
        let age_hours = lp.get("avg_age_hour").and_then(num).unwrap_or(0.0);
        if inflow < 50.0 || age_hours < 0.5 {
            continue;
        }
        considered += 1;
        if lp.get("total_pnl").and_then(num).unwrap_or(0.0) > 0.0 {
            profitable += 1;
        }
    }
    if considered == 0 {
        return None;
    }
    Some(profitable as f64 / considered as f64)
}
