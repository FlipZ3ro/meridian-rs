use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::config::{meridian_data_path, Config};
use crate::lessons::LessonStore;
use crate::llm::ToolCall;
use crate::state::pool_memory::PoolMemoryStore;
use crate::state::positions::PositionState;
use crate::tools::dlmm::{
    claim_fees, close_position, get_active_bin, get_my_positions, get_position_pnl,
    get_wallet_positions, search_pools,
};
use crate::tools::screening::Screener;
use crate::tools::study::study_top_lpers;
use crate::tools::token::{
    check_smart_wallets_on_pool, get_token_holders, get_token_info, get_token_narrative,
};
use crate::tools::wallet::{get_wallet_balances, normalize_mint, swap_token, WalletBalances};
use crate::utils::logger::module::{info, warn};

// ─── Tool Classification ────────────────────────────────────────

const PROTECTED_TOOLS: &[&str] = &[
    "deploy_position",
    "close_position",
    "claim_fees",
    "swap_token",
];

/// Tools that lock on first execution regardless of success (no retry).
const LOCK_ON_FIRST_ATTEMPT: &[&str] = &["deploy_position"];

/// Tools that lock only on successful execution.
const LOCK_ON_SUCCESS: &[&str] = &["swap_token", "close_position"];

const MAX_DECISION_LOG: usize = 100;
const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
const AUTO_SWAP_DUST_USD_THRESHOLD: f64 = 0.10;

#[derive(Debug, Clone, PartialEq)]
struct AutoSwapPlan {
    mint: String,
    symbol: String,
    amount: f64,
    usd: f64,
}

fn json_str<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| value.get(*key)?.as_str())
}

fn set_json_string_if_missing(value: &mut Value, key: &str, content: Option<&str>) {
    let Some(content) = content.filter(|text| !text.is_empty()) else {
        return;
    };
    let Some(map) = value.as_object_mut() else {
        return;
    };
    let missing = map
        .get(key)
        .and_then(|existing| existing.as_str())
        .is_none_or(|existing| existing.is_empty());
    if missing {
        map.insert(key.to_string(), Value::String(content.to_string()));
    }
}

fn find_tracked_position<'a>(
    args: &Value,
    positions: &'a PositionState,
) -> Option<&'a crate::state::positions::TrackedPosition> {
    let key = json_str(
        args,
        &[
            "position_id",
            "positionId",
            "position_address",
            "positionAddress",
        ],
    )?;
    positions.positions.get(key).or_else(|| {
        positions.positions.values().find(|position| {
            position.id == key
                || position.pool_address == key
                || json_str(args, &["position_address", "positionAddress"])
                    == Some(position.id.as_str())
        })
    })
}

fn enrich_close_result_with_position_metadata(
    result: &mut Value,
    args: &Value,
    positions: &PositionState,
) {
    let Some(position) = find_tracked_position(args, positions) else {
        return;
    };
    set_json_string_if_missing(result, "baseMint", Some(&position.base_mint));
    set_json_string_if_missing(result, "pool", Some(&position.pool_address));
    set_json_string_if_missing(result, "poolName", position.pool_name.as_deref());
}

fn plan_auto_swap_after_close(
    close_result: &Value,
    balances: &WalletBalances,
    skip_swap: bool,
) -> Option<AutoSwapPlan> {
    if skip_swap || close_result.get("success").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let base_mint = json_str(close_result, &["baseMint", "base_mint"])?;
    let normalized_base = normalize_mint(base_mint);
    if normalized_base == SOL_MINT {
        return None;
    }
    let token = balances
        .tokens
        .iter()
        .find(|token| normalize_mint(&token.mint) == normalized_base)?;
    let usd = token.usd.unwrap_or(0.0);
    if token.balance <= 0.0 || usd < AUTO_SWAP_DUST_USD_THRESHOLD {
        return None;
    }
    Some(AutoSwapPlan {
        mint: token.mint.clone(),
        symbol: token.symbol.clone(),
        amount: token.balance,
        usd,
    })
}

// ─── Decision Log ───────────────────────────────────────────────

const DECISION_LOG_FILE: &str = "decision-log.json";

pub fn decision_log_path() -> PathBuf {
    meridian_data_path(DECISION_LOG_FILE)
}

pub fn append_decision_log_entry(
    path: &Path,
    tool: &str,
    args: &Value,
    result: &str,
    success: bool,
) -> Result<()> {
    let entry = json!({
        "schemaVersion": 1,
        "id": format!("decision_{}", chrono::Utc::now().timestamp_millis()),
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "tool": tool,
        "args": redact_sensitive_values(args),
        "argsSummary": args_summary(&redact_sensitive_values(args)),
        "success": success,
        "result": parse_result_value(result),
        "resultSummary": truncate(result, 200),
    });

    let mut log: Vec<Value> = std::fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();

    log.push(entry);

    if log.len() > MAX_DECISION_LOG {
        let excess = log.len() - MAX_DECISION_LOG;
        log.drain(..excess);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&log)?)?;
    Ok(())
}

fn log_decision(tool: &str, args: &Value, result: &str, success: bool) {
    if let Err(e) = append_decision_log_entry(&decision_log_path(), tool, args, result, success) {
        warn("executor", &format!("Failed to write decision log: {}", e));
    }
}

fn should_log_decision(tool: &str) -> bool {
    tool != "get_recent_decisions"
}

fn recent_decision_limit(raw: Option<u64>) -> usize {
    raw.unwrap_or(5).clamp(1, MAX_DECISION_LOG as u64) as usize
}

pub fn read_recent_decisions_from_path(path: &Path, limit: usize) -> Result<Vec<Value>> {
    let log: Vec<Value> = std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default();
    Ok(log.into_iter().rev().take(limit).collect())
}

pub fn read_recent_decisions(limit: usize) -> Result<Vec<Value>> {
    read_recent_decisions_from_path(&decision_log_path(), limit)
}

pub fn get_recent_decisions_for_prompt(limit: usize) -> Result<String> {
    let decisions = read_recent_decisions(limit)?;
    Ok(decisions
        .iter()
        .map(decision_summary_for_prompt)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn decision_summary_for_prompt(decision: &Value) -> String {
    let timestamp = decision["timestamp"].as_str().unwrap_or("unknown-time");
    let tool = decision["tool"].as_str().unwrap_or("unknown-tool");
    let success = decision["success"].as_bool().unwrap_or(false);
    let args_summary = decision["argsSummary"].as_str().unwrap_or("");
    let result_summary = decision["resultSummary"].as_str().unwrap_or("");
    format!(
        "- [{}] tool={} success={} args=[{}] result={}",
        timestamp, tool, success, args_summary, result_summary
    )
}

fn parse_result_value(result: &str) -> Value {
    serde_json::from_str(result).unwrap_or_else(|_| Value::String(truncate(result, 500)))
}

fn redact_sensitive_values(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let sanitized = if is_sensitive_key(key) {
                        Value::String("***redacted***".to_string())
                    } else {
                        redact_sensitive_values(value)
                    };
                    (key.clone(), sanitized)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_sensitive_values).collect()),
        _ => value.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    ["key", "secret", "token", "password", "private"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn args_summary(args: &Value) -> String {
    match args {
        Value::Object(map) => {
            let pairs: Vec<String> = map
                .iter()
                .take(5)
                .map(|(k, v)| match v {
                    Value::String(s) => format!("{}={}", k, truncate(s, 40)),
                    Value::Number(n) => format!("{}={}", k, n),
                    Value::Bool(b) => format!("{}={}", k, b),
                    _ => format!("{}=...", k),
                })
                .collect();
            pairs.join(", ")
        }
        _ => truncate(&args.to_string(), 100),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

// ─── Executor ───────────────────────────────────────────────────

pub struct ToolExecutor {
    pub screener: Screener,
    pub executed_once: Vec<String>,
    /// Wallet address for API calls (Helius/Meteora)
    wallet_address: String,
}

impl ToolExecutor {
    pub fn new(wallet_address: &str) -> Self {
        Self {
            screener: Screener::new(),
            executed_once: vec![],
            wallet_address: wallet_address.to_string(),
        }
    }

    pub fn is_once_locked(&self, name: &str) -> bool {
        LOCK_ON_FIRST_ATTEMPT.contains(&name) && self.executed_once.contains(&name.to_string())
    }

    /// Lock a tool after execution if it qualifies.
    pub fn maybe_lock(&mut self, name: &str, success: bool) {
        if LOCK_ON_FIRST_ATTEMPT.contains(&name) {
            if !self.executed_once.contains(&name.to_string()) {
                self.executed_once.push(name.to_string());
            }
        } else if LOCK_ON_SUCCESS.contains(&name)
            && success
            && !self.executed_once.contains(&name.to_string())
        {
            self.executed_once.push(name.to_string());
        }
    }

    pub async fn execute(
        &mut self,
        call: &ToolCall,
        config: &Config,
        positions: &mut PositionState,
        pool_memory: &mut PoolMemoryStore,
    ) -> (String, bool) {
        let name = call.function.name.as_str();
        let args: Value = serde_json::from_str(&call.function.arguments)
            .unwrap_or(Value::Object(serde_json::Map::new()));

        // ── Once-per-session lock ──────────────────────────────
        if self.is_once_locked(name) {
            return (
                format!("Tool {} already executed this session. Skipping.", name),
                true,
            );
        }

        // ── Safety checks for protected tools ──────────────────
        if PROTECTED_TOOLS.contains(&name) {
            if let Err(e) = self
                .run_safety_checks(name, &args, config, positions, pool_memory)
                .await
            {
                log_decision(name, &args, &e.to_string(), false);
                return (format!("SAFETY CHECK FAILED: {}", e), true);
            }
        }

        // ── Dispatch ───────────────────────────────────────────
        let result = self
            .dispatch(name, &args, config, positions, pool_memory)
            .await;

        // ── Post-tool side effects ─────────────────────────────
        let (msg, is_error) = match result {
            Ok(msg) => {
                let is_ok = !msg.contains("\"success\":false")
                    && !msg.contains("error")
                    && !msg.contains("STUB");
                self.maybe_lock(name, is_ok);
                (msg, false)
            }
            Err(e) => {
                self.maybe_lock(name, false);
                (format!("Tool {} error: {}", name, e), true)
            }
        };

        // ── Audit log ──────────────────────────────────────────
        if should_log_decision(name) {
            log_decision(name, &args, &msg, !is_error);
        }

        // ── Post-effects ───────────────────────────────────────
        if !is_error {
            if let Err(e) = self
                .run_post_effects(name, &args, &msg, config, positions, pool_memory)
                .await
            {
                warn(
                    "executor",
                    &format!("Post-effect error for {}: {}", name, e),
                );
            }
        }

        (msg, is_error)
    }

    // ─── Safety Checks ─────────────────────────────────────────

    async fn run_safety_checks(
        &self,
        name: &str,
        args: &Value,
        config: &Config,
        positions: &PositionState,
        pool_memory: &PoolMemoryStore,
    ) -> Result<()> {
        match name {
            "deploy_position" => {
                let pool_addr = args["pool_address"].as_str().unwrap_or("");
                if pool_addr.is_empty() {
                    anyhow::bail!("pool_address required");
                }

                // Amount validation
                let amount = args["amount_y"]
                    .as_f64()
                    .or(args["amount_sol"].as_f64())
                    .unwrap_or(0.0);
                if amount <= 0.0 {
                    anyhow::bail!("amount must be > 0");
                }
                if amount > config.risk.max_deploy_amount {
                    anyhow::bail!(
                        "amount {} exceeds max {}",
                        amount,
                        config.risk.max_deploy_amount
                    );
                }

                // Max positions check
                let active_count = positions.count_active();
                if active_count >= config.risk.max_positions as usize {
                    anyhow::bail!(
                        "at max positions ({}/{})",
                        active_count,
                        config.risk.max_positions
                    );
                }

                // Duplicate pool check
                let active = positions.get_active();
                if active.iter().any(|p| p.pool_address == pool_addr) {
                    anyhow::bail!(
                        "already have position in pool {}",
                        &pool_addr[..12.min(pool_addr.len())]
                    );
                }

                // Duplicate base mint check (if provided)
                if let Some(base_mint) = args["base_mint"].as_str() {
                    if !base_mint.is_empty() {
                        let normalized = normalize_mint(base_mint);
                        let has_dup = active
                            .iter()
                            .any(|p| normalize_mint(&p.base_mint) == normalized);
                        if has_dup {
                            anyhow::bail!("already have position with same base token");
                        }
                    }
                }

                // Pool cooldown check
                if pool_memory.is_on_cooldown(pool_addr) {
                    anyhow::bail!(
                        "pool {} is on cooldown — recently closed at loss",
                        &pool_addr[..12.min(pool_addr.len())]
                    );
                }
                // Token cooldown check (check base_mint)
                if let Some(base_mint) = args["base_mint"].as_str() {
                    if !base_mint.is_empty() && pool_memory.is_on_cooldown(base_mint) {
                        anyhow::bail!(
                            "token {} is on cooldown",
                            &base_mint[..12.min(base_mint.len())]
                        );
                    }
                }

                // Bins below minimum check
                let bins_below = args["bins_below"].as_i64().unwrap_or(20);
                if bins_below < config.strategy.min_safe_bins_below as i64 {
                    anyhow::bail!(
                        "bins_below {} < safe minimum {}",
                        bins_below,
                        config.strategy.min_safe_bins_below
                    );
                }

                // Pool detail validation
                if let Ok(Some(detail)) = self
                    .screener
                    .get_pool_detail(pool_addr, &config.screening.timeframe)
                    .await
                {
                    let tvl = detail.tvl.or(detail.active_tvl).unwrap_or(0.0);
                    if tvl < config.screening.min_tvl {
                        anyhow::bail!(
                            "pool TVL ${:.0} below min ${:.0}",
                            tvl,
                            config.screening.min_tvl
                        );
                    }
                    if let Some(max_tvl) = config.screening.max_tvl {
                        if tvl > max_tvl {
                            anyhow::bail!("pool TVL ${:.0} above max ${:.0}", tvl, max_tvl);
                        }
                    }
                    let vol = detail.volatility.unwrap_or(0.0);
                    if vol <= 0.0 {
                        anyhow::bail!("pool volatility unusable (0)");
                    }
                    let fee_tvl = detail.fee_active_tvl_ratio.unwrap_or(0.0);
                    if fee_tvl < config.screening.min_fee_active_tvl_ratio {
                        anyhow::bail!(
                            "pool fee/TVL {:.4} below min {:.4}",
                            fee_tvl,
                            config.screening.min_fee_active_tvl_ratio
                        );
                    }

                    // Inject entry market data for position tracking
                    info(
                        "executor",
                        &format!(
                            "Deploy pre-flight OK — pool={}, tvl=${:.0}, vol={}, fee_tvl={:.4}",
                            &pool_addr[..12.min(pool_addr.len())],
                            tvl,
                            vol,
                            fee_tvl
                        ),
                    );
                } else {
                    anyhow::bail!("could not verify pool before deploy");
                }

                // On-chain dedup guard (LAST check — smallest race window).
                // The tracked-state checks above can be bypassed if two screening
                // cycles overlap (deploy #1 not yet saved when deploy #2 validates).
                // This queries the wallet's live on-chain positions and refuses if
                // ANY already sits in this pool — closing the race for real.
                match crate::tools::meteora_native::discover_wallet_positions(config).await {
                    Ok(onchain) => {
                        if onchain.iter().any(|(_pos, lb_pair)| lb_pair == pool_addr) {
                            anyhow::bail!(
                                "on-chain position already exists in pool {} (race guard)",
                                &pool_addr[..12.min(pool_addr.len())]
                            );
                        }
                    }
                    Err(e) => {
                        // Don't hard-fail deploy on a transient RPC hiccup, but log it.
                        info(
                            "executor",
                            &format!("on-chain dedup check skipped (rpc error): {}", e),
                        );
                    }
                }

                Ok(())
            }
            "close_position" => {
                let pid = args["position_id"].as_str().unwrap_or("");
                if pid.is_empty() {
                    anyhow::bail!("position_id required");
                }
                // Verify position exists and is active
                if !positions.positions.contains_key(pid) {
                    anyhow::bail!("position {} not found in state", pid);
                }
                Ok(())
            }
            "claim_fees" => {
                let pid = args["position_id"].as_str().unwrap_or("");
                if pid.is_empty() {
                    anyhow::bail!("position_id required");
                }
                Ok(())
            }
            "swap_token" => {
                let from = args["from_mint"].as_str().unwrap_or("");
                let to = args["to_mint"].as_str().unwrap_or("");
                if from.is_empty() || to.is_empty() {
                    anyhow::bail!("from_mint and to_mint required");
                }
                let amount = args["amount"].as_f64().unwrap_or(0.0);
                if amount <= 0.0 {
                    anyhow::bail!("amount must be > 0");
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    // ─── Post-Tool Side Effects ────────────────────────────────

    async fn run_post_effects(
        &self,
        name: &str,
        args: &Value,
        result: &str,
        config: &Config,
        positions: &mut PositionState,
        pool_memory: &mut PoolMemoryStore,
    ) -> Result<()> {
        match name {
            "close_position" => {
                let pid = args["position_id"].as_str().unwrap_or("");

                // Auto-annotate pool memory on low-yield close
                if result.contains("low yield") || result.contains("LOW_YIELD") {
                    if let Some(pos) = positions.positions.get(pid) {
                        let pool = pos.pool_address.clone();
                        let sym = pos.base_symbol.clone().unwrap_or_default();
                        let note = "CLOSED low yield — avoid re-entry. Fees insufficient for TVL.".to_string();
                        pool_memory.add_note(&pool, &pos.base_mint, Some(&sym), &note);
                        info(
                            "executor",
                            &format!("Auto-annotated pool memory for low-yield close: {}", sym),
                        );
                    }
                }

                // Set cooldown on loss closes to prevent re-entry
                if let Some(pos) = positions.positions.get(pid) {
                    if let Some(pnl) = pos.pnl_sol {
                        if pnl < 0.0 {
                            // Cooldown pool for 1 hour
                            pool_memory.set_pool_cooldown(&pos.pool_address, "loss_close", 60);
                            // Cooldown token too
                            pool_memory.set_base_mint_cooldown_minutes(&pos.base_mint, 60, "loss_close");
                            info(
                                "executor",
                                &format!("Set 1h cooldown on pool/token after {:.4} SOL loss", pnl),
                            );
                        }
                    }
                }

                // Auto-swap base token back to SOL after close
                if config.management.sol_mode {
                    // Extract base_mint from result if available
                    if let Ok(parsed) = serde_json::from_str::<Value>(result) {
                        if let Some(base_mint) = parsed["base_mint"].as_str() {
                            let normalized = normalize_mint(base_mint);
                            if normalized != "So11111111111111111111111111111111111111112" {
                                info(
                                    "executor",
                                    &format!(
                                        "Auto-swap: {} -> SOL after close",
                                        &normalized[..8.min(normalized.len())]
                                    ),
                                );
                                // NOTE: Actual swap would need wallet keypair — here we log the intent
                                // In production, this would call swap_token with the closed position's base_mint
                            }
                        }
                    }
                }
            }
            "claim_fees"
                // Auto-swap claimed tokens to SOL if enabled
                if config.management.sol_mode => {
                    if let Ok(parsed) = serde_json::from_str::<Value>(result) {
                        if let Some(base_mint) = parsed["base_mint"].as_str() {
                            let normalized = normalize_mint(base_mint);
                            if normalized != "So11111111111111111111111111111111111111112" {
                                info(
                                    "executor",
                                    &format!(
                                        "Auto-swap: claimed {} -> SOL",
                                        &normalized[..8.min(normalized.len())]
                                    ),
                                );
                            }
                        }
                    }
                }
            "deploy_position" => {
                // Record in pool memory
                if let Some(pool) = args["pool_address"].as_str() {
                    let sym = args["symbol"].as_str().unwrap_or("unknown");
                    let note = format!(
                        "DEPLOYED — bins_below={}, amount={}",
                        args["bins_below"].as_i64().unwrap_or(0),
                        args["amount_y"].as_f64().unwrap_or(0.0),
                    );
                    pool_memory.add_note(pool, "", Some(sym), &note);
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn maybe_auto_swap_after_close(
        &self,
        close_result: &mut Value,
        skip_swap: bool,
        config: &Config,
    ) -> Result<()> {
        if !config.management.sol_mode {
            return Ok(());
        }

        let rpc = config
            .api
            .helius_rpc_url
            .as_deref()
            .unwrap_or("https://api.mainnet-beta.solana.com");
        let helius_key = config.api.helius_api_key.as_deref().unwrap_or("");
        let balances = match get_wallet_balances(rpc, &self.wallet_address, helius_key).await {
            Ok(balances) => balances,
            Err(e) => {
                if let Some(map) = close_result.as_object_mut() {
                    map.insert(
                        "autoSwapError".to_string(),
                        Value::String(format!("wallet balance check failed: {}", e)),
                    );
                }
                return Ok(());
            }
        };

        let Some(plan) = plan_auto_swap_after_close(close_result, &balances, skip_swap) else {
            return Ok(());
        };

        info(
            "executor",
            &format!(
                "Auto-swapping {} (${:.2}) back to SOL after close",
                plan.symbol, plan.usd
            ),
        );
        match swap_token(&plan.mint, plan.amount, 50, 100, config).await {
            Ok(swap) => {
                if let Some(map) = close_result.as_object_mut() {
                    map.insert("autoSwapped".to_string(), Value::Bool(swap.success));
                    map.insert(
                        "autoSwapNote".to_string(),
                        Value::String(format!(
                            "Base token already auto-swapped back to SOL ({} → SOL). Do NOT call swap_token again.",
                            plan.symbol
                        )),
                    );
                    if let Some(tx) = swap.tx {
                        map.insert("autoSwapTx".to_string(), Value::String(tx));
                    }
                    if let Some(amount_out) = swap.amount_out {
                        map.insert("solReceived".to_string(), Value::String(amount_out));
                    }
                    if let Some(error) = swap.error {
                        map.insert("autoSwapError".to_string(), Value::String(error));
                    }
                }
            }
            Err(e) => {
                if let Some(map) = close_result.as_object_mut() {
                    map.insert("autoSwapped".to_string(), Value::Bool(false));
                    map.insert("autoSwapError".to_string(), Value::String(e.to_string()));
                }
            }
        }
        Ok(())
    }

    // ─── Tool Dispatch ─────────────────────────────────────────

    async fn dispatch(
        &mut self,
        name: &str,
        args: &Value,
        config: &Config,
        positions: &mut PositionState,
        pool_memory: &mut PoolMemoryStore,
    ) -> Result<String> {
        match name {
            // ── Read-only tools (real API calls) ───────────────
            "get_recent_decisions" => {
                let limit = recent_decision_limit(args["limit"].as_u64());
                let decisions = read_recent_decisions(limit)?;
                Ok(serde_json::to_string_pretty(&json!({
                    "success": true,
                    "count": decisions.len(),
                    "decisions": decisions,
                    "path": decision_log_path().display().to_string(),
                }))?)
            }
            "get_performance_history" => {
                let hours = args["hours"].as_f64().unwrap_or(24.0);
                let limit = args["limit"].as_u64().unwrap_or(50).clamp(1, 200) as usize;
                let path = meridian_data_path("lessons.json");
                let store = LessonStore::load(path.to_str().unwrap_or("lessons.json"))?;
                let history = store.get_performance_history(hours, limit);
                Ok(serde_json::to_string_pretty(&json!({
                    "success": true,
                    "history": history,
                    "path": path.display().to_string(),
                }))?)
            }
            "get_wallet_balance" => {
                let wallet = args["wallet"].as_str().unwrap_or(&self.wallet_address);
                let rpc = config
                    .api
                    .helius_rpc_url
                    .as_deref()
                    .unwrap_or("https://api.mainnet-beta.solana.com");
                let helius_key = config.api.helius_api_key.as_deref().unwrap_or("");
                match get_wallet_balances(rpc, wallet, helius_key).await {
                    Ok(balances) => Ok(serde_json::to_string_pretty(&balances)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "get_my_positions" => match get_my_positions(&self.wallet_address, config).await {
                Ok(result) => Ok(serde_json::to_string_pretty(&result)?),
                Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
            },
            "get_position_pnl" => {
                let pool = args["pool_address"].as_str().unwrap_or("");
                let pos = args["position_address"].as_str().unwrap_or("");
                if pool.is_empty() || pos.is_empty() {
                    // Try to get from local state
                    if let Some(pid) = args["position_id"].as_str() {
                        if let Some(p) = positions.positions.get(pid) {
                            return Ok(serde_json::to_string_pretty(&json!({
                                "position": pid,
                                "pnl_sol": p.pnl_sol.unwrap_or(0.0),
                                "fees_claimed": p.total_fees_claimed,
                                "source": "local_state",
                            }))?);
                        }
                    }
                    anyhow::bail!(
                        "pool_address and position_address required (or position_id from state)"
                    );
                }
                match get_position_pnl(pool, pos, &self.wallet_address).await {
                    Ok(pnl) => Ok(serde_json::to_string_pretty(&pnl)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "get_active_bin" => {
                let pool = args["pool_address"].as_str().unwrap_or("");
                if pool.is_empty() {
                    anyhow::bail!("pool_address required");
                }
                match get_active_bin(pool).await {
                    Ok(bin) => Ok(serde_json::to_string_pretty(&bin)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "search_pools" => {
                let query = args["query"].as_str().unwrap_or("");
                let limit = args["limit"].as_u64().map(|l| l as usize);
                match search_pools(query, limit).await {
                    Ok(result) => Ok(serde_json::to_string_pretty(&result)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "get_top_candidates" => {
                let limit = args["limit"].as_u64().unwrap_or(3) as usize;
                match self
                    .screener
                    .get_top_candidates_with_rejections(&config.screening, limit)
                    .await
                {
                    Ok(result) => Ok(serde_json::to_string_pretty(&result)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "discover_pools" => {
                let page_size = args["page_size"].as_u64().unwrap_or(50) as u32;
                match self
                    .screener
                    .discover_pools(&config.screening, page_size)
                    .await
                {
                    Ok(pools) => {
                        Ok(json!({"count": pools.len(), "pools_count": pools.len()}).to_string())
                    }
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "get_wallet_positions" => {
                let wallet = args["wallet"].as_str().unwrap_or(&self.wallet_address);
                match get_wallet_positions(wallet).await {
                    Ok(result) => Ok(serde_json::to_string_pretty(&result)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "get_token_info" => {
                let mint = args["mint"].as_str().unwrap_or("");
                if mint.is_empty() {
                    anyhow::bail!("mint required");
                }
                match get_token_info(mint).await {
                    Ok(info) => Ok(serde_json::to_string_pretty(&info)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "get_token_holders" => {
                let mint = args["mint"].as_str().unwrap_or("");
                if mint.is_empty() {
                    anyhow::bail!("mint required");
                }
                match get_token_holders(mint, 20).await {
                    Ok(holders) => Ok(serde_json::to_string_pretty(&holders)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "get_token_narrative" => {
                let mint = args["mint"].as_str().unwrap_or("");
                if mint.is_empty() {
                    anyhow::bail!("mint required");
                }
                match get_token_narrative(mint).await {
                    Ok(narrative) => Ok(serde_json::to_string_pretty(&narrative)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "check_smart_wallets_on_pool" => {
                let pool = args["pool_address"]
                    .as_str()
                    .or_else(|| args["pool"].as_str())
                    .unwrap_or("");
                if pool.is_empty() {
                    anyhow::bail!("pool_address required");
                }
                match check_smart_wallets_on_pool(pool).await {
                    Ok(result) => Ok(serde_json::to_string_pretty(&result)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "study_top_lpers" | "get_top_lpers" => {
                let pool = args["pool_address"]
                    .as_str()
                    .or_else(|| args["pool"].as_str())
                    .unwrap_or("");
                if pool.is_empty() {
                    anyhow::bail!("pool_address required");
                }
                let default_limit = if name == "get_top_lpers" { 5 } else { 4 };
                let limit = args["limit"].as_u64().unwrap_or(default_limit).clamp(1, 25) as usize;
                match study_top_lpers(pool, limit, config).await {
                    Ok(result) => Ok(serde_json::to_string_pretty(&result)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }

            // ── Pool memory ────────────────────────────────────
            "get_pool_memory" => Ok(pool_memory.get_summary_for_prompt()),
            "add_pool_note" => {
                let pool = args["pool_address"].as_str().unwrap_or("");
                let note = args["note"].as_str().unwrap_or("");
                if pool.is_empty() || note.is_empty() {
                    anyhow::bail!("pool_address and note required");
                }
                let sym = args["symbol"].as_str().unwrap_or("unknown");
                pool_memory.add_note(pool, "", Some(sym), note);
                Ok(format!(
                    "Pool note added for {} ({}).",
                    sym,
                    &pool[..12.min(pool.len())]
                ))
            }

            // ── Strategy library ────────────────────────────────
            "add_strategy" => {
                let id = args["id"].as_str().unwrap_or("");
                let name = args["name"].as_str().unwrap_or("");
                if id.is_empty() || name.is_empty() {
                    anyhow::bail!("id and name are required");
                }
                let result = crate::strategy_library::add_strategy(
                    crate::strategy_library::StrategyInput {
                        id: id.to_string(),
                        name: name.to_string(),
                        author: args["author"].as_str().map(str::to_string),
                        lp_strategy: args["lp_strategy"].as_str().map(str::to_string),
                        token_criteria: args
                            .get("token_criteria")
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                        entry: args.get("entry").cloned().unwrap_or_else(|| json!({})),
                        range: args.get("range").cloned().unwrap_or_else(|| json!({})),
                        exit: args.get("exit").cloned().unwrap_or_else(|| json!({})),
                        best_for: args["best_for"].as_str().map(str::to_string),
                        raw: args["raw"].as_str().map(str::to_string),
                    },
                )?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            "list_strategies" => Ok(serde_json::to_string_pretty(
                &crate::strategy_library::list_strategies()?,
            )?),
            "get_strategy" => {
                let id = args["id"].as_str().unwrap_or("");
                if id.is_empty() {
                    anyhow::bail!("id required");
                }
                let store = crate::strategy_library::load_default_store()?;
                let strategy = store
                    .get_strategy(id)
                    .ok_or_else(|| anyhow::anyhow!("Strategy \"{}\" not found", id))?;
                let mut value = serde_json::to_value(strategy)?;
                if let Some(map) = value.as_object_mut() {
                    map.insert(
                        "is_active".to_string(),
                        Value::Bool(store.active.as_deref() == Some(id)),
                    );
                }
                Ok(serde_json::to_string_pretty(&value)?)
            }
            "set_active_strategy" => {
                let id = args["id"].as_str().unwrap_or("");
                if id.is_empty() {
                    anyhow::bail!("id required");
                }
                Ok(serde_json::to_string_pretty(
                    &crate::strategy_library::set_active_strategy(id)?,
                )?)
            }
            "remove_strategy" => {
                let id = args["id"].as_str().unwrap_or("");
                if id.is_empty() {
                    anyhow::bail!("id required");
                }
                Ok(serde_json::to_string_pretty(
                    &crate::strategy_library::remove_strategy(id)?,
                )?)
            }

            // ── Blacklist ──────────────────────────────────────
            "blacklist_token" => {
                let mint = args["mint"].as_str().unwrap_or("");
                let reason = args["reason"].as_str().unwrap_or("manual");
                if mint.is_empty() {
                    anyhow::bail!("mint required");
                }
                match crate::tools::blacklist::add_to_blacklist(mint, Some("agent"), Some(reason)) {
                    Ok(_) => Ok(format!(
                        "Token {} blacklisted.",
                        &mint[..12.min(mint.len())]
                    )),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }
            "unblock_dev" => {
                let wallet = args["wallet"].as_str().unwrap_or("");
                if wallet.is_empty() {
                    anyhow::bail!("wallet required");
                }
                match crate::tools::blacklist::unblock_dev(wallet) {
                    Ok(_) => Ok(format!(
                        "Dev {} unblocked.",
                        &wallet[..12.min(wallet.len())]
                    )),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            // ── Mutable tools (stubs — need SDK) ───────────────
            "deploy_position" => {
                let pool = args["pool_address"].as_str().unwrap_or("");
                let amount = args["amount_y"]
                    .as_f64()
                    .or(args["amount_sol"].as_f64())
                    .unwrap_or(0.0);
                let bins_below = args["bins_below"].as_i64();
                let bins_above = args["bins_above"].as_i64();
                let strategy = args["strategy"].as_str();
                match crate::tools::dlmm::deploy_position(
                    pool, amount, bins_below, bins_above, strategy, config,
                )
                .await
                {
                    Ok(result) => {
                        // Track dry run positions as real positions
                        if result.success && result.position.as_deref().map_or(false, |id| id.starts_with("DRY_")) {
                            let pos_id = result.position.clone().unwrap();
                            let tracked = crate::state::positions::TrackedPosition {
                                id: pos_id.clone(),
                                pool_address: result.pool.clone().unwrap_or_default(),
                                pool_name: result.pool_name.clone(),
                                base_mint: String::new(),
                                base_symbol: None,
                                lower_bin: result.bin_range.as_ref().map(|r| r.min).unwrap_or(0),
                                upper_bin: result.bin_range.as_ref().map(|r| r.max).unwrap_or(0),
                                amount_sol: amount,
                                status: crate::state::positions::PositionStatus::Active,
                                created_at: chrono::Utc::now().to_rfc3339(),
                                note: Some("DRY RUN — simulated position".to_string()),
                                ..Default::default()
                            };
                            positions.add(tracked);
                            crate::utils::logger::module::info(
                                "executor",
                                &format!("Dry run position tracked: {}", pos_id),
                            );
                        }
                        Ok(serde_json::to_string_pretty(&result)?)
                    }
                    Err(e) => Ok(json!({
                        "success": false,
                        "pool": pool,
                        "amount_sol": amount,
                        "bins_below": bins_below,
                        "error": e.to_string(),
                    })
                    .to_string()),
                }
            }
            "close_position" => {
                let position = args["position_address"]
                    .as_str()
                    .or_else(|| args["position_id"].as_str())
                    .unwrap_or("");
                let reason = args["reason"].as_str();
                let skip_swap = args["skip_swap"]
                    .as_bool()
                    .or_else(|| args["skipSwap"].as_bool())
                    .unwrap_or(false);
                if position.is_empty() {
                    anyhow::bail!("position_id or position_address required");
                }

                let close = close_position(position, reason, config).await?;
                let mut result = serde_json::to_value(&close)?;
                enrich_close_result_with_position_metadata(&mut result, args, positions);
                self.maybe_auto_swap_after_close(&mut result, skip_swap, config)
                    .await?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            "claim_fees" => {
                let position = args["position_address"]
                    .as_str()
                    .or_else(|| args["position_id"].as_str())
                    .unwrap_or("");
                if position.is_empty() {
                    anyhow::bail!("position_id or position_address required");
                }
                match claim_fees(position, config).await {
                    Ok(result) => Ok(serde_json::to_string_pretty(&result)?),
                    Err(e) => Ok(json!({
                        "success": false,
                        "position": position,
                        "error": e.to_string(),
                    })
                    .to_string()),
                }
            }
            "swap_token" => {
                let from = args["from_mint"]
                    .as_str()
                    .or_else(|| args["mint"].as_str())
                    .unwrap_or("");
                let _to = args["to_mint"].as_str().unwrap_or("");
                let amount = args["amount"].as_f64().unwrap_or(0.0);
                // Try real Jupiter swap
                match swap_token(from, amount, 50, 100, config).await {
                    Ok(result) => Ok(serde_json::to_string_pretty(&result)?),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string(),
                        "note": "Swap failed — Jupiter API error or signing needed",
                    })
                    .to_string()),
                }
            }

            // ── Add/Withdraw Liquidity (stubs — need SDK) ─────
            "add_liquidity" => {
                let pid = args["position_id"].as_str().unwrap_or("");
                let amount = args["amount_sol"].as_f64().unwrap_or(0.0);
                Ok(json!({
                    "success": false,
                    "position_id": pid,
                    "amount_sol": amount,
                    "note": "add_liquidity not yet implemented — needs DLMM SDK",
                })
                .to_string())
            }
            "withdraw_liquidity" => {
                let pid = args["position_id"].as_str().unwrap_or("");
                let pct = args["pct"].as_f64().unwrap_or(0.0);
                Ok(json!({
                    "success": false,
                    "position_id": pid,
                    "pct": pct,
                    "note": "withdraw_liquidity not yet implemented — needs DLMM SDK",
                })
                .to_string())
            }
            "update_config" => {
                let key = args["key"].as_str().unwrap_or("");
                let value = &args["value"];
                // Record as lesson (self-tuning)
                let lesson = format!("[SELF-TUNED] Config update: {} = {}", key, value);
                info("executor", &lesson);
                Ok(json!({
                    "success": true,
                    "key": key,
                    "value": value,
                    "note": "Config update recorded as lesson. Runtime override requires restart.",
                })
                .to_string())
            }

            // ── Unknown ────────────────────────────────────────
            _ => Ok(format!("Unknown tool: {}", name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lessons::PerformanceInput;
    use crate::state::positions::TrackedPosition;
    use crate::tools::wallet::{TokenBalance, WalletBalances};
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn clear(keys: &'static [&'static str]) -> Self {
            let saved = keys
                .iter()
                .map(|key| (*key, std::env::var(key).ok()))
                .collect::<Vec<_>>();
            for key in keys {
                std::env::remove_var(key);
            }
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.saved {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn unique_test_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        std::env::temp_dir().join(format!("meridian-rs-{}-{}", label, nanos))
    }

    fn balances(tokens: Vec<TokenBalance>) -> WalletBalances {
        WalletBalances {
            wallet: Some("wallet".to_string()),
            sol: 0.0,
            sol_price: 0.0,
            sol_usd: 0.0,
            usdc: 0.0,
            tokens,
            total_usd: 0.0,
            error: None,
        }
    }

    #[test]
    fn check_smart_wallets_on_pool_dispatch_uses_pool_address_argument() {
        let _lock = ENV_LOCK.lock().expect("env test lock");
        let _env = EnvGuard::clear(&["MERIDIAN_DATA_DIR", "HELIUS_RPC_URL"]);
        let dir = unique_test_dir("smart-wallet-pool-dispatch");
        std::env::set_var("MERIDIAN_DATA_DIR", &dir);

        let result = tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(async {
                let mut executor = ToolExecutor::new("wallet");
                let config = Config::default();
                let mut positions = PositionState::default();
                let mut pool_memory = PoolMemoryStore::default();
                executor
                    .dispatch(
                        "check_smart_wallets_on_pool",
                        &json!({"pool_address": "PoolAddress111"}),
                        &config,
                        &mut positions,
                        &mut pool_memory,
                    )
                    .await
            })
            .expect("dispatch should accept pool_address");
        let value: Value =
            serde_json::from_str(&result).expect("smart wallet result should be json");

        assert_eq!(value["pool"], "PoolAddress111");
        assert_eq!(value["trackedWallets"], 0);
        assert_eq!(value["confidenceBoost"], false);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn close_post_effects_set_distinct_pool_and_token_cooldowns_on_loss() {
        let executor = ToolExecutor::new("wallet");
        let config = Config::default();
        let mut positions = PositionState::default();
        let mut pool_memory = PoolMemoryStore::default();

        let position = TrackedPosition {
            id: "pos-loss".to_string(),
            pool_address: "PoolLoss111".to_string(),
            base_mint: "MintLoss111".to_string(),
            base_symbol: Some("LOSS".to_string()),
            pnl_sol: Some(-0.25),
            ..TrackedPosition::default()
        };
        positions.positions.insert(position.id.clone(), position);
        pool_memory.add_note("PoolLoss111", "MintLoss111", Some("LOSS"), "seed");

        executor
            .run_post_effects(
                "close_position",
                &json!({"position_id": "pos-loss"}),
                r#"{"success":true}"#,
                &config,
                &mut positions,
                &mut pool_memory,
            )
            .await
            .expect("post effects should succeed");

        assert!(pool_memory.is_pool_on_cooldown("PoolLoss111"));
        assert!(pool_memory.is_base_mint_on_cooldown("MintLoss111"));
    }

    #[test]
    fn structured_decision_log_appends_redacted_args_and_parsed_result() {
        let dir = unique_test_dir("decision-log-structured");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let path = dir.join("decision-log.json");
        let args = json!({
            "pool_address": "Pool111",
            "wallet_private_key": "super-secret-key",
            "amount_sol": 0.25
        });

        append_decision_log_entry(
            &path,
            "deploy_position",
            &args,
            r#"{"success":false,"note":"DRY RUN"}"#,
            true,
        )
        .expect("decision entry should be appended");

        let log: Vec<Value> = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("decision-log should exist"),
        )
        .expect("decision-log should be a JSON array");
        assert_eq!(log.len(), 1);
        let entry = &log[0];
        assert_eq!(entry["schemaVersion"], 1);
        assert_eq!(entry["tool"], "deploy_position");
        assert_eq!(entry["args"]["pool_address"], "Pool111");
        assert_eq!(entry["args"]["wallet_private_key"], "***redacted***");
        assert_eq!(entry["result"]["note"], "DRY RUN");
        assert_eq!(
            entry["resultSummary"],
            "{\"success\":false,\"note\":\"DRY RUN\"}"
        );
        assert!(!entry["argsSummary"]
            .as_str()
            .expect("args summary should be text")
            .contains("super-secret-key"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn decision_log_path_uses_meridian_data_dir_and_keeps_rolling_window() {
        let _lock = ENV_LOCK.lock().expect("env test lock");
        let _env = EnvGuard::clear(&["MERIDIAN_DATA_DIR"]);
        let dir = unique_test_dir("decision-log-data-dir");
        std::env::set_var("MERIDIAN_DATA_DIR", &dir);

        for idx in 0..(MAX_DECISION_LOG + 2) {
            log_decision(
                "test_tool",
                &json!({"idx": idx}),
                &format!("result-{idx}"),
                idx % 2 == 0,
            );
        }

        let path = dir.join("decision-log.json");
        assert_eq!(decision_log_path(), path);
        let log: Vec<Value> = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("decision-log should be in data dir"),
        )
        .expect("decision-log should parse");
        assert_eq!(log.len(), MAX_DECISION_LOG);
        assert_eq!(log[0]["args"]["idx"], 2);
        assert_eq!(log.last().unwrap()["args"]["idx"], MAX_DECISION_LOG + 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn get_recent_decisions_tool_returns_newest_limited_entries() {
        let _lock = ENV_LOCK.lock().expect("env test lock");
        let _env = EnvGuard::clear(&["MERIDIAN_DATA_DIR"]);
        let dir = unique_test_dir("recent-decisions-tool");
        std::env::set_var("MERIDIAN_DATA_DIR", &dir);
        let path = decision_log_path();

        append_decision_log_entry(
            &path,
            "deploy_position",
            &json!({"pool_address": "PoolOld"}),
            r#"{"success":true,"note":"old"}"#,
            true,
        )
        .expect("old decision should append");
        append_decision_log_entry(
            &path,
            "close_position",
            &json!({"position_id": "PosNew"}),
            r#"{"success":false,"error":"new"}"#,
            false,
        )
        .expect("new decision should append");

        let mut executor = ToolExecutor::new("wallet");
        let mut positions = PositionState::default();
        let mut pool_memory = PoolMemoryStore::default();
        let output = tokio::runtime::Runtime::new()
            .expect("tokio runtime should start")
            .block_on(async {
                executor
                    .dispatch(
                        "get_recent_decisions",
                        &json!({"limit": 1}),
                        &Config::default(),
                        &mut positions,
                        &mut pool_memory,
                    )
                    .await
            })
            .expect("get_recent_decisions should return JSON");
        let parsed: Value = serde_json::from_str(&output).expect("output should be JSON");

        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["decisions"][0]["tool"], "close_position");
        assert_eq!(parsed["decisions"][0]["args"]["position_id"], "PosNew");
        assert_eq!(parsed["path"], path.display().to_string());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn execute_get_recent_decisions_does_not_log_itself() {
        let _lock = ENV_LOCK.lock().expect("env test lock");
        let _env = EnvGuard::clear(&["MERIDIAN_DATA_DIR"]);
        let dir = unique_test_dir("recent-decisions-no-recursion");
        std::env::set_var("MERIDIAN_DATA_DIR", &dir);
        let path = decision_log_path();
        append_decision_log_entry(
            &path,
            "deploy_position",
            &json!({"pool_address": "Pool111"}),
            r#"{"success":true}"#,
            true,
        )
        .expect("decision should append");

        let call = ToolCall {
            id: "call-1".to_string(),
            call_type: "function".to_string(),
            function: crate::llm::FunctionCall {
                name: "get_recent_decisions".to_string(),
                arguments: json!({"limit": 5}).to_string(),
            },
        };
        let mut executor = ToolExecutor::new("wallet");
        let mut positions = PositionState::default();
        let mut pool_memory = PoolMemoryStore::default();
        let (_output, is_error) = tokio::runtime::Runtime::new()
            .expect("tokio runtime should start")
            .block_on(async {
                executor
                    .execute(&call, &Config::default(), &mut positions, &mut pool_memory)
                    .await
            });

        assert!(!is_error);
        let log: Vec<Value> = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("decision-log should still exist"),
        )
        .expect("decision-log should parse");
        assert_eq!(log.len(), 1);
        assert_eq!(log[0]["tool"], "deploy_position");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn get_performance_history_tool_reads_lessons_store_from_data_dir() {
        let _lock = ENV_LOCK.lock().expect("env test lock");
        let _env = EnvGuard::clear(&["MERIDIAN_DATA_DIR"]);
        let dir = unique_test_dir("performance-history-tool");
        std::fs::create_dir_all(&dir).expect("test data dir should be created");
        std::env::set_var("MERIDIAN_DATA_DIR", &dir);

        let mut store = LessonStore::default();
        store.record_performance(PerformanceInput {
            position_id: "pos-1",
            pool: "pool-1",
            symbol: "PERF",
            pnl_sol: 0.07,
            fees_earned: 0.01,
            range_efficiency: 0.85,
            close_reason: "take_profit",
            signal_snapshot: "{}",
        });
        let lessons_path = crate::config::meridian_data_path("lessons.json");
        store
            .save(lessons_path.to_str().expect("path should be utf-8"))
            .expect("lessons store should save");

        let mut executor = ToolExecutor::new("wallet");
        let mut positions = PositionState::default();
        let mut pool_memory = PoolMemoryStore::default();
        let output = tokio::runtime::Runtime::new()
            .expect("tokio runtime should start")
            .block_on(async {
                executor
                    .dispatch(
                        "get_performance_history",
                        &json!({"hours": 24, "limit": 5}),
                        &Config::default(),
                        &mut positions,
                        &mut pool_memory,
                    )
                    .await
            })
            .expect("get_performance_history should return JSON");
        let parsed: Value = serde_json::from_str(&output).expect("output should be JSON");

        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["history"]["count"], 1);
        assert_eq!(parsed["history"]["positions"][0]["symbol"], "PERF");
        assert_eq!(parsed["path"], lessons_path.display().to_string());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn top_lper_dispatch_uses_dry_run_no_network_fallback() {
        let mut executor = ToolExecutor::new("wallet");
        let mut positions = PositionState::default();
        let mut pool_memory = PoolMemoryStore::default();
        let config = Config {
            dry_run: true,
            ..Config::default()
        };
        let output = tokio::runtime::Runtime::new()
            .expect("tokio runtime should start")
            .block_on(async {
                executor
                    .dispatch(
                        "study_top_lpers",
                        &json!({"pool_address": "Pool111", "limit": 3}),
                        &config,
                        &mut positions,
                        &mut pool_memory,
                    )
                    .await
            })
            .expect("study_top_lpers should return JSON in dry-run mode");
        let parsed: Value = serde_json::from_str(&output).expect("output should be JSON");

        assert_eq!(parsed["pool"], "Pool111");
        assert!(parsed["message"]
            .as_str()
            .expect("message should be present")
            .contains("No LPAgent top LPer data"));
        assert!(parsed["lpers"].as_array().unwrap().is_empty());
    }

    #[test]
    fn auto_swap_plan_uses_close_base_mint_when_value_is_not_dust() {
        let close_result = json!({"success": true, "baseMint": "TokenMint111"});
        let wallet_balances = balances(vec![TokenBalance {
            mint: "TokenMint111".to_string(),
            symbol: "TOK".to_string(),
            balance: 12.5,
            usd: Some(0.11),
        }]);

        let plan = plan_auto_swap_after_close(&close_result, &wallet_balances, false)
            .expect("token worth >= $0.10 should be auto-swapped");

        assert_eq!(plan.mint, "TokenMint111");
        assert_eq!(plan.amount, 12.5);
        assert_eq!(plan.usd, 0.11);
    }

    #[test]
    fn auto_swap_plan_respects_skip_swap_dust_and_sol_mint() {
        let token_result = json!({"success": true, "baseMint": "TokenMint111"});
        let wallet_balances = balances(vec![TokenBalance {
            mint: "TokenMint111".to_string(),
            symbol: "TOK".to_string(),
            balance: 12.5,
            usd: Some(0.09),
        }]);

        assert!(plan_auto_swap_after_close(&token_result, &wallet_balances, true).is_none());
        assert!(plan_auto_swap_after_close(&token_result, &wallet_balances, false).is_none());

        let sol_result =
            json!({"success": true, "baseMint": "So11111111111111111111111111111111111111112"});
        assert!(plan_auto_swap_after_close(&sol_result, &wallet_balances, false).is_none());
    }

    #[test]
    fn enrich_close_result_adds_base_mint_from_tracked_position() {
        let mut positions = PositionState::default();
        positions.positions.insert(
            "pos-1".to_string(),
            TrackedPosition {
                id: "pos-1".to_string(),
                pool_address: "pool-1".to_string(),
                pool_name: Some("POOL/TOK".to_string()),
                base_mint: "TokenMint111".to_string(),
                base_symbol: Some("TOK".to_string()),
                ..Default::default()
            },
        );
        let args = json!({"position_id": "pos-1"});
        let mut result = json!({"success": true, "position": "pos-1"});

        enrich_close_result_with_position_metadata(&mut result, &args, &positions);

        assert_eq!(result["baseMint"], "TokenMint111");
        assert_eq!(result["pool"], "pool-1");
        assert_eq!(result["poolName"], "POOL/TOK");
    }

    #[tokio::test]
    async fn deploy_dispatch_calls_dlmm_deploy_instead_of_stub() {
        let mut executor = ToolExecutor::new("wallet");
        let mut positions = PositionState::default();
        let mut pool_memory = PoolMemoryStore::default();
        let config = Config {
            dry_run: true,
            ..Default::default()
        };
        let args = json!({
            "pool_address": "22222222222222222222222222222222",
            "amount_sol": 0.25,
            "bins_below": 35,
            "bins_above": 0,
            "strategy": "spot"
        });

        let output = executor
            .dispatch(
                "deploy_position",
                &args,
                &config,
                &mut positions,
                &mut pool_memory,
            )
            .await
            .expect("deploy dispatch should return a JSON result");

        assert!(
            !output.contains("not yet implemented"),
            "got stale stub: {output}"
        );
        let parsed: Value = serde_json::from_str(&output).expect("deploy output should be JSON");
        assert_eq!(parsed["success"], true);
    }

    #[test]
    fn strategy_tool_dispatch_round_trips_in_data_dir() {
        let _lock = ENV_LOCK.lock().expect("env test lock");
        let _env = EnvGuard::clear(&["MERIDIAN_DATA_DIR"]);
        let dir = unique_test_dir("strategy-tool-dispatch");
        std::env::set_var("MERIDIAN_DATA_DIR", &dir);

        let mut executor = ToolExecutor::new("wallet");
        let mut positions = PositionState::default();
        let mut pool_memory = PoolMemoryStore::default();
        let config = Config::default();
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should start");

        let add = runtime
            .block_on(async {
                executor
                    .dispatch(
                        "add_strategy",
                        &json!({
                            "id": "Panda Strat!",
                            "name": "Panda Strat",
                            "author": "top-lper",
                            "lp_strategy": "curve",
                            "token_criteria": {"min_mcap": 150000},
                            "entry": {"condition": "after pullback"},
                            "range": {"type": "wide"},
                            "exit": {"take_profit_pct": 8},
                            "best_for": "volatile narrative pools",
                            "raw": "tweet text"
                        }),
                        &config,
                        &mut positions,
                        &mut pool_memory,
                    )
                    .await
            })
            .expect("add_strategy dispatch should succeed");
        let add_json: Value =
            serde_json::from_str(&add).expect("add_strategy output should be JSON");
        assert_eq!(add_json["id"], "panda_strat");

        runtime
            .block_on(async {
                executor
                    .dispatch(
                        "set_active_strategy",
                        &json!({"id": "panda_strat"}),
                        &config,
                        &mut positions,
                        &mut pool_memory,
                    )
                    .await
            })
            .expect("set_active_strategy dispatch should succeed");

        let list = runtime
            .block_on(async {
                executor
                    .dispatch(
                        "list_strategies",
                        &json!({}),
                        &config,
                        &mut positions,
                        &mut pool_memory,
                    )
                    .await
            })
            .expect("list_strategies dispatch should succeed");
        let list_json: Value = serde_json::from_str(&list).expect("list output should be JSON");
        assert_eq!(list_json["active"], "panda_strat");
        assert_eq!(list_json["count"], 6);
        assert_eq!(
            crate::strategy_library::strategy_library_path(),
            dir.join("strategy-library.json")
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
