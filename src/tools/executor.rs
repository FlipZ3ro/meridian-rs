use anyhow::Result;
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;

use crate::config::Config;
use crate::llm::ToolCall;
use crate::state::pool_memory::PoolMemoryStore;
use crate::state::positions::PositionState;
use crate::tools::dlmm::{get_active_bin, get_my_positions, get_position_pnl, get_wallet_positions, search_pools};
use crate::tools::screening::Screener;
use crate::tools::token::{get_token_info, get_token_holders, get_token_narrative, check_smart_wallets_on_pool};
use crate::tools::wallet::{get_wallet_balances, swap_token, normalize_mint};
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

const DECISION_LOG_PATH: &str = "decision-log.json";
const MAX_DECISION_LOG: usize = 100;

// ─── Decision Log ───────────────────────────────────────────────

fn log_decision(tool: &str, args: &Value, result: &str, success: bool) {
    let entry = json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "tool": tool,
        "args_summary": args_summary(args),
        "success": success,
        "result_summary": truncate(result, 200),
    });

    // Read existing log
    let mut log: Vec<Value> = std::fs::read_to_string(DECISION_LOG_PATH)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();

    log.push(entry);

    // Keep rolling window
    if log.len() > MAX_DECISION_LOG {
        let excess = log.len() - MAX_DECISION_LOG;
        log.drain(..excess);
    }

    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .truncate(true)
        .open(DECISION_LOG_PATH)
    {
        if let Ok(content) = serde_json::to_string_pretty(&log) {
            let _ = f.write_all(content.as_bytes());
        }
    }
}

fn args_summary(args: &Value) -> String {
    match args {
        Value::Object(map) => {
            let pairs: Vec<String> = map.iter().take(5).map(|(k, v)| {
                match v {
                    Value::String(s) => format!("{}={}", k, truncate(s, 40)),
                    Value::Number(n) => format!("{}={}", k, n),
                    Value::Bool(b) => format!("{}={}", k, b),
                    _ => format!("{}=...", k),
                }
            }).collect();
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
        } else if LOCK_ON_SUCCESS.contains(&name) && success {
            if !self.executed_once.contains(&name.to_string()) {
                self.executed_once.push(name.to_string());
            }
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
            if let Err(e) = self.run_safety_checks(name, &args, config, positions).await {
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
                let is_ok = !msg.contains("\"success\":false") && !msg.contains("error") && !msg.contains("STUB");
                self.maybe_lock(name, is_ok);
                (msg, false)
            }
            Err(e) => {
                self.maybe_lock(name, false);
                (format!("Tool {} error: {}", name, e), true)
            }
        };

        // ── Audit log ──────────────────────────────────────────
        log_decision(name, &args, &msg, !is_error);

        // ── Post-effects ───────────────────────────────────────
        if !is_error {
            if let Err(e) = self.run_post_effects(name, &args, &msg, config, positions, pool_memory).await {
                warn("executor", &format!("Post-effect error for {}: {}", name, e));
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
    ) -> Result<()> {
        match name {
            "deploy_position" => {
                let pool_addr = args["pool_address"].as_str().unwrap_or("");
                if pool_addr.is_empty() {
                    anyhow::bail!("pool_address required");
                }

                // Amount validation
                let amount = args["amount_y"].as_f64()
                    .or(args["amount_sol"].as_f64())
                    .unwrap_or(0.0);
                if amount <= 0.0 {
                    anyhow::bail!("amount must be > 0");
                }
                if amount > config.risk.max_deploy_amount {
                    anyhow::bail!("amount {} exceeds max {}", amount, config.risk.max_deploy_amount);
                }

                // Max positions check
                let active_count = positions.count_active();
                if active_count >= config.risk.max_positions as usize {
                    anyhow::bail!("at max positions ({}/{})", active_count, config.risk.max_positions);
                }

                // Duplicate pool check
                let active = positions.get_active();
                if active.iter().any(|p| p.pool_address == pool_addr) {
                    anyhow::bail!("already have position in pool {}", &pool_addr[..12.min(pool_addr.len())]);
                }

                // Duplicate base mint check (if provided)
                if let Some(base_mint) = args["base_mint"].as_str() {
                    if !base_mint.is_empty() {
                        let normalized = normalize_mint(base_mint);
                        let has_dup = active.iter().any(|p| normalize_mint(&p.base_mint) == normalized);
                        if has_dup {
                            anyhow::bail!("already have position with same base token");
                        }
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
                if let Ok(Some(detail)) = self.screener.get_pool_detail(pool_addr, &config.screening.timeframe).await {
                    let tvl = detail.tvl.or(detail.active_tvl).unwrap_or(0.0);
                    if tvl < config.screening.min_tvl {
                        anyhow::bail!("pool TVL ${:.0} below min ${:.0}", tvl, config.screening.min_tvl);
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
                        anyhow::bail!("pool fee/TVL {:.4} below min {:.4}", fee_tvl, config.screening.min_fee_active_tvl_ratio);
                    }
                } else {
                    anyhow::bail!("could not verify pool before deploy");
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
                        let note = format!("CLOSED low yield — avoid re-entry. Fees insufficient for TVL.");
                        pool_memory.add_note(&pool, &pos.base_mint, Some(&sym), &note);
                        info("executor", &format!("Auto-annotated pool memory for low-yield close: {}", sym));
                    }
                }

                // Auto-swap base token back to SOL after close
                if config.management.sol_mode {
                    // Extract base_mint from result if available
                    if let Ok(parsed) = serde_json::from_str::<Value>(result) {
                        if let Some(base_mint) = parsed["base_mint"].as_str() {
                            let normalized = normalize_mint(base_mint);
                            if normalized != "So11111111111111111111111111111111111111112" {
                                info("executor", &format!("Auto-swap: {} -> SOL after close", &normalized[..8.min(normalized.len())]));
                                // NOTE: Actual swap would need wallet keypair — here we log the intent
                                // In production, this would call swap_token with the closed position's base_mint
                            }
                        }
                    }
                }
            }
            "claim_fees" => {
                // Auto-swap claimed tokens to SOL if enabled
                if config.management.sol_mode {
                    if let Ok(parsed) = serde_json::from_str::<Value>(result) {
                        if let Some(base_mint) = parsed["base_mint"].as_str() {
                            let normalized = normalize_mint(base_mint);
                            if normalized != "So11111111111111111111111111111111111111112" {
                                info("executor", &format!("Auto-swap: claimed {} -> SOL", &normalized[..8.min(normalized.len())]));
                            }
                        }
                    }
                }
            }
            "deploy_position" => {
                // Record in pool memory
                if let Some(pool) = args["pool_address"].as_str() {
                    let sym = args["symbol"].as_str().unwrap_or("unknown");
                    let note = format!("DEPLOYED — bins_below={}, amount={}",
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
            "get_wallet_balance" => {
                let wallet = args["wallet"].as_str().unwrap_or(&self.wallet_address);
                let rpc = config.api.helius_rpc_url.as_deref().unwrap_or("https://api.mainnet-beta.solana.com");
                let helius_key = config.api.helius_api_key.as_deref().unwrap_or("");
                match get_wallet_balances(rpc, wallet, helius_key).await {
                    Ok(balances) => Ok(serde_json::to_string_pretty(&balances)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "get_my_positions" => {
                match get_my_positions(&self.wallet_address, config).await {
                    Ok(result) => Ok(serde_json::to_string_pretty(&result)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
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
                    anyhow::bail!("pool_address and position_address required (or position_id from state)");
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
                match self.screener.get_top_candidates(&config.screening, limit).await {
                    Ok(candidates) => Ok(serde_json::to_string_pretty(&candidates)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "discover_pools" => {
                let page_size = args["page_size"].as_u64().unwrap_or(50) as u32;
                match self.screener.discover_pools(&config.screening, page_size).await {
                    Ok(pools) => Ok(json!({"count": pools.len(), "pools_count": pools.len()}).to_string()),
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
                if mint.is_empty() { anyhow::bail!("mint required"); }
                match get_token_info(mint).await {
                    Ok(info) => Ok(serde_json::to_string_pretty(&info)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "get_token_holders" => {
                let mint = args["mint"].as_str().unwrap_or("");
                if mint.is_empty() { anyhow::bail!("mint required"); }
                match get_token_holders(mint, 20).await {
                    Ok(holders) => Ok(serde_json::to_string_pretty(&holders)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "get_token_narrative" => {
                let mint = args["mint"].as_str().unwrap_or("");
                if mint.is_empty() { anyhow::bail!("mint required"); }
                match get_token_narrative(mint).await {
                    Ok(narrative) => Ok(serde_json::to_string_pretty(&narrative)?),
                    Err(e) => Ok(format!("{{\"error\": \"{}\"}}", e)),
                }
            }
            "check_smart_wallets_on_pool" => {
                let mint = args["mint"].as_str().unwrap_or("");
                if mint.is_empty() { anyhow::bail!("mint required"); }
                match check_smart_wallets_on_pool(mint).await {
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
                Ok(format!("Pool note added for {} ({}).", sym, &pool[..12.min(pool.len())]))
            }

            // ── Blacklist ──────────────────────────────────────
            "blacklist_token" => {
                let mint = args["mint"].as_str().unwrap_or("");
                let reason = args["reason"].as_str().unwrap_or("manual");
                if mint.is_empty() { anyhow::bail!("mint required"); }
                match crate::tools::blacklist::add_to_blacklist(mint, Some("agent"), Some(reason)) {
                    Ok(_) => Ok(format!("Token {} blacklisted.", &mint[..12.min(mint.len())])),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }
            "unblock_dev" => {
                let wallet = args["wallet"].as_str().unwrap_or("");
                if wallet.is_empty() { anyhow::bail!("wallet required"); }
                match crate::tools::blacklist::unblock_dev(wallet) {
                    Ok(_) => Ok(format!("Dev {} unblocked.", &wallet[..12.min(wallet.len())])),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            // ── Mutable tools (stubs — need SDK) ───────────────
            "deploy_position" => {
                let pool = args["pool_address"].as_str().unwrap_or("");
                let amount = args["amount_y"].as_f64().or(args["amount_sol"].as_f64()).unwrap_or(0.0);
                let bins = args["bins_below"].as_i64().unwrap_or(20);
                Ok(json!({
                    "success": false,
                    "pool": pool,
                    "amount_sol": amount,
                    "bins_below": bins,
                    "note": "Deploy not yet implemented — needs Meteora DLMM SDK Rust binding",
                    "wallet": self.wallet_address,
                }).to_string())
            }
            "close_position" => {
                let pid = args["position_id"].as_str().unwrap_or("");
                Ok(json!({
                    "success": false,
                    "position_id": pid,
                    "note": "Close not yet implemented — needs DLMM SDK + signing",
                }).to_string())
            }
            "claim_fees" => {
                let pid = args["position_id"].as_str().unwrap_or("");
                Ok(json!({
                    "success": false,
                    "position_id": pid,
                    "note": "Claim not yet implemented — needs DLMM SDK",
                }).to_string())
            }
            "swap_token" => {
                let from = args["from_mint"].as_str().unwrap_or("");
                let to = args["to_mint"].as_str().unwrap_or("");
                let amount = args["amount"].as_f64().unwrap_or(0.0);
                // Try real Jupiter swap
                match swap_token(from, amount, 50, 100, config).await {
                    Ok(result) => Ok(serde_json::to_string_pretty(&result)?),
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string(),
                        "note": "Swap failed — Jupiter API error or signing needed",
                    }).to_string()),
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
                }).to_string())
            }
            "withdraw_liquidity" => {
                let pid = args["position_id"].as_str().unwrap_or("");
                let pct = args["pct"].as_f64().unwrap_or(0.0);
                Ok(json!({
                    "success": false,
                    "position_id": pid,
                    "pct": pct,
                    "note": "withdraw_liquidity not yet implemented — needs DLMM SDK",
                }).to_string())
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
                }).to_string())
            }

            // ── Unknown ────────────────────────────────────────
            _ => Ok(format!("Unknown tool: {}", name)),
        }
    }
}
