use crate::agent::loop_::AgentLoop;
use crate::agent::prompt::AgentRole;
use crate::config::loader::compute_deploy_amount;
use crate::config::Config;
use crate::llm::LlmClient;
use crate::state::pool_memory::PoolMemoryStore;
use crate::state::positions::{
    get_deterministic_close_rule, minutes_out_of_range, position_age_minutes,
    resolve_pending_peak_with_pnl, resolve_pending_trailing_drop, update_trailing_state,
    CloseRule, PositionState,
};
use crate::tools::dlmm::get_my_positions;
use crate::utils::logger::module::{info, warn};
use anyhow::Result;
use std::collections::HashMap;

// ─── Action types from deterministic evaluation ─────────────────

#[derive(Debug, Clone)]
pub enum PositionAction {
    Close { rule: u8, reason: String },
    Claim,
    TrailingExit { reason: String },
    Instruction,
    Stay,
}

impl PositionAction {
    pub fn as_str(&self) -> &str {
        match self {
            PositionAction::Close { .. } => "CLOSE",
            PositionAction::Claim => "CLAIM",
            PositionAction::TrailingExit { .. } => "TRAILING_TP",
            PositionAction::Instruction => "INSTRUCTION",
            PositionAction::Stay => "STAY",
        }
    }
}

// ─── Position PnL data snapshot ─────────────────────────────────

#[derive(Debug, Clone)]
pub struct PositionPnlData {
    pub position_address: String,
    pub pool_address: String,
    pub pair: String,
    pub pnl_pct: Option<f64>,
    pub in_range: bool,
    pub fee_per_tvl_24h: Option<f64>,
    pub total_value_usd: Option<f64>,
    pub unclaimed_fees_usd: Option<f64>,
    pub age_minutes: Option<u32>,
    pub minutes_out_of_range: u32,
    pub active_bin: Option<i32>,
    pub upper_bin: Option<i32>,
    pub lower_bin: Option<i32>,
    pub instruction: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════
//  MANAGEMENT CYCLE (deterministic rules + LLM for actions only)
// ═══════════════════════════════════════════════════════════════════

pub async fn run_management_cycle(
    config: &Config,
    llm: &LlmClient,
    positions: &mut PositionState,
    pool_memory: &mut PoolMemoryStore,
    wallet_address: &str,
) -> Result<String> {
    info("cycle", "Management Cycle Starting");

    let active_count = positions.count_active();
    if active_count == 0 {
        info("cycle", "No active positions to manage.");
        return Ok("No active positions.".to_string());
    }

    // ── 1. Sync positions with on-chain state ──────────────────
    match get_my_positions(wallet_address, config).await {
        Ok(result) => {
            if !result.positions.is_empty() {
                let addrs: Vec<String> = result.positions.iter().map(|p| p.position.clone()).collect();
                positions.sync_open_positions(addrs);
            }
        }
        Err(e) => {
            warn("cycle", &format!("Position sync skipped: {}", e));
        }
    }

    // ── 2. Build position data + update trailing state ─────────
    // Clone the position data we need to avoid borrow conflicts
    let pos_snapshots: Vec<(String, Option<f64>, bool, Option<f64>, Option<String>, i32, i32, i32, u32, u32)> =
        positions
            .get_active()
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    p.pnl_sol.map(|_| 0.0), // pnl_pct placeholder
                    p.out_of_range_since.is_none(),
                    None, // fee_per_tvl_24h placeholder
                    p.instruction.clone(),
                    p.upper_bin,
                    p.lower_bin,
                    0,    // active_bin placeholder
                    minutes_out_of_range(p),
                    position_age_minutes(p),
                )
            })
            .collect();

    // Update trailing state for each position
    for (addr, pnl_opt, _, _, _, _, _, _, _, _) in &pos_snapshots {
        if let Some(pnl) = pnl_opt {
            if let Some(pos) = positions.positions.get_mut(addr) {
                update_trailing_state(pos, *pnl, config.management.trailing_trigger_pct, config.management.trailing_drop_pct);
                resolve_pending_peak_with_pnl(pos, *pnl);
                resolve_pending_trailing_drop(pos, *pnl, config.management.trailing_drop_pct, 1.0);
            }
        }
    }

    // ── 3. Check trailing TP exits ─────────────────────────────
    let mut exit_map: HashMap<String, String> = HashMap::new();
    for (addr, _, _, _, _, _, _, _, _, _) in &pos_snapshots {
        if let Some(pos) = positions.positions.get(addr) {
            if let (Some(ref reason), Some(ref until)) = (&pos.trailing.confirmed_trailing_exit_reason, &pos.trailing.confirmed_trailing_exit_until) {
                if chrono::Utc::now().to_rfc3339() < *until {
                    exit_map.insert(addr.clone(), reason.clone());
                }
            }
        }
    }

    // ── 4. Build action map using deterministic rules ──────────
    let mut action_map: HashMap<String, PositionAction> = HashMap::new();
    for (addr, pnl_opt, in_range, fee_tvl_opt, instruction, upper, lower, active_bin, oor_mins, age_mins) in &pos_snapshots {
        // Trailing exit — highest priority
        if let Some(reason) = exit_map.get(addr) {
            action_map.insert(addr.clone(), PositionAction::TrailingExit { reason: reason.clone() });
            continue;
        }
        // Instruction
        if instruction.is_some() {
            action_map.insert(addr.clone(), PositionAction::Instruction);
            continue;
        }
        // Deterministic close rules
        if let (Some(pnl), Some(fee_tvl)) = (pnl_opt, fee_tvl_opt) {
            if let Some(pos) = positions.positions.get(addr) {
                if let Some(rule) = get_deterministic_close_rule(pos, *active_bin, *pnl, *fee_tvl, *oor_mins, config) {
                    let (rule_num, reason) = match rule {
                        CloseRule::StopLoss => (1u8, "stop loss"),
                        CloseRule::TakeProfit => (2, "take profit"),
                        CloseRule::PumpedAboveRange => (3, "pumped far above range"),
                        CloseRule::OutOfRange => (4, "OOR"),
                        CloseRule::LowYield => (5, "low yield"),
                        CloseRule::TrailingTp => (0, "trailing TP"),
                    };
                    action_map.insert(addr.clone(), PositionAction::Close { rule: rule_num, reason: reason.to_string() });
                    continue;
                }
            }
        }
        action_map.insert(addr.clone(), PositionAction::Stay);
    }

    // ── 5. Build report ────────────────────────────────────────
    let cur = if config.management.sol_mode { "◎" } else { "$" };
    let needs_action: Vec<_> = action_map.values().filter(|a| !matches!(a, PositionAction::Stay)).collect();
    let action_summary = if needs_action.is_empty() {
        "no action".to_string()
    } else {
        needs_action.iter().map(|a| a.as_str().to_string()).collect::<Vec<_>>().join(", ")
    };

    let mgmt_report = format!("Summary: 💼 {} positions | {}", active_count, action_summary);

    if needs_action.is_empty() {
        info("cycle", "Management: no actions needed");
        return Ok(mgmt_report);
    }

    info("cycle", &format!("Management: {} action(s) needed — invoking LLM", needs_action.len()));

    // Build LLM goal with action details
    let action_blocks: Vec<String> = pos_snapshots
        .iter()
        .filter(|(addr, _, _, _, _, _, _, _, _, _)| !matches!(action_map.get(addr), Some(PositionAction::Stay) | None))
        .map(|(addr, pnl, _, fee_tvl, _, upper, lower, active, oor, _)| {
            let act = action_map.get(addr).unwrap_or(&PositionAction::Stay);
            format!(
                "POSITION: {}\n  action: {}\n  pnl_pct: {:.2}% | fee_per_tvl: {:.4}% | bins: [{},{}] active={} | oor: {}m",
                addr, act.as_str(), pnl.unwrap_or(0.0), fee_tvl.unwrap_or(0.0), lower, upper, active, oor
            )
        })
        .collect();

    let goal = format!(
        "MANAGEMENT ACTION REQUIRED — {} position(s)\n\n{}\n\nExecute the required actions.",
        needs_action.len(),
        action_blocks.join("\n\n")
    );

    let agent = AgentLoop::new();
    let result = agent.run(&goal, AgentRole::Manager, config, llm, positions, pool_memory, wallet_address).await?;

    info("cycle", &format!("Management result: {}", &result[..result.len().min(300)]));
    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════
//  PnL POLLER (30s lightweight cycle, no LLM)
// ═══════════════════════════════════════════════════════════════════

pub async fn run_pnl_poll(
    config: &Config,
    positions: &mut PositionState,
    wallet_address: &str,
) -> Result<Vec<(String, String)>> {
    let active = positions.get_active();
    if active.is_empty() {
        return Ok(vec![]);
    }

    // Fetch real PnL + active_bin for each position
    let mut pos_data: Vec<(String, Option<f64>, bool, i32, i32, u32, Option<f64>)> = Vec::new();
    for p in &active {
        let mut pnl_sol: Option<f64> = None;
        let mut fee_tvl: Option<f64> = None;
        let mut active_bin = 0i32;

        // Fetch real PnL
        if let Ok(pnl_result) = crate::tools::dlmm::get_position_pnl(
            &p.pool_address,
            &p.id,
            wallet_address,
        ).await {
            // Use pnl_usd as proxy; convert via config sol price if available
            pnl_sol = pnl_result.pnl_usd;
            fee_tvl = pnl_result.fee_per_tvl_24h;
            // Also try to get active_bin from PnL response
            if let Some(ab) = pnl_result.active_bin {
                active_bin = ab;
            }
        }

        // Fetch real active bin
        if let Ok(bin_result) = crate::tools::dlmm::get_active_bin(&p.pool_address).await {
            active_bin = bin_result.bin_id;
        }

        let in_range = active_bin >= p.lower_bin && active_bin <= p.upper_bin;

        pos_data.push((
            p.id.clone(),
            pnl_sol,
            in_range,
            p.upper_bin,
            p.lower_bin,
            minutes_out_of_range(p),
            fee_tvl,
        ));
    }
 
    let mut exits_needed: Vec<(String, String)> = vec![];

    for (addr, pnl_opt, in_range, upper, _lower, oor_mins, fee_tvl_opt) in &pos_data {
        // Update trailing state
        if let Some(pnl) = pnl_opt {
            if let Some(pos) = positions.positions.get_mut(addr) {
                update_trailing_state(pos, *pnl, config.management.trailing_trigger_pct, config.management.trailing_drop_pct);
                resolve_pending_peak_with_pnl(pos, *pnl);
                resolve_pending_trailing_drop(pos, *pnl, config.management.trailing_drop_pct, 1.0);

                // Check confirmed trailing exit
                if let (Some(ref reason), Some(ref until)) = (&pos.trailing.confirmed_trailing_exit_reason, &pos.trailing.confirmed_trailing_exit_until) {
                    if chrono::Utc::now().to_rfc3339() < *until {
                        exits_needed.push((addr.clone(), reason.clone()));
                        let pos_mut = positions.positions.get_mut(addr).unwrap();
                        pos_mut.trailing.confirmed_trailing_exit_reason = None;
                        pos_mut.trailing.confirmed_trailing_exit_until = None;
                    }
                }
            }
        }

        // Check deterministic close rules
        if let (Some(pnl), fee_tvl) = (pnl_opt, fee_tvl_opt.unwrap_or(0.001)) {
            if let Some(pos) = positions.positions.get(addr) {
                if let Some(rule) = get_deterministic_close_rule(pos, 0, *pnl, fee_tvl, *oor_mins, config) {
                    let reason = match rule {
                        CloseRule::StopLoss => "stop loss",
                        CloseRule::TakeProfit => "take profit",
                        CloseRule::PumpedAboveRange => "pumped far above range",
                        CloseRule::OutOfRange => "OOR",
                        CloseRule::LowYield => "low yield",
                        CloseRule::TrailingTp => "trailing TP",
                    };
                    if !exits_needed.iter().any(|(a, _)| a == addr) {
                        exits_needed.push((addr.clone(), reason.to_string()));
                    }
                }
            }
        }

        // Update OOR state
        if !in_range {
            positions.mark_oor(addr);
        } else {
            positions.mark_in_range(addr);
        }
    }

    if !exits_needed.is_empty() {
        info("pnl_poll", &format!("{} exit(s) detected", exits_needed.len()));
    }

    Ok(exits_needed)
}

// ═══════════════════════════════════════════════════════════════════
//  SCREENING CYCLE
// ═══════════════════════════════════════════════════════════════════

pub async fn run_screening_cycle(
    config: &Config,
    llm: &LlmClient,
    positions: &mut PositionState,
    pool_memory: &mut PoolMemoryStore,
    wallet_sol: f64,
    wallet_address: &str,
) -> Result<String> {
    info("cycle", "Screening Cycle Starting");

    let active_count = positions.count_active();
    if active_count >= config.risk.max_positions as usize {
        let msg = format!("At max positions ({}/{}). Skipping.", active_count, config.risk.max_positions);
        info("cycle", &msg);
        return Ok(msg);
    }

    let deploy_amount = compute_deploy_amount(config, wallet_sol);
    if deploy_amount <= 0.0 {
        let msg = format!("Not enough SOL ({:.2}) to deploy.", wallet_sol);
        info("cycle", &msg);
        return Ok(msg);
    }

    let goal = format!(
        "Screen Meteora DLMM pools and deploy {:.4} SOL to the best candidate.          Active positions: {}. Max: {}.          Use get_top_candidates, then call deploy_position with amount_y={:.4}.          Deploy ONLY if a candidate passes ALL thresholds.",
        deploy_amount, active_count, config.risk.max_positions, deploy_amount,
    );

    let agent = AgentLoop::new();
    let result = agent.run(&goal, AgentRole::Screener, config, llm, positions, pool_memory, wallet_address).await?;

    info("cycle", &format!("Screening result: {}", &result[..result.len().min(300)]));
    Ok(result)
}
