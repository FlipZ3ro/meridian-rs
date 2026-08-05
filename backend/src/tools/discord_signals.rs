use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::meridian_data_path;
use crate::models::pool::{RawPool, TokenX, TokenY};

pub const DISCORD_SIGNALS_FILE: &str = "discord-signals.json";
const MAX_SIGNALS: usize = 100;
const DEDUP_WINDOW_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscordSignalStatus {
    #[default]
    Pending,
    Processed,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DiscordSignalRecord {
    pub id: String,
    pub pool_address: String,
    pub base_mint: String,
    pub base_symbol: String,
    #[serde(default = "default_signal_source")]
    pub signal_source: String,
    #[serde(default)]
    pub discord_guild: String,
    #[serde(default)]
    pub discord_channel: String,
    #[serde(default)]
    pub discord_author: String,
    #[serde(default)]
    pub discord_message_snippet: String,
    pub queued_at: String,
    #[serde(default)]
    pub rug_score: Option<f64>,
    #[serde(default)]
    pub total_fees_sol: Option<f64>,
    #[serde(default)]
    pub token_age_minutes: Option<i64>,
    #[serde(default)]
    pub status: DiscordSignalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery_pool: Option<RawPool>,
}

fn default_signal_source() -> String {
    "discord".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordSignalListItem {
    pub id: String,
    pub symbol: String,
    pub pool: String,
    pub author: String,
    pub channel: String,
    pub queued_at: String,
    pub rug_score: Option<f64>,
    pub status: DiscordSignalStatus,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordSignalSummary {
    pub count: usize,
    pub pending: usize,
    pub processed: usize,
    pub signals: Vec<DiscordSignalListItem>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordSignalClearResult {
    pub cleared: usize,
    pub remaining: usize,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct DiscordSignalStore {
    pub path: PathBuf,
    pub signals: Vec<DiscordSignalRecord>,
}

impl DiscordSignalStore {
    pub fn default_path() -> PathBuf {
        meridian_data_path(DISCORD_SIGNALS_FILE)
    }

    pub fn load_default() -> Result<Self> {
        Self::load_at(Self::default_path())
    }

    pub fn load_at(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(Self {
                path,
                signals: Vec::new(),
            });
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let signals = if content.trim().is_empty() {
            Vec::new()
        } else {
            serde_json::from_str::<Vec<DiscordSignalRecord>>(&content)
                .with_context(|| format!("failed to parse {}", path.display()))?
        };
        Ok(Self { path, signals })
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(&self.signals)?;
        fs::write(&self.path, json)
            .with_context(|| format!("failed to write {}", self.path.display()))
    }

    pub fn queue(&mut self, record: DiscordSignalRecord) -> Result<()> {
        self.signals.insert(0, record);
        self.signals.truncate(MAX_SIGNALS);
        self.save()
    }

    pub fn pending(&self) -> Vec<&DiscordSignalRecord> {
        self.signals
            .iter()
            .filter(|signal| signal.status == DiscordSignalStatus::Pending)
            .collect()
    }

    pub fn pending_cloned(&self) -> Vec<DiscordSignalRecord> {
        self.pending().into_iter().cloned().collect()
    }

    pub fn clear_processed(&mut self) -> Result<DiscordSignalClearResult> {
        let before = self.signals.len();
        self.signals
            .retain(|signal| signal.status == DiscordSignalStatus::Pending);
        self.save()?;
        Ok(DiscordSignalClearResult {
            cleared: before.saturating_sub(self.signals.len()),
            remaining: self.signals.len(),
            path: self.path.display().to_string(),
        })
    }

    pub fn summary(&self) -> DiscordSignalSummary {
        let pending = self
            .signals
            .iter()
            .filter(|signal| signal.status == DiscordSignalStatus::Pending)
            .count();
        let processed = self.signals.len().saturating_sub(pending);
        DiscordSignalSummary {
            count: self.signals.len(),
            pending,
            processed,
            signals: self
                .signals
                .iter()
                .map(|signal| DiscordSignalListItem {
                    id: signal.id.clone(),
                    symbol: signal.base_symbol.clone(),
                    pool: signal.pool_address.clone(),
                    author: signal.discord_author.clone(),
                    channel: signal.discord_channel.clone(),
                    queued_at: signal.queued_at.clone(),
                    rug_score: signal.rug_score,
                    status: signal.status.clone(),
                    snippet: signal.discord_message_snippet.chars().take(60).collect(),
                })
                .collect(),
            path: self.path.display().to_string(),
            message: if self.signals.is_empty() {
                Some("No discord-signals.json found or queue is empty. Is the listener/importer running?".to_string())
            } else {
                None
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PrecheckResolvedPool {
    pub pool_address: String,
    pub base_mint: String,
    pub symbol: Option<String>,
    pub source: String,
    pub token_age_minutes: Option<i64>,
    pub rug_score: Option<f64>,
    pub total_fees_sol: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery_pool: Option<RawPool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PrecheckResult {
    pub pass: bool,
    pub reason: Option<String>,
    pub pool_address: Option<String>,
    pub base_mint: Option<String>,
    pub symbol: Option<String>,
    pub source: Option<String>,
    pub rug_score: Option<f64>,
    pub total_fees_sol: Option<f64>,
    pub token_age_minutes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery_pool: Option<RawPool>,
}

impl PrecheckResult {
    fn rejected(reason: impl Into<String>) -> Self {
        Self {
            pass: false,
            reason: Some(reason.into()),
            pool_address: None,
            base_mint: None,
            symbol: None,
            source: None,
            rug_score: None,
            total_fees_sol: None,
            token_age_minutes: None,
            discovery_pool: None,
        }
    }

    fn passed(resolved: PrecheckResolvedPool) -> Self {
        Self {
            pass: true,
            reason: None,
            pool_address: Some(resolved.pool_address),
            base_mint: Some(resolved.base_mint),
            symbol: resolved.symbol,
            source: Some(resolved.source),
            rug_score: resolved.rug_score,
            total_fees_sol: resolved.total_fees_sol,
            token_age_minutes: resolved.token_age_minutes,
            discovery_pool: resolved.discovery_pool,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiscordPrecheckPipeline {
    recent_seen: HashMap<String, i64>,
}

impl DiscordPrecheckPipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn run_offline(
        &mut self,
        address: &str,
        resolved: Option<PrecheckResolvedPool>,
        blacklisted_mints: &HashSet<String>,
        min_fees_sol: f64,
    ) -> PrecheckResult {
        self.run_offline_at(
            address,
            resolved,
            blacklisted_mints,
            min_fees_sol,
            chrono::Utc::now().timestamp_millis(),
        )
    }

    pub fn run_offline_at(
        &mut self,
        address: &str,
        resolved: Option<PrecheckResolvedPool>,
        blacklisted_mints: &HashSet<String>,
        min_fees_sol: f64,
        now_ms: i64,
    ) -> PrecheckResult {
        self.recent_seen
            .retain(|_, seen_at| now_ms.saturating_sub(*seen_at) <= DEDUP_WINDOW_MS);
        if self.recent_seen.contains_key(address) {
            return PrecheckResult::rejected("dedup: seen in last 10 minutes");
        }
        self.recent_seen.insert(address.to_string(), now_ms);

        if blacklisted_mints.contains(address) {
            return PrecheckResult::rejected(format!("blacklisted: {address}"));
        }

        let Some(resolved) = resolved else {
            return PrecheckResult::rejected("no Meteora DLMM pool found for this token");
        };

        if blacklisted_mints.contains(&resolved.base_mint) {
            return PrecheckResult::rejected(format!("blacklisted: {}", resolved.base_mint));
        }

        if let Some(total_fees_sol) = resolved.total_fees_sol {
            if total_fees_sol < min_fees_sol {
                return PrecheckResult::rejected(format!(
                    "global fees too low: {:.2} SOL < {} SOL threshold",
                    total_fees_sol, min_fees_sol
                ));
            }
        }

        PrecheckResult::passed(resolved)
    }
}

pub fn extract_solana_addresses(text: &str) -> Vec<String> {
    let regex = Regex::new(r"[1-9A-HJ-NP-Za-km-z]{32,44}").expect("valid Solana regex");
    let mut seen = HashSet::new();
    let mut addresses = Vec::new();
    for matched in regex.find_iter(text).map(|m| m.as_str()) {
        if is_likely_solana_address(matched) && seen.insert(matched.to_string()) {
            addresses.push(matched.to_string());
        }
    }
    addresses
}

pub fn is_likely_solana_address(value: &str) -> bool {
    const FALSE_POSITIVE_SKIP: &[&str] = &["solana", "meteora", "jupiter", "raydium", "orca"];
    let trimmed = value.trim();
    if !(32..=44).contains(&trimmed.len()) {
        return false;
    }
    if FALSE_POSITIVE_SKIP
        .iter()
        .any(|word| trimmed.eq_ignore_ascii_case(word))
    {
        return false;
    }
    trimmed.chars().any(|ch| ch.is_ascii_digit())
}

pub fn record_from_precheck(
    address: &str,
    result: PrecheckResult,
    guild: &str,
    channel: &str,
    author: &str,
    snippet: &str,
) -> Option<DiscordSignalRecord> {
    if !result.pass {
        return None;
    }
    let pool_address = result.pool_address?;
    let base_mint = result.base_mint?;
    let base_symbol = result.symbol.unwrap_or_else(|| "?".to_string());
    Some(DiscordSignalRecord {
        id: format!(
            "{}-{}",
            address.chars().take(8).collect::<String>(),
            chrono::Utc::now().timestamp_millis()
        ),
        pool_address,
        base_mint,
        base_symbol,
        signal_source: "discord".to_string(),
        discord_guild: empty_to_unknown(guild),
        discord_channel: empty_to_unknown(channel),
        discord_author: empty_to_unknown(author),
        discord_message_snippet: snippet.chars().take(120).collect(),
        queued_at: chrono::Utc::now().to_rfc3339(),
        rug_score: result.rug_score,
        total_fees_sol: result.total_fees_sol,
        token_age_minutes: result.token_age_minutes,
        status: DiscordSignalStatus::Pending,
        discovery_pool: result.discovery_pool,
    })
}

fn empty_to_unknown(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn signal_to_raw_pool(signal: &DiscordSignalRecord) -> Option<RawPool> {
    if signal.pool_address.trim().is_empty() {
        return None;
    }

    let mut pool = signal.discovery_pool.clone().unwrap_or_else(|| RawPool {
        pool_address: Some(signal.pool_address.clone()),
        name: Some(format!("{}/SOL", signal.base_symbol)),
        pool_type: Some("dlmm".to_string()),
        tvl: None,
        active_tvl: None,
        volume: None,
        fee: signal.total_fees_sol,
        fee_active_tvl_ratio: None,
        volatility: None,
        base_token_holders: None,
        base_token_mcap: None,
        base_token_organic_score: None,
        quote_token_organic_score: None,
        base_token_has_critical_warnings: Some(false),
        quote_token_has_critical_warnings: Some(false),
        base_token_has_high_supply_concentration: Some(false),
        base_token_has_high_single_ownership: Some(false),
        base_token_launchpad: None,
        dlmm_bin_step: None,
        token_x: Some(TokenX {
            address: Some(signal.base_mint.clone()),
            symbol: Some(signal.base_symbol.clone()),
            name: Some(signal.base_symbol.clone()),
            market_cap: None,
            organic_score: None,
            launchpad: None,
            launchpad_platform: None,
            created_at: None,
            icon: None,
        }),
        token_y: Some(TokenY {
            address: Some("So11111111111111111111111111111111111111112".to_string()),
            symbol: Some("SOL".to_string()),
            organic_score: None,
        }),
        extra: HashMap::new(),
    });

    mark_pool_from_signal(&mut pool, signal);
    Some(pool)
}

pub fn merge_discord_signal_pools(
    raw_pools: Vec<RawPool>,
    signals: &[DiscordSignalRecord],
    mode: Option<&str>,
) -> Vec<RawPool> {
    let signal_pools: Vec<RawPool> = signals
        .iter()
        .filter(|signal| signal.status == DiscordSignalStatus::Pending)
        .filter_map(signal_to_raw_pool)
        .collect();

    if matches!(mode, Some(value) if value.eq_ignore_ascii_case("only")) {
        return signal_pools;
    }

    let mut merged = raw_pools;
    for signal_pool in signal_pools {
        let signal_pool_address = signal_pool.pool_address.clone().unwrap_or_default();
        if signal_pool_address.is_empty() {
            continue;
        }
        if let Some(existing) = merged
            .iter_mut()
            .find(|pool| pool.pool_address.as_deref() == Some(signal_pool_address.as_str()))
        {
            merge_signal_metadata(existing, &signal_pool);
        } else {
            merged.push(signal_pool);
        }
    }
    merged
}

fn mark_pool_from_signal(pool: &mut RawPool, signal: &DiscordSignalRecord) {
    if pool.pool_address.as_deref().unwrap_or_default().is_empty() {
        pool.pool_address = Some(signal.pool_address.clone());
    }
    if pool.name.as_deref().unwrap_or_default().is_empty() {
        pool.name = Some(format!("{}/SOL", signal.base_symbol));
    }
    if pool.pool_type.is_none() {
        pool.pool_type = Some("dlmm".to_string());
    }
    if pool.fee.is_none() {
        pool.fee = signal.total_fees_sol;
    }
    ensure_token_x(pool, signal);
    pool.extra.insert("discord_signal".to_string(), json!(true));
    pool.extra
        .insert("discord_signal_id".to_string(), json!(signal.id));
    pool.extra.insert(
        "discord_signal_author".to_string(),
        json!(signal.discord_author),
    );
    pool.extra.insert(
        "discord_signal_channel".to_string(),
        json!(signal.discord_channel),
    );
    pool.extra.insert(
        "discord_signal_queued_at".to_string(),
        json!(signal.queued_at),
    );
    if let Some(rug_score) = signal.rug_score {
        pool.extra
            .insert("discord_signal_rug_score".to_string(), json!(rug_score));
    }
    if let Some(token_age_minutes) = signal.token_age_minutes {
        pool.extra.insert(
            "discord_signal_token_age_minutes".to_string(),
            json!(token_age_minutes),
        );
    }
}

fn ensure_token_x(pool: &mut RawPool, signal: &DiscordSignalRecord) {
    match pool.token_x.as_mut() {
        Some(token) => {
            if token.address.as_deref().unwrap_or_default().is_empty() {
                token.address = Some(signal.base_mint.clone());
            }
            if token.symbol.as_deref().unwrap_or_default().is_empty() {
                token.symbol = Some(signal.base_symbol.clone());
            }
        }
        None => {
            pool.token_x = Some(TokenX {
                address: Some(signal.base_mint.clone()),
                symbol: Some(signal.base_symbol.clone()),
                name: Some(signal.base_symbol.clone()),
                market_cap: pool.base_token_mcap,
                organic_score: pool.base_token_organic_score,
                launchpad: pool.base_token_launchpad.clone(),
                launchpad_platform: None,
                created_at: None,
                icon: None,
            });
        }
    }
}

fn merge_signal_metadata(existing: &mut RawPool, signal_pool: &RawPool) {
    for (key, value) in &signal_pool.extra {
        if key.starts_with("discord_signal") {
            existing.extra.insert(key.clone(), value.clone());
        }
    }
    if existing.token_x.is_none() {
        existing.token_x = signal_pool.token_x.clone();
    }
    if existing.fee.is_none() {
        existing.fee = signal_pool.fee;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "meridian-discord-signals-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("test dir");
        dir
    }

    fn raw_pool(pool: &str, mint: &str, symbol: &str, score_boost: f64) -> RawPool {
        RawPool {
            pool_address: Some(pool.to_string()),
            name: Some(format!("{symbol}/SOL")),
            pool_type: Some("dlmm".to_string()),
            tvl: Some(25_000.0 + score_boost),
            active_tvl: Some(25_000.0 + score_boost),
            volume: Some(2_500.0 + score_boost),
            fee: Some(9.0),
            fee_active_tvl_ratio: Some(0.12),
            volatility: Some(0.4),
            base_token_holders: Some(900),
            base_token_mcap: Some(500_000.0),
            base_token_organic_score: Some(80.0),
            quote_token_organic_score: Some(90.0),
            base_token_has_critical_warnings: Some(false),
            quote_token_has_critical_warnings: Some(false),
            base_token_has_high_supply_concentration: Some(false),
            base_token_has_high_single_ownership: Some(false),
            base_token_launchpad: Some("meteora".to_string()),
            dlmm_bin_step: Some(100),
            token_x: Some(TokenX {
                address: Some(mint.to_string()),
                symbol: Some(symbol.to_string()),
                name: Some(symbol.to_string()),
                market_cap: Some(500_000.0),
                organic_score: Some(80.0),
                launchpad: Some("meteora".to_string()),
                launchpad_platform: None,
                created_at: None,
                icon: None,
            }),
            token_y: Some(TokenY {
                address: Some("So11111111111111111111111111111111111111112".to_string()),
                symbol: Some("SOL".to_string()),
                organic_score: Some(90.0),
            }),
            extra: HashMap::new(),
        }
    }

    fn signal_with_pool(id: &str, pool: RawPool) -> DiscordSignalRecord {
        DiscordSignalRecord {
            id: id.to_string(),
            pool_address: pool.pool_address.clone().unwrap_or_default(),
            base_mint: pool
                .token_x
                .as_ref()
                .and_then(|token| token.address.clone())
                .unwrap_or_default(),
            base_symbol: pool
                .token_x
                .as_ref()
                .and_then(|token| token.symbol.clone())
                .unwrap_or_else(|| "?".to_string()),
            signal_source: "discord".to_string(),
            discord_guild: "LP Army".to_string(),
            discord_channel: "alpha".to_string(),
            discord_author: "Metlex Pool Bot".to_string(),
            discord_message_snippet: "fresh pool".to_string(),
            queued_at: "2026-06-09T00:00:00Z".to_string(),
            rug_score: Some(42.0),
            total_fees_sol: Some(31.0),
            token_age_minutes: Some(15),
            status: DiscordSignalStatus::Pending,
            discovery_pool: Some(pool),
        }
    }

    #[test]
    fn extracts_unique_likely_solana_addresses_from_message_and_embeds() {
        let addr = "So11111111111111111111111111111111111111112";
        let text = format!("pool {addr} duplicate {addr} solana plainword");

        let found = extract_solana_addresses(&text);

        assert_eq!(found, vec![addr.to_string()]);
    }

    #[test]
    fn queue_keeps_newest_first_caps_to_100_and_clears_processed() {
        let dir = test_dir("queue");
        let path = dir.join("discord-signals.json");
        let mut store = DiscordSignalStore::load_at(&path).expect("store loads");

        for idx in 0..105 {
            let mut signal = signal_with_pool(
                &format!("sig-{idx}"),
                raw_pool(
                    &format!("Pool{idx}"),
                    &format!("Mint{idx}"),
                    "SIG",
                    idx as f64,
                ),
            );
            if idx % 10 == 0 {
                signal.status = DiscordSignalStatus::Processed;
            }
            store.queue(signal).expect("signal queued");
        }

        let reloaded = DiscordSignalStore::load_at(&path).expect("store reloads");
        assert_eq!(reloaded.signals.len(), 100);
        assert_eq!(reloaded.signals[0].id, "sig-104");
        assert!(!reloaded.signals.iter().any(|signal| signal.id == "sig-0"));

        let mut reloaded = reloaded;
        let clear = reloaded.clear_processed().expect("processed cleared");
        assert!(clear.cleared > 0);
        assert_eq!(clear.remaining, reloaded.pending().len());
        assert!(reloaded
            .signals
            .iter()
            .all(|signal| signal.status == DiscordSignalStatus::Pending));
    }

    #[test]
    fn precheck_rejects_dedup_blacklist_missing_pool_and_low_fees() {
        let mut pipeline = DiscordPrecheckPipeline::new();
        let address = "So11111111111111111111111111111111111111112";
        let mut blacklist = HashSet::new();
        blacklist.insert("BlockedMint".to_string());

        let missing = pipeline.run_offline(address, None, &blacklist, 30.0);
        assert!(!missing.pass);
        assert!(missing.reason.unwrap().contains("no Meteora DLMM pool"));

        let resolved = PrecheckResolvedPool {
            pool_address: "PoolOk".to_string(),
            base_mint: "MintOk".to_string(),
            symbol: Some("OK".to_string()),
            source: "test".to_string(),
            token_age_minutes: Some(12),
            rug_score: Some(5.0),
            total_fees_sol: Some(40.0),
            discovery_pool: Some(raw_pool("PoolOk", "MintOk", "OK", 0.0)),
        };
        let pass = pipeline.run_offline(
            "Another1111111111111111111111111111111111",
            Some(resolved.clone()),
            &blacklist,
            30.0,
        );
        assert!(pass.pass);
        let dedup = pipeline.run_offline(
            "Another1111111111111111111111111111111111",
            Some(resolved.clone()),
            &blacklist,
            30.0,
        );
        assert!(!dedup.pass);
        assert!(dedup.reason.unwrap().contains("dedup"));

        let blocked = PrecheckResolvedPool {
            base_mint: "BlockedMint".to_string(),
            ..resolved.clone()
        };
        let blocked_result = pipeline.run_offline(
            "Third11111111111111111111111111111111111",
            Some(blocked),
            &blacklist,
            30.0,
        );
        assert!(!blocked_result.pass);
        assert!(blocked_result.reason.unwrap().contains("blacklisted"));

        let low_fee = PrecheckResolvedPool {
            total_fees_sol: Some(2.0),
            ..resolved
        };
        let low_fee_result = pipeline.run_offline(
            "Fourth1111111111111111111111111111111111",
            Some(low_fee),
            &blacklist,
            30.0,
        );
        assert!(!low_fee_result.pass);
        assert!(low_fee_result
            .reason
            .unwrap()
            .contains("global fees too low"));
    }

    #[test]
    fn screening_merge_honors_discord_signal_only_and_merge_modes() {
        let regular = raw_pool("PoolRegular", "MintRegular", "REG", 10.0);
        let existing_signal = signal_with_pool(
            "sig-existing",
            raw_pool("PoolRegular", "MintRegular", "REG", 50.0),
        );
        let discord_only = signal_with_pool(
            "sig-new",
            raw_pool("PoolDiscord", "MintDiscord", "DISC", 100.0),
        );

        let only = merge_discord_signal_pools(
            vec![regular.clone()],
            &[existing_signal.clone(), discord_only.clone()],
            Some("only"),
        );
        assert_eq!(only.len(), 2);
        assert!(only.iter().all(|pool| pool
            .extra
            .get("discord_signal")
            .and_then(|value| value.as_bool())
            == Some(true)));

        let merged = merge_discord_signal_pools(
            vec![regular],
            &[existing_signal, discord_only],
            Some("merge"),
        );
        assert_eq!(merged.len(), 2);
        let regular = merged
            .iter()
            .find(|pool| pool.pool_address.as_deref() == Some("PoolRegular"))
            .unwrap();
        assert_eq!(
            regular
                .extra
                .get("discord_signal")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert!(merged
            .iter()
            .any(|pool| pool.pool_address.as_deref() == Some("PoolDiscord")));
    }
}
