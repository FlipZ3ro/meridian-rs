//! GMGN OpenAPI fee source.
//!
//! Port of the original Node.js `tools/gmgn.js`. Provides cumulative token fee
//! figures (`total_fee` / `trade_fee` in SOL) used as the primary
//! `minTokenFeesSol` screening gate, with graceful fallback to pool/Jupiter
//! fees when no API key is configured.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::Mutex;
use tokio::time::{sleep, Instant};

use crate::config::Config;
use crate::utils::logger::module;

/// Cumulative token fees reported by GMGN, in SOL.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GmgnTokenFees {
    pub total_fee: Option<f64>,
    pub trade_fee: Option<f64>,
}

/// Whether a GMGN API key is configured (config or `GMGN_API_KEY` env).
pub fn has_gmgn_api_key(config: &Config) -> bool {
    config
        .gmgn
        .api_key
        .as_deref()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
        || std::env::var("GMGN_API_KEY")
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false)
}

fn api_key(config: &Config) -> Option<String> {
    config
        .gmgn
        .api_key
        .as_deref()
        .filter(|k| !k.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("GMGN_API_KEY")
                .ok()
                .filter(|k| !k.trim().is_empty())
        })
}

/// Process-wide pacer to keep GMGN requests under the configured delay.
fn pacer() -> &'static Mutex<Option<Instant>> {
    static PACER: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    PACER.get_or_init(|| Mutex::new(None))
}

/// Throttle so consecutive requests are at least `delay_ms` apart.
async fn pace_request(delay_ms: u64) {
    if delay_ms == 0 {
        return;
    }
    let delay = Duration::from_millis(delay_ms);
    let mut last = pacer().lock().await;
    if let Some(prev) = *last {
        let elapsed = prev.elapsed();
        if elapsed < delay {
            sleep(delay - elapsed).await;
        }
    }
    *last = Some(Instant::now());
}

/// Generate a unique-ish client id (replaces JS `randomUUID`). GMGN only needs
/// it to be unique per request, not cryptographically random.
fn client_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("rs-{:x}-{:x}", ts, seq)
}

fn num(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Low-level GMGN GET with rate limiting and 429/ban backoff.
async fn gmgn_fetch(
    config: &Config,
    pathname: &str,
    params: &[(&str, String)],
) -> anyhow::Result<Value> {
    let key = api_key(config)
        .ok_or_else(|| anyhow::anyhow!("GMGN_API_KEY is required for the GMGN fee source"))?;
    let base = config.gmgn.base_url.trim_end_matches('/');
    let max_retries = config.gmgn.max_retries;
    let client = reqwest::Client::new();

    for attempt in 0..=max_retries {
        pace_request(config.gmgn.request_delay_ms).await;

        let mut req = client
            .get(format!("{}{}", base, pathname))
            .header("X-APIKEY", &key)
            .header("Content-Type", "application/json")
            .query(&[
                ("timestamp", (chrono::Utc::now().timestamp()).to_string()),
                ("client_id", client_id()),
            ]);
        for (k, v) in params {
            req = req.query(&[(*k, v)]);
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                if attempt < max_retries {
                    continue;
                }
                return Err(anyhow::anyhow!("GMGN {} request failed: {}", pathname, e));
            }
        };

        let status = resp.status();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<f64>().ok());
        let text = resp.text().await.unwrap_or_default();
        let payload: Value =
            serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({ "raw": text }));

        if status.is_success() {
            return Ok(payload);
        }

        let message = payload
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| payload.get("error").and_then(Value::as_str))
            .or_else(|| payload.get("raw").and_then(Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| format!("GMGN {} {}", pathname, status));

        let lower = message.to_ascii_lowercase();
        let rate_limited = status.as_u16() == 429
            || lower.contains("rate limit")
            || lower.contains("temporarily banned");

        if rate_limited && attempt < max_retries {
            let backoff_ms = if let Some(secs) = retry_after {
                (secs * 1000.0) as u64
            } else if lower.contains("temporarily banned") {
                60_000
            } else {
                (3_000u64 * 2u64.pow(attempt)).min(30_000)
            };
            sleep(Duration::from_millis(backoff_ms)).await;
            continue;
        }

        return Err(anyhow::anyhow!(message));
    }

    Err(anyhow::anyhow!("GMGN {} failed", pathname))
}

/// Fetch cumulative token fees (SOL) for the `minTokenFeesSol` gate.
///
/// Returns `None` on missing key or any error so callers fall back to the
/// pool/Jupiter fee figure (matching the original JS behavior).
pub async fn get_gmgn_token_fees(mint: &str, config: &Config) -> Option<GmgnTokenFees> {
    if mint.is_empty() || !has_gmgn_api_key(config) {
        return None;
    }

    let params = [("chain", "sol".to_string()), ("address", mint.to_string())];
    match gmgn_fetch(config, "/v1/token/info", &params).await {
        Ok(payload) => {
            // payload.data.data || payload.data || payload
            let info = payload
                .get("data")
                .and_then(|d| d.get("data"))
                .or_else(|| payload.get("data"))
                .unwrap_or(&payload);
            if !info.is_object() {
                return None;
            }
            Some(GmgnTokenFees {
                total_fee: info.get("total_fee").and_then(num),
                trade_fee: info.get("trade_fee").and_then(num),
            })
        }
        Err(e) => {
            let short: String = mint.chars().take(8).collect();
            module::warn(
                "gmgn",
                &format!("token fees lookup failed for {}: {}", short, e),
            );
            None
        }
    }
}

/// Token security snapshot from GMGN's `/v1/token/security`. Field names match
/// the live response (honeypot/can_not_sell/blacklist are int 0/1; renounce
/// flags are bool). Booleans default to the SAFE value when a field is missing
/// so a partial/absent response never hard-blocks a deploy.
#[derive(Debug, Clone)]
pub struct TokenSecurity {
    pub honeypot: bool,
    pub cannot_sell: bool,
    pub blacklist: bool,
    pub renounced_mint: bool,
    pub renounced_freeze: bool,
    pub top_10_holder_rate: f64,
}

/// Fetch GMGN token-security metrics for a mint. Returns `None` on missing key
/// or any error (caller should allow the deploy — don't block on a data gap).
pub async fn get_token_security(mint: &str, config: &Config) -> Option<TokenSecurity> {
    if mint.is_empty() || !has_gmgn_api_key(config) {
        return None;
    }
    let params = [("chain", "sol".to_string()), ("address", mint.to_string())];
    match gmgn_fetch(config, "/v1/token/security", &params).await {
        Ok(payload) => {
            let info = payload
                .get("data")
                .and_then(|d| d.get("data"))
                .or_else(|| payload.get("data"))
                .unwrap_or(&payload);
            if !info.is_object() {
                return None;
            }
            // Accept bool, "yes"/"true"/"1", or a non-zero number as true;
            // missing → the safe default for that field.
            let flag = |k: &str, default: bool| -> bool {
                match info.get(k) {
                    Some(Value::Bool(b)) => *b,
                    Some(Value::String(s)) => {
                        let s = s.to_lowercase();
                        s == "yes" || s == "true" || s == "1"
                    }
                    Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
                    _ => default,
                }
            };
            Some(TokenSecurity {
                honeypot: flag("honeypot", false) || flag("is_honeypot", false),
                cannot_sell: flag("can_not_sell", false),
                blacklist: flag("blacklist", false) || flag("is_blacklist", false),
                // Authority renounce: default TRUE when absent so a data gap
                // doesn't block; only an explicit false gates.
                renounced_mint: flag("renounced_mint", true),
                renounced_freeze: flag("renounced_freeze_account", true),
                top_10_holder_rate: info.get("top_10_holder_rate").and_then(num).unwrap_or(0.0),
            })
        }
        Err(e) => {
            let short: String = mint.chars().take(8).collect();
            module::warn(
                "gmgn",
                &format!("security lookup failed for {}: {}", short, e),
            );
            None
        }
    }
}

/// Wallet tags GMGN uses to mark high-quality "smart money" holders/traders.
/// The `/token_top_traders` payload uses `bluechip_owner` / `diamond_hands` /
/// `whale`; `renowned` / `kol` / `smart_degen` show up on stronger wallets too.
const SMART_MONEY_TAGS: &[&str] = &[
    "renowned",
    "kol",
    "smart_degen",
    "smart_money",
    "bluechip_owner",
    "diamond_hands",
    "whale",
];

/// Count how many of a token's top traders GMGN tags as "smart money"
/// (smart_degen / renowned / etc.). A screening quality signal: smart money
/// backing a token → less likely to be a rug, better to LP against. Returns
/// `None` on missing key/error (caller treats as a neutral/absent signal).
pub async fn get_smart_money_count(mint: &str, config: &Config) -> Option<u32> {
    if mint.is_empty() || !has_gmgn_api_key(config) {
        return None;
    }
    let params = [("chain", "sol".to_string()), ("address", mint.to_string())];
    match gmgn_fetch(config, "/v1/market/token_top_traders", &params).await {
        Ok(payload) => {
            let list = payload
                .get("data")
                .and_then(|d| d.get("list"))
                .and_then(Value::as_array);
            let Some(list) = list else {
                return Some(0);
            };
            let mut count = 0u32;
            for it in list {
                // Tags can live in any of these fields, as arrays or strings —
                // stringify and substring-match to stay robust to the shape.
                let blob = ["tags", "maker_token_tags", "wallet_tag_v2"]
                    .iter()
                    .filter_map(|k| it.get(*k).map(|v| v.to_string()))
                    .collect::<String>();
                let suspicious = it
                    .get("is_suspicious")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if !suspicious && SMART_MONEY_TAGS.iter().any(|t| blob.contains(t)) {
                    count += 1;
                }
            }
            Some(count)
        }
        Err(e) => {
            let short: String = mint.chars().take(8).collect();
            module::warn(
                "gmgn",
                &format!("smart-money lookup failed for {}: {}", short, e),
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_key_reads_config() {
        let mut config = Config::default();
        assert!(!has_gmgn_api_key(&config));
        config.gmgn.api_key = Some("  ".to_string());
        assert!(!has_gmgn_api_key(&config));
        config.gmgn.api_key = Some("abc123".to_string());
        assert!(has_gmgn_api_key(&config));
    }

    #[test]
    fn num_coerces_strings_and_numbers() {
        assert_eq!(num(&serde_json::json!(1.5)), Some(1.5));
        assert_eq!(num(&serde_json::json!("2.25")), Some(2.25));
        assert_eq!(num(&serde_json::json!("nan-ish")), None);
        assert_eq!(num(&serde_json::json!(null)), None);
    }

    #[test]
    fn client_id_is_unique() {
        assert_ne!(client_id(), client_id());
    }
}
