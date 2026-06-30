use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::config::types::ManagementConfig;

const LOW_YIELD_COOLDOWN_HOURS: u32 = 4;
// Repeat-loss guard: a base token that closes at a loss this many times (across
// ALL its pools) while net-negative is a systematic bleeder — cool it down hard
// at the token level so the screener stops re-deploying into it.
const REPEAT_LOSS_TRIGGER: usize = 2;
const REPEAT_LOSS_COOLDOWN_HOURS: u32 = 24;
const REPEAT_LOSS_THRESHOLD_PCT: f64 = -1.0; // ignore ~breakeven closes
const MAX_NOTE_LENGTH: usize = 280;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PoolEntry {
    #[serde(default)]
    pub pool_address: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_mint: String,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub notes: Vec<PoolNote>,
    #[serde(default)]
    pub deploys: Vec<PoolDeployRecord>,
    #[serde(default)]
    pub total_deploys: u32,
    #[serde(default)]
    pub avg_pnl_pct: f64,
    #[serde(default)]
    pub win_rate: f64,
    #[serde(default)]
    pub adjusted_win_rate: f64,
    #[serde(default)]
    pub adjusted_win_rate_sample_count: u32,
    #[serde(default)]
    pub last_deployed_at: Option<String>,
    #[serde(default)]
    pub last_outcome: Option<String>,
    #[serde(default)]
    pub last_yield: Option<String>,
    #[serde(default)]
    pub last_close_reason: Option<String>,
    #[serde(default)]
    pub last_close_pnl: Option<f64>,
    #[serde(default)]
    pub last_close_at: Option<String>,
    #[serde(default)]
    pub total_positions: u32,
    #[serde(default)]
    pub total_fees_earned: f64,
    #[serde(default)]
    pub on_cooldown: bool,
    #[serde(default)]
    pub cooldown_until: Option<String>,
    #[serde(default)]
    pub cooldown_reason: Option<String>,
    #[serde(default)]
    pub base_mint_cooldown_until: Option<String>,
    #[serde(default)]
    pub base_mint_cooldown_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PoolDeployRecord {
    #[serde(default)]
    pub deployed_at: Option<String>,
    #[serde(default)]
    pub closed_at: Option<String>,
    #[serde(default)]
    pub pnl_pct: Option<f64>,
    #[serde(default)]
    pub pnl_usd: Option<f64>,
    #[serde(default)]
    pub fees_earned_usd: Option<f64>,
    #[serde(default)]
    pub fees_earned_sol: Option<f64>,
    #[serde(default)]
    pub fee_earned_pct: Option<f64>,
    #[serde(default)]
    pub range_efficiency: Option<f64>,
    #[serde(default)]
    pub minutes_held: Option<f64>,
    #[serde(default)]
    pub close_reason: Option<String>,
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub volatility_at_deploy: Option<f64>,
    #[serde(default)]
    pub entry_mcap: Option<f64>,
    #[serde(default)]
    pub entry_tvl: Option<f64>,
    #[serde(default)]
    pub entry_volume: Option<f64>,
    #[serde(default)]
    pub exit_mcap: Option<f64>,
    #[serde(default)]
    pub exit_tvl: Option<f64>,
    #[serde(default)]
    pub exit_volume: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct PoolDeployInput {
    pub pool_name: Option<String>,
    pub base_mint: Option<String>,
    pub symbol: Option<String>,
    pub deployed_at: Option<String>,
    pub closed_at: Option<String>,
    pub pnl_pct: Option<f64>,
    pub pnl_usd: Option<f64>,
    pub fees_earned_usd: Option<f64>,
    pub fees_earned_sol: Option<f64>,
    pub fee_earned_pct: Option<f64>,
    pub range_efficiency: Option<f64>,
    pub minutes_held: Option<f64>,
    pub close_reason: Option<String>,
    pub strategy: Option<String>,
    pub volatility: Option<f64>,
    pub entry_mcap: Option<f64>,
    pub entry_tvl: Option<f64>,
    pub entry_volume: Option<f64>,
    pub exit_mcap: Option<f64>,
    pub exit_tvl: Option<f64>,
    pub exit_volume: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PoolNote {
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PoolMemoryStore {
    pub pools: HashMap<String, PoolEntry>,
}

impl PoolMemoryStore {
    pub fn load(path: &str) -> Result<Self> {
        if Path::new(path).exists() {
            let content = fs::read_to_string(path)?;
            Ok(serde_json::from_str(&content).unwrap_or_default())
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, path: &str) -> Result<()> {
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn get(&self, pool_address: &str) -> Option<&PoolEntry> {
        self.pools.get(pool_address)
    }

    pub fn add_note(
        &mut self,
        pool_address: &str,
        base_mint: &str,
        symbol: Option<&str>,
        content: &str,
    ) {
        let entry = self.ensure_entry(
            pool_address,
            if base_mint.is_empty() {
                None
            } else {
                Some(base_mint)
            },
            symbol,
            None,
        );
        entry.notes.push(PoolNote {
            content: sanitize_stored_note(content),
            created_at: Utc::now().to_rfc3339(),
        });
    }

    pub fn record_close(
        &mut self,
        pool_address: &str,
        base_mint: &str,
        symbol: Option<&str>,
        reason: &str,
        pnl: f64,
    ) {
        let management = crate::config::types::Config::default().management;
        self.record_deploy(
            pool_address,
            PoolDeployInput {
                base_mint: Some(base_mint.to_string()),
                symbol: symbol.map(String::from),
                close_reason: Some(reason.to_string()),
                pnl_pct: Some(pnl),
                ..Default::default()
            },
            &management,
        );
    }

    pub fn record_deploy(
        &mut self,
        pool_address: &str,
        input: PoolDeployInput,
        management: &ManagementConfig,
    ) {
        if pool_address.is_empty() {
            return;
        }

        let now = Utc::now().to_rfc3339();
        let closed_at = input.closed_at.clone().unwrap_or_else(|| now.clone());
        let record = PoolDeployRecord {
            deployed_at: input.deployed_at.clone(),
            closed_at: Some(closed_at.clone()),
            pnl_pct: input.pnl_pct,
            pnl_usd: input.pnl_usd,
            fees_earned_usd: input.fees_earned_usd,
            fees_earned_sol: input.fees_earned_sol,
            fee_earned_pct: input.fee_earned_pct,
            range_efficiency: input.range_efficiency,
            minutes_held: input.minutes_held,
            close_reason: input.close_reason.clone(),
            strategy: input.strategy.clone(),
            volatility_at_deploy: input.volatility,
            entry_mcap: input.entry_mcap,
            entry_tvl: input.entry_tvl,
            entry_volume: input.entry_volume,
            exit_mcap: input.exit_mcap,
            exit_tvl: input.exit_tvl,
            exit_volume: input.exit_volume,
        };

        let mut pending_base_cooldowns: Vec<(String, u32, String)> = Vec::new();
        {
            let entry = self.ensure_entry(
                pool_address,
                input.base_mint.as_deref(),
                input.symbol.as_deref(),
                input.pool_name.as_deref(),
            );
            entry.deploys.push(record);
            entry.total_deploys = entry.deploys.len() as u32;
            entry.total_positions = entry.total_deploys;
            entry.last_deployed_at = Some(closed_at.clone());
            entry.last_close_at = Some(closed_at);
            entry.last_close_reason = input.close_reason.clone();
            entry.last_close_pnl = input.pnl_pct;
            entry.last_outcome = input.pnl_pct.map(|pnl| {
                if pnl >= 0.0 {
                    "profit".to_string()
                } else {
                    "loss".to_string()
                }
            });
            entry.total_fees_earned += input.fees_earned_sol.unwrap_or(0.0);
            recompute_aggregates(entry);

            if is_low_yield_close(entry.deploys.last().and_then(|d| d.close_reason.as_deref())) {
                set_pool_cooldown_hours(entry, LOW_YIELD_COOLDOWN_HOURS, "low yield");
            }

            let oor_trigger_count = management.oor_cooldown_trigger_count.max(1) as usize;
            let repeated_oor_closes = entry.deploys.len() >= oor_trigger_count
                && entry
                    .deploys
                    .iter()
                    .rev()
                    .take(oor_trigger_count)
                    .all(|deploy| is_oor_close_reason(deploy.close_reason.as_deref()));
            if repeated_oor_closes {
                let reason = format!("repeated OOR closes ({oor_trigger_count}x)");
                set_pool_cooldown_hours(entry, management.oor_cooldown_hours, &reason);
                if !entry.base_mint.is_empty() {
                    pending_base_cooldowns.push((
                        entry.base_mint.clone(),
                        management.oor_cooldown_hours,
                        reason,
                    ));
                }
            }

            if management.repeat_deploy_cooldown_enabled {
                let trigger_count = management.repeat_deploy_cooldown_trigger_count.max(1) as usize;
                let cooldown_hours = management.repeat_deploy_cooldown_hours;
                let scope = normalize_repeat_scope(&management.repeat_deploy_cooldown_scope);
                let repeated_fee_generating_deploys = cooldown_hours > 0
                    && entry.deploys.len() >= trigger_count
                    && entry
                        .deploys
                        .iter()
                        .rev()
                        .take(trigger_count)
                        .all(|deploy| {
                            deploy.pnl_pct.is_some()
                                && is_fee_generating_deploy(
                                    deploy,
                                    management.repeat_deploy_cooldown_min_fee_earned_pct,
                                )
                        });

                if repeated_fee_generating_deploys {
                    let reason = format!("repeat fee-generating deploys ({trigger_count}x)");
                    if scope == "pool" || scope == "both" || entry.base_mint.is_empty() {
                        set_pool_cooldown_hours(entry, cooldown_hours, &reason);
                    }
                    if (scope == "token" || scope == "both") && !entry.base_mint.is_empty() {
                        pending_base_cooldowns.push((
                            entry.base_mint.clone(),
                            cooldown_hours,
                            reason,
                        ));
                    }
                }
            }
        }

        // Repeat-loss guard (token-level, across ALL pools of this base mint).
        // SUPERMAN lost across its 80- and 100-bin-step pools — a per-pool count
        // never catches that, so aggregate by base token. If the token has at
        // least REPEAT_LOSS_TRIGGER losing closes AND is net-negative overall,
        // cool the whole token down so the bot stops re-entering a known bleeder.
        if let Some(base_mint) = input.base_mint.as_deref() {
            if !base_mint.is_empty() {
                let (loss_count, net_pnl) = self
                    .pools
                    .values()
                    .filter(|e| e.base_mint == base_mint)
                    .flat_map(|e| e.deploys.iter())
                    .filter_map(|d| d.pnl_pct)
                    .fold((0usize, 0.0f64), |(c, s), p| {
                        (
                            if p <= REPEAT_LOSS_THRESHOLD_PCT { c + 1 } else { c },
                            s + p,
                        )
                    });
                if loss_count >= REPEAT_LOSS_TRIGGER && net_pnl < 0.0 {
                    pending_base_cooldowns.push((
                        base_mint.to_string(),
                        REPEAT_LOSS_COOLDOWN_HOURS,
                        format!("repeat losses ({loss_count}x, net {net_pnl:.1}%)"),
                    ));
                }
            }
        }

        for (base_mint, hours, reason) in pending_base_cooldowns {
            self.set_base_mint_cooldown_hours(&base_mint, hours, &reason);
        }
    }

    pub fn is_on_cooldown(&self, base_mint: &str) -> bool {
        self.is_base_mint_on_cooldown(base_mint)
    }

    pub fn is_pool_on_cooldown(&self, pool_address: &str) -> bool {
        self.pools
            .get(pool_address)
            .and_then(|entry| entry.cooldown_until.as_deref())
            .is_some_and(is_future)
    }

    pub fn is_base_mint_on_cooldown(&self, base_mint: &str) -> bool {
        if base_mint.is_empty() {
            return false;
        }
        self.pools.values().any(|entry| {
            entry.base_mint == base_mint
                && entry
                    .base_mint_cooldown_until
                    .as_deref()
                    .is_some_and(is_future)
        })
    }

    pub fn set_pool_cooldown(&mut self, pool_address: &str, reason: &str, minutes: u32) {
        if let Some(entry) = self.pools.get_mut(pool_address) {
            set_pool_cooldown_minutes(entry, minutes, reason);
        }
    }

    pub fn set_cooldown(&mut self, base_mint: &str, reason: &str, minutes: u32) {
        self.set_base_mint_cooldown_minutes(base_mint, minutes, reason);
    }

    pub fn set_base_mint_cooldown_minutes(&mut self, base_mint: &str, minutes: u32, reason: &str) {
        if base_mint.is_empty() {
            return;
        }
        let until = (Utc::now() + Duration::minutes(minutes as i64)).to_rfc3339();
        for entry in self.pools.values_mut() {
            if entry.base_mint == base_mint {
                entry.on_cooldown = true;
                entry.base_mint_cooldown_until = Some(until.clone());
                entry.base_mint_cooldown_reason = Some(reason.to_string());
            }
        }
    }

    fn set_base_mint_cooldown_hours(&mut self, base_mint: &str, hours: u32, reason: &str) {
        self.set_base_mint_cooldown_minutes(base_mint, hours.saturating_mul(60), reason);
    }

    pub fn clear_cooldown(&mut self, base_mint: &str) {
        for entry in self.pools.values_mut() {
            if entry.base_mint == base_mint {
                entry.base_mint_cooldown_until = None;
                entry.base_mint_cooldown_reason = None;
                if entry
                    .cooldown_until
                    .as_deref()
                    .is_none_or(|until| !is_future(until))
                {
                    entry.on_cooldown = false;
                }
            }
        }
    }

    pub fn get_summary_for_prompt(&self) -> String {
        if self.pools.is_empty() {
            return "No pool history".to_string();
        }
        let mut lines = Vec::new();
        for entry in self.pools.values() {
            let deploy_count = entry.total_deploys.max(entry.total_positions);
            if deploy_count > 0 || !entry.notes.is_empty() {
                let mut line = format!(
                    "{}: {} positions, {:.4} SOL fees",
                    entry_display_name(entry),
                    deploy_count,
                    entry.total_fees_earned
                );
                if let Some(ref reason) = entry.last_close_reason {
                    line += &format!(", last close: {reason}");
                }
                if self.is_pool_on_cooldown(&entry.pool_address)
                    || self.is_base_mint_on_cooldown(&entry.base_mint)
                {
                    line += " [COOLDOWN]";
                }
                lines.push(line);
            }
        }
        lines.join("\n")
    }

    fn ensure_entry(
        &mut self,
        pool_address: &str,
        base_mint: Option<&str>,
        symbol: Option<&str>,
        name: Option<&str>,
    ) -> &mut PoolEntry {
        let fallback_name = if pool_address.len() > 8 {
            &pool_address[..8]
        } else {
            pool_address
        };
        let entry = self
            .pools
            .entry(pool_address.to_string())
            .or_insert_with(|| PoolEntry {
                pool_address: pool_address.to_string(),
                name: name
                    .map(String::from)
                    .or_else(|| Some(fallback_name.to_string())),
                base_mint: base_mint.unwrap_or_default().to_string(),
                symbol: symbol.map(String::from),
                ..Default::default()
            });

        if entry.pool_address.is_empty() {
            entry.pool_address = pool_address.to_string();
        }
        if entry.name.is_none() {
            entry.name = name
                .map(String::from)
                .or_else(|| Some(fallback_name.to_string()));
        }
        if entry.base_mint.is_empty() {
            if let Some(base_mint) = base_mint {
                entry.base_mint = base_mint.to_string();
            }
        }
        if entry.symbol.is_none() {
            entry.symbol = symbol.map(String::from);
        }
        entry
    }
}

fn set_pool_cooldown_hours(entry: &mut PoolEntry, hours: u32, reason: &str) {
    set_pool_cooldown_minutes(entry, hours.saturating_mul(60), reason);
}

fn set_pool_cooldown_minutes(entry: &mut PoolEntry, minutes: u32, reason: &str) {
    let until = (Utc::now() + Duration::minutes(minutes as i64)).to_rfc3339();
    entry.on_cooldown = true;
    entry.cooldown_until = Some(until);
    entry.cooldown_reason = Some(reason.to_string());
}

fn is_future(ts: &str) -> bool {
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc) > Utc::now())
        .unwrap_or(false)
}

fn sanitize_stored_note(text: &str) -> String {
    let mut cleaned = text
        .replace(['\r', '\n', '\t'], " ")
        .replace(['<', '>', '`'], " ");
    while cleaned.contains("  ") {
        cleaned = cleaned.replace("  ", " ");
    }
    cleaned.trim().chars().take(MAX_NOTE_LENGTH).collect()
}

fn entry_display_name(entry: &PoolEntry) -> String {
    if let Some(symbol) = entry.symbol.as_deref().filter(|s| !s.is_empty()) {
        symbol.to_string()
    } else if let Some(name) = entry.name.as_deref().filter(|s| !s.is_empty()) {
        name.to_string()
    } else if !entry.base_mint.is_empty() {
        entry.base_mint.chars().take(8).collect()
    } else {
        entry.pool_address.chars().take(8).collect()
    }
}

fn recompute_aggregates(entry: &mut PoolEntry) {
    let with_pnl: Vec<&PoolDeployRecord> = entry
        .deploys
        .iter()
        .filter(|deploy| deploy.pnl_pct.is_some())
        .collect();

    if !with_pnl.is_empty() {
        let sum: f64 = with_pnl.iter().filter_map(|deploy| deploy.pnl_pct).sum();
        entry.avg_pnl_pct = round2(sum / with_pnl.len() as f64);
        let wins = with_pnl
            .iter()
            .filter(|deploy| deploy.pnl_pct.unwrap_or_default() >= 0.0)
            .count();
        entry.win_rate = round2(wins as f64 / with_pnl.len() as f64);
    }

    let adjusted: Vec<&PoolDeployRecord> = with_pnl
        .into_iter()
        .filter(|deploy| !is_adjusted_win_rate_excluded_reason(deploy.close_reason.as_deref()))
        .collect();
    entry.adjusted_win_rate_sample_count = adjusted.len() as u32;
    entry.adjusted_win_rate = if adjusted.is_empty() {
        0.0
    } else {
        let adjusted_wins = adjusted
            .iter()
            .filter(|deploy| deploy.pnl_pct.unwrap_or_default() >= 0.0)
            .count();
        round2((adjusted_wins as f64 / adjusted.len() as f64) * 100.0)
    };
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn normalize_repeat_scope(scope: &str) -> &'static str {
    match scope.trim().to_ascii_lowercase().as_str() {
        "pool" => "pool",
        "both" => "both",
        _ => "token",
    }
}

fn is_low_yield_close(reason: Option<&str>) -> bool {
    reason
        .map(|reason| reason.trim().eq_ignore_ascii_case("low yield"))
        .unwrap_or(false)
}

fn is_oor_close_reason(reason: Option<&str>) -> bool {
    let text = reason.unwrap_or_default().trim().to_ascii_lowercase();
    text == "oor" || text.contains("out of range") || text.contains("oor")
}

fn is_adjusted_win_rate_excluded_reason(reason: Option<&str>) -> bool {
    let text = reason.unwrap_or_default().trim().to_ascii_lowercase();
    text.contains("out of range")
        || text.contains("pumped far above range")
        || text == "oor"
        || text.contains("oor")
}

fn is_fee_generating_deploy(deploy: &PoolDeployRecord, min_fee_earned_pct: f64) -> bool {
    let fee_earned_pct = deploy.fee_earned_pct.unwrap_or_default();
    let fees_usd = deploy.fees_earned_usd.unwrap_or_default();
    let fees_sol = deploy.fees_earned_sol.unwrap_or_default();
    let has_fees =
        (fees_usd.is_finite() && fees_usd > 0.0) || (fees_sol.is_finite() && fees_sol > 0.0);
    has_fees && fee_earned_pct.is_finite() && fee_earned_pct >= min_fee_earned_pct
}

#[cfg(test)]
mod tests {
    use super::{PoolDeployInput, PoolMemoryStore};
    use crate::config::types::{Config, ManagementConfig};

    fn js_management_config() -> ManagementConfig {
        let mut management = Config::default().management;
        management.oor_cooldown_trigger_count = 2;
        management.oor_cooldown_hours = 8;
        management.repeat_deploy_cooldown_enabled = true;
        management.repeat_deploy_cooldown_trigger_count = 2;
        management.repeat_deploy_cooldown_hours = 6;
        management.repeat_deploy_cooldown_scope = "token".to_string();
        management.repeat_deploy_cooldown_min_fee_earned_pct = 0.1;
        management
    }

    fn deploy(base_mint: &str, reason: &str, pnl_pct: f64) -> PoolDeployInput {
        PoolDeployInput {
            pool_name: Some("TEST/SOL".to_string()),
            base_mint: Some(base_mint.to_string()),
            close_reason: Some(reason.to_string()),
            pnl_pct: Some(pnl_pct),
            ..Default::default()
        }
    }

    #[test]
    fn low_yield_close_sets_js_parity_pool_cooldown_only() {
        let mut store = PoolMemoryStore::default();
        let management = js_management_config();

        store.record_deploy(
            "pool-low",
            deploy("MINT_LOW", "low yield", 0.2),
            &management,
        );

        let entry = store.get("pool-low").expect("pool should be recorded");
        assert_eq!(entry.total_deploys, 1);
        assert_eq!(entry.cooldown_reason.as_deref(), Some("low yield"));
        assert!(store.is_pool_on_cooldown("pool-low"));
        assert!(!store.is_base_mint_on_cooldown("MINT_LOW"));
    }

    #[test]
    fn repeated_oor_closes_set_pool_and_base_mint_cooldowns() {
        let mut store = PoolMemoryStore::default();
        let management = js_management_config();

        store.record_deploy(
            "pool-oor",
            deploy("MINT_OOR", "out of range", -3.0),
            &management,
        );
        assert!(!store.is_pool_on_cooldown("pool-oor"));

        store.record_deploy("pool-oor", deploy("MINT_OOR", "OOR", -2.0), &management);

        let entry = store.get("pool-oor").expect("pool should be recorded");
        assert_eq!(
            entry.cooldown_reason.as_deref(),
            Some("repeated OOR closes (2x)")
        );
        assert_eq!(
            entry.base_mint_cooldown_reason.as_deref(),
            Some("repeated OOR closes (2x)")
        );
        assert!(store.is_pool_on_cooldown("pool-oor"));
        assert!(store.is_base_mint_on_cooldown("MINT_OOR"));
    }

    #[test]
    fn repeat_fee_generating_deploys_follow_js_scope_token_default() {
        let mut store = PoolMemoryStore::default();
        let management = js_management_config();

        for _ in 0..2 {
            store.record_deploy(
                "pool-fees",
                PoolDeployInput {
                    fees_earned_sol: Some(0.01),
                    fee_earned_pct: Some(0.2),
                    ..deploy("MINT_FEES", "take profit", 2.0)
                },
                &management,
            );
        }

        let entry = store.get("pool-fees").expect("pool should be recorded");
        assert!(
            entry.cooldown_reason.is_none(),
            "token scope should not pool-cooldown"
        );
        assert_eq!(
            entry.base_mint_cooldown_reason.as_deref(),
            Some("repeat fee-generating deploys (2x)")
        );
        assert!(!store.is_pool_on_cooldown("pool-fees"));
        assert!(store.is_base_mint_on_cooldown("MINT_FEES"));
    }

    #[test]
    fn adjusted_win_rate_excludes_oor_reasons_like_original_js() {
        let mut store = PoolMemoryStore::default();
        let management = js_management_config();

        store.record_deploy("pool-stats", deploy("MINT_STATS", "OOR", -5.0), &management);
        store.record_deploy(
            "pool-stats",
            deploy("MINT_STATS", "take profit", 3.0),
            &management,
        );

        let entry = store.get("pool-stats").expect("pool should be recorded");
        assert_eq!(entry.avg_pnl_pct, -1.0);
        assert_eq!(entry.win_rate, 0.5);
        assert_eq!(entry.adjusted_win_rate_sample_count, 1);
        assert_eq!(entry.adjusted_win_rate, 100.0);
    }
}
