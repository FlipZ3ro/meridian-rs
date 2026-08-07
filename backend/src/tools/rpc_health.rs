//! RPC endpoint health: watch the active Solana RPC, shout when it dies, and
//! fail over to a spare.
//!
//! A dead RPC does not degrade the bot, it blinds it. On 2026-08-06 the provider
//! ran out of quota and the poller spent seven hours detecting exits it could not
//! execute: one position drifted from +22% to -40% while its close was retried
//! 505 times, and nothing about it ever reached the operator. Hence a heartbeat
//! that alerts on the transition (once, not every tick) and a spare endpoint to
//! rotate onto, so the next outage costs a minute instead of a session.

use crate::config::Config;
use crate::utils::logger::module::{error, info};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

/// Index into `endpoints()` of the endpoint currently in use.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
/// Health checks failed back-to-back. Any success resets it.
static FAILURES: AtomicU32 = AtomicU32::new(0);
/// Set once the operator has been told the RPC is down, so an outage is
/// announced a single time and recovery is reported exactly once.
static ALERTED: AtomicU32 = AtomicU32::new(0);

/// Failures required before failing over. Three at a 30s beat catches a real
/// outage in ~90s while a single blip is not enough to move off a working
/// endpoint.
const FAILURES_BEFORE_ROTATE: u32 = 3;
const CHECK_INTERVAL_SECS: u64 = 30;

/// Every endpoint available, primary first. Spares come from `rpcFallbackUrls`
/// in config or the `RPC_FALLBACK_URLS` env var (comma-separated).
pub fn endpoints(config: &Config) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(primary) = config
        .api
        .helius_rpc_url
        .clone()
        .or_else(|| std::env::var("HELIUS_RPC_URL").ok())
        .or_else(|| std::env::var("RPC_URL").ok())
        .filter(|v| !v.trim().is_empty())
    {
        out.push(primary.trim().to_string());
    }
    let from_env: Vec<String> = std::env::var("RPC_FALLBACK_URLS")
        .ok()
        .map(|raw| raw.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();
    for url in config
        .api
        .rpc_fallback_urls
        .clone()
        .into_iter()
        .chain(from_env)
    {
        let url = url.trim().to_string();
        if !url.is_empty() && !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// The endpoint callers should use right now. `None` when nothing is configured,
/// which leaves the caller on its own default.
pub fn active_url(config: &Config) -> Option<String> {
    let list = endpoints(config);
    if list.is_empty() {
        return None;
    }
    Some(list[ACTIVE.load(Ordering::Relaxed) % list.len()].clone())
}

/// Advance to the next endpoint, returning its index. `None` when there is
/// nowhere to go — a lone endpoint cannot fail over to itself.
fn rotate(config: &Config) -> Option<usize> {
    let len = endpoints(config).len();
    if len < 2 {
        return None;
    }
    Some((ACTIVE.fetch_add(1, Ordering::Relaxed) + 1) % len)
}

/// Trim the api-key so an endpoint can be named in a Telegram message without
/// leaking the credential.
fn redact(url: &str) -> String {
    match url.split_once("api-key=") {
        Some((head, tail)) => {
            let keep: String = tail.chars().take(8).collect();
            format!("{head}api-key={keep}…")
        }
        None => url.to_string(),
    }
}

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

/// One `getHealth` round-trip. Status is what decides: a 429 still returns a
/// readable body, so only a 2xx counts as alive.
async fn probe(client: &reqwest::Client, url: &str) -> Result<(), String> {
    let resp = client
        .post(url)
        .header("content-type", "application/json")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"getHealth"}"#)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", status.as_u16()))
    }
}

/// Background heartbeat, running for the life of the process.
pub async fn monitor(config: Config) {
    let count = endpoints(&config).len();
    if count == 0 {
        info("rpc", "No RPC endpoint configured — health monitor idle");
        return;
    }
    info(
        "rpc",
        &format!("RPC health monitor started — {count} endpoint(s), checking every {CHECK_INTERVAL_SECS}s"),
    );

    let client = reqwest::Client::new();
    let mut ticker = tokio::time::interval(Duration::from_secs(CHECK_INTERVAL_SECS));
    ticker.tick().await; // the first tick is immediate; skip it

    loop {
        ticker.tick().await;
        let Some(url) = active_url(&config) else {
            continue;
        };
        match probe(&client, &url).await {
            Ok(()) => {
                FAILURES.store(0, Ordering::Relaxed);
                if ALERTED.swap(0, Ordering::Relaxed) == 1 {
                    info("rpc", &format!("RPC recovered on {}", redact(&url)));
                    notify(
                        &config,
                        &format!("✅ *RPC PULIH*\n`{}`", redact(&url)),
                    )
                    .await;
                }
            }
            Err(reason) => {
                let n = FAILURES.fetch_add(1, Ordering::Relaxed) + 1;
                error(
                    "rpc",
                    &format!(
                        "health check failed ({reason}) — {n} in a row on {}",
                        redact(&url)
                    ),
                );
                if n < FAILURES_BEFORE_ROTATE {
                    continue;
                }
                FAILURES.store(0, Ordering::Relaxed);
                let moved_to = rotate(&config);
                // Announce once per outage. Rotating still logs, so a silent
                // failover is visible without paging the operator again.
                if ALERTED.swap(1, Ordering::Relaxed) == 0 {
                    let tail = match moved_to {
                        Some(idx) => {
                            let list = endpoints(&config);
                            info("rpc", &format!("failing over to {}", redact(&list[idx])));
                            format!("\npindah ke: `{}`", redact(&list[idx]))
                        }
                        None => {
                            "\n⚠️ tidak ada endpoint cadangan — bot buta sampai ini beres".to_string()
                        }
                    };
                    notify(
                        &config,
                        &format!("🚨 *RPC MATI*\n`{}`\nsebab: {reason}{tail}", redact(&url)),
                    )
                    .await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(primary: Option<&str>, spares: &[&str]) -> Config {
        Config {
            api: crate::config::types::ApiConfig {
                helius_rpc_url: primary.map(|s| s.to_string()),
                rpc_fallback_urls: spares.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn endpoints_put_primary_first_and_drop_duplicates() {
        let c = cfg(
            Some("https://a.test"),
            &["https://b.test", "https://a.test", ""],
        );
        assert_eq!(
            endpoints(&c),
            vec!["https://a.test".to_string(), "https://b.test".to_string()]
        );
    }

    #[test]
    fn rotate_is_a_no_op_without_a_spare() {
        let c = cfg(Some("https://only.test"), &[]);
        assert_eq!(rotate(&c), None);
    }

    #[test]
    fn redact_keeps_the_host_and_hides_the_key() {
        let out = redact("https://rpc.test/?api-key=abcdefgh12345678");
        assert_eq!(out, "https://rpc.test/?api-key=abcdefgh…");
        assert_eq!(redact("https://plain.test"), "https://plain.test");
    }
}
