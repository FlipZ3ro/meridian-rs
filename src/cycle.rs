use crate::agent::loop_::{AgentLoop, AgentRunContext};
use crate::agent::prompt::AgentRole;
use crate::config::loader::compute_deploy_amount;
use crate::config::Config;
use crate::llm::LlmClient;
use crate::state::pool_memory::PoolMemoryStore;
use crate::state::positions::{
    get_deterministic_close_rule, minutes_out_of_range, resolve_pending_peak_with_pnl,
    resolve_pending_trailing_drop, update_trailing_state, CloseRule, PositionState,
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

#[derive(Debug, Clone)]
struct ManagementPositionSnapshot {
    id: String,
    pnl_pct: Option<f64>,
    fee_tvl: Option<f64>,
    unclaimed_fees_usd: Option<f64>,
    instruction: Option<String>,
    upper_bin: i32,
    lower_bin: i32,
    active_bin: i32,
    minutes_out_of_range: u32,
}

#[derive(Debug, Clone)]
struct PnlPollSnapshot {
    id: String,
    pnl_pct: Option<f64>,
    in_range: bool,
    minutes_out_of_range: u32,
    fee_tvl: Option<f64>,
    active_bin: i32,
}

fn format_management_action_block(
    snapshot: &ManagementPositionSnapshot,
    act: &PositionAction,
) -> String {
    let mut block = format!(
        "POSITION: {}\n  action: {}\n  pnl_pct: {:.2}% | fee_per_tvl: {:.4}% | bins: [{},{}] active={} | oor: {}m",
        snapshot.id,
        act.as_str(),
        snapshot.pnl_pct.unwrap_or(0.0),
        snapshot.fee_tvl.unwrap_or(0.0),
        snapshot.lower_bin,
        snapshot.upper_bin,
        snapshot.active_bin,
        snapshot.minutes_out_of_range
    );

    match act {
        PositionAction::Close { rule, reason } => {
            block.push_str(&format!("\n  close_rule: {} ({})", rule, reason));
        }
        PositionAction::TrailingExit { reason } => {
            block.push_str(&format!("\n  trailing_exit_reason: {}", reason));
        }
        _ => {}
    }

    if let Some(instruction) = snapshot.instruction.as_deref() {
        if !instruction.trim().is_empty() {
            block.push_str(&format!("\n  instruction: {}", instruction));
        }
    }

    if matches!(act, PositionAction::Claim) {
        block.push_str(&format!(
            "\n  claimable_fees_usd: {:.4}",
            snapshot.unclaimed_fees_usd.unwrap_or(0.0)
        ));
    }

    block
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
                let addrs: Vec<String> = result
                    .positions
                    .iter()
                    .map(|p| p.position.clone())
                    .collect();
                positions.sync_open_positions(addrs);
            }
        }
        Err(e) => {
            warn("cycle", &format!("Position sync skipped: {}", e));
        }
    }

    // ── 2. Fetch real PnL + active_bin for each position ────────
    let active = positions.get_active();
    let mut pos_snapshots: Vec<ManagementPositionSnapshot> = Vec::new();

    for p in &active {
        let mut pnl_pct: Option<f64> = None;
        let mut fee_tvl: Option<f64> = None;
        let mut unclaimed_fees_usd: Option<f64> = None;
        let mut active_bin = 0i32;

        // Fetch real PnL from Meteora API
        if let Ok(pnl_result) =
            crate::tools::dlmm::get_position_pnl(&p.pool_address, &p.id, wallet_address).await
        {
            pnl_pct = pnl_result.pnl_pct;
            fee_tvl = pnl_result.fee_per_tvl_24h;
            unclaimed_fees_usd = pnl_result.unclaimed_fee_usd;
            if let Some(ab) = pnl_result.active_bin {
                active_bin = ab;
            }
        }

        // Fallback: fetch active_bin separately if PnL didn't provide it
        if active_bin == 0 {
            if let Ok(bin_result) = crate::tools::dlmm::get_active_bin(&p.pool_address).await {
                active_bin = bin_result.bin_id;
            }
        }

        pos_snapshots.push(ManagementPositionSnapshot {
            id: p.id.clone(),
            pnl_pct,
            fee_tvl,
            unclaimed_fees_usd,
            instruction: p.instruction.clone(),
            upper_bin: p.upper_bin,
            lower_bin: p.lower_bin,
            active_bin,
            minutes_out_of_range: minutes_out_of_range(p),
        });
    }

    // Update trailing state for each position
    for snapshot in &pos_snapshots {
        if let Some(pnl) = snapshot.pnl_pct {
            if let Some(pos) = positions.positions.get_mut(&snapshot.id) {
                update_trailing_state(
                    pos,
                    pnl,
                    config.management.trailing_trigger_pct,
                    config.management.trailing_drop_pct,
                );
                resolve_pending_peak_with_pnl(pos, pnl);
                resolve_pending_trailing_drop(pos, pnl, config.management.trailing_drop_pct, 1.0);
            }
        }
    }

    // ── 3. Check trailing TP exits ─────────────────────────────
    let mut exit_map: HashMap<String, String> = HashMap::new();
    for snapshot in &pos_snapshots {
        if let Some(pos) = positions.positions.get(&snapshot.id) {
            if let (Some(ref reason), Some(ref until)) = (
                &pos.trailing.confirmed_trailing_exit_reason,
                &pos.trailing.confirmed_trailing_exit_until,
            ) {
                if chrono::Utc::now().to_rfc3339() < *until {
                    exit_map.insert(snapshot.id.clone(), reason.clone());
                }
            }
        }
    }

    // ── 4. Build action map using deterministic rules ──────────
    let mut action_map: HashMap<String, PositionAction> = HashMap::new();
    for snapshot in &pos_snapshots {
        // Trailing exit — highest priority
        if let Some(reason) = exit_map.get(&snapshot.id) {
            action_map.insert(
                snapshot.id.clone(),
                PositionAction::TrailingExit {
                    reason: reason.clone(),
                },
            );
            continue;
        }
        // Instruction
        if snapshot.instruction.is_some() {
            action_map.insert(snapshot.id.clone(), PositionAction::Instruction);
            continue;
        }
        // Deterministic close rules
        if let Some(fees) = snapshot.unclaimed_fees_usd {
            if fees >= config.management.min_claim_amount {
                action_map.insert(snapshot.id.clone(), PositionAction::Claim);
                continue;
            }
        }

        if let (Some(pnl), Some(fee_tvl)) = (snapshot.pnl_pct, snapshot.fee_tvl) {
            if let Some(pos) = positions.positions.get(&snapshot.id) {
                if let Some(rule) = get_deterministic_close_rule(
                    pos,
                    snapshot.active_bin,
                    pnl,
                    fee_tvl,
                    snapshot.minutes_out_of_range,
                    config,
                ) {
                    let (rule_num, reason) = match rule {
                        CloseRule::StopLoss => (1u8, "stop loss"),
                        CloseRule::TakeProfit => (2, "take profit"),
                        CloseRule::SafetyExit => (2, "safety exit (breakeven after drawdown)"),
                        CloseRule::PumpedAboveRange => (3, "pumped far above range"),
                        CloseRule::OutOfRange => (4, "OOR"),
                        CloseRule::LowYield => (5, "low yield"),
                        CloseRule::TrailingTp => (0, "trailing TP"),
                    };
                    action_map.insert(
                        snapshot.id.clone(),
                        PositionAction::Close {
                            rule: rule_num,
                            reason: reason.to_string(),
                        },
                    );
                    continue;
                }
            }
        }
        action_map.insert(snapshot.id.clone(), PositionAction::Stay);
    }

    // ── 5. Build report ────────────────────────────────────────
    let needs_action: Vec<_> = action_map
        .values()
        .filter(|a| !matches!(a, PositionAction::Stay))
        .collect();
    let action_summary = if needs_action.is_empty() {
        "no action".to_string()
    } else {
        needs_action
            .iter()
            .map(|a| a.as_str().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };

    let mgmt_report = format!(
        "Summary: 💼 {} positions | {}",
        active_count, action_summary
    );

    if needs_action.is_empty() {
        info("cycle", "Management: no actions needed");
        return Ok(mgmt_report);
    }

    info(
        "cycle",
        &format!(
            "Management: {} action(s) needed — invoking LLM",
            needs_action.len()
        ),
    );

    // Build LLM goal with action details
    let action_blocks: Vec<String> = pos_snapshots
        .iter()
        .filter(|snapshot| {
            !matches!(
                action_map.get(&snapshot.id),
                Some(PositionAction::Stay) | None
            )
        })
        .map(|snapshot| {
            let act = action_map
                .get(&snapshot.id)
                .unwrap_or(&PositionAction::Stay);
            format_management_action_block(snapshot, act)
        })
        .collect();

    let goal = format!(
        "MANAGEMENT ACTION REQUIRED — {} position(s)\n\n{}\n\nExecute the required actions.",
        needs_action.len(),
        action_blocks.join("\n\n")
    );

    let agent = AgentLoop::new();
    let result = agent
        .run(
            &goal,
            AgentRole::Manager,
            AgentRunContext {
                config,
                llm,
                positions,
                pool_memory,
                wallet_address,
            },
        )
        .await?;

    info(
        "cycle",
        &format!("Management result: {}", result.chars().take(300).collect::<String>()),
    );
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
    let active_ids: Vec<String> = positions.get_active().iter().map(|p| p.id.clone()).collect();
    if active_ids.is_empty() {
        return Ok(vec![]);
    }

    // Self-heal: mark any tracked-active position that's already gone on-chain as
    // closed. A position closed by a prior exit (or externally) otherwise stays
    // "active" in state until the next restart and shows stuck on the dashboard.
    if let Ok(existing) = crate::tools::meteora_native::existing_positions(config, &active_ids).await
    {
        for id in &active_ids {
            if !existing.contains(id) {
                positions.mark_orphaned(id);
            }
        }
    }

    let active = positions.get_active();
    if active.is_empty() {
        return Ok(vec![]);
    }

    // Fetch real PnL + active_bin for each position
    let mut pos_data: Vec<PnlPollSnapshot> = Vec::new();
    for p in &active {
        let mut pnl_pct: Option<f64> = None;
        let mut fee_tvl: Option<f64> = None;
        let mut active_bin = 0i32;
        let mut pnl_in_range: Option<bool> = None;

        // Fetch real PnL
        if let Ok(pnl_result) =
            crate::tools::dlmm::get_position_pnl(&p.pool_address, &p.id, wallet_address).await
        {
            pnl_pct = pnl_result.pnl_pct;
            fee_tvl = pnl_result.fee_per_tvl_24h;
            // Authoritative in-range flag from the API (isOutOfRange).
            pnl_in_range = pnl_result.in_range;
            // Also try to get active_bin from PnL response
            if let Some(ab) = pnl_result.active_bin {
                active_bin = ab;
            }
        }

        // Fetch real active bin
        if let Ok(bin_result) = crate::tools::dlmm::get_active_bin(&p.pool_address).await {
            active_bin = bin_result.bin_id;
        }

        // Prefer the API's isOutOfRange flag; fall back to a bin comparison only
        // when it's unavailable. (The stored bins are relative and active_bin is
        // unreliable here, so the manual comparison alone mis-reported in-range.)
        let in_range = pnl_in_range.unwrap_or(active_bin >= p.lower_bin && active_bin <= p.upper_bin);

        pos_data.push(PnlPollSnapshot {
            id: p.id.clone(),
            pnl_pct,
            in_range,
            minutes_out_of_range: minutes_out_of_range(p),
            fee_tvl,
            active_bin,
        });
    }

    let mut exits_needed: Vec<(String, String)> = vec![];

    for snapshot in &pos_data {
        // Update trailing state
        if let Some(pnl) = snapshot.pnl_pct {
            if let Some(pos) = positions.positions.get_mut(&snapshot.id) {
                update_trailing_state(
                    pos,
                    pnl,
                    config.management.trailing_trigger_pct,
                    config.management.trailing_drop_pct,
                );
                resolve_pending_peak_with_pnl(pos, pnl);
                resolve_pending_trailing_drop(pos, pnl, config.management.trailing_drop_pct, 1.0);

                // Check confirmed trailing exit
                if let (Some(ref reason), Some(ref until)) = (
                    &pos.trailing.confirmed_trailing_exit_reason,
                    &pos.trailing.confirmed_trailing_exit_until,
                ) {
                    if chrono::Utc::now().to_rfc3339() < *until {
                        exits_needed.push((snapshot.id.clone(), reason.clone()));
                        pos.trailing.confirmed_trailing_exit_reason = None;
                        pos.trailing.confirmed_trailing_exit_until = None;
                    }
                }
            }
        }

        // Check deterministic close rules
        if let Some(pnl) = snapshot.pnl_pct {
            let fee_tvl = snapshot.fee_tvl.unwrap_or(0.001);
            if let Some(pos) = positions.positions.get(&snapshot.id) {
                if let Some(rule) = get_deterministic_close_rule(
                    pos,
                    snapshot.active_bin,
                    pnl,
                    fee_tvl,
                    snapshot.minutes_out_of_range,
                    config,
                ) {
                    let reason = match rule {
                        CloseRule::StopLoss => "stop loss",
                        CloseRule::TakeProfit => "take profit",
                        CloseRule::SafetyExit => "safety exit (breakeven after drawdown)",
                        CloseRule::PumpedAboveRange => "pumped far above range",
                        CloseRule::OutOfRange => "OOR",
                        CloseRule::LowYield => "low yield",
                        CloseRule::TrailingTp => "trailing TP",
                    };
                    if !exits_needed.iter().any(|(addr, _)| addr == &snapshot.id) {
                        exits_needed.push((snapshot.id.clone(), reason.to_string()));
                    }
                }
            }
        }

        // Update OOR state
        if !snapshot.in_range {
            positions.mark_oor(&snapshot.id);
        } else {
            positions.mark_in_range(&snapshot.id);
        }
    }

    if !exits_needed.is_empty() {
        info(
            "pnl_poll",
            &format!("{} exit(s) detected", exits_needed.len()),
        );
    }

    Ok(exits_needed)
}

pub fn queue_pnl_exit_close_instructions(
    positions: &mut PositionState,
    exits: &[(String, String)],
) -> usize {
    let mut queued = 0;
    for (position_id, reason) in exits {
        let instruction = format!("CLOSE: {}", reason);
        if positions.set_instruction(position_id, Some(&instruction)) {
            queued += 1;
        }
    }
    queued
}

// ═══════════════════════════════════════════════════════════════════
//  SCREENING CYCLE
// ═══════════════════════════════════════════════════════════════════

fn build_screening_goal(
    config: &Config,
    deploy_amount: f64,
    active_count: usize,
    active_strategy: Option<&crate::strategy_library::StrategyEntry>,
) -> String {
    let slots = (config.risk.max_positions as usize).saturating_sub(active_count);
    let base = if slots > 1 {
        // Fill every open slot in a single cycle so capital isn't left idle
        // waiting for the next tick (important for fast fee-printing rotation).
        format!(
            "Screen Meteora DLMM pools and fill up to {} open position slots, deploying {:.4} SOL to EACH of the best {} DISTINCT candidates (different pools and different base tokens).          Active positions: {}. Max: {}.          BE FAST: call get_top_candidates ONCE, then immediately call deploy_position once per candidate with amount_y={:.4} until all {} slots are filled or no more candidates pass. The candidates already passed every screening filter — do NOT do extra per-token research or analysis.          Deploy ONLY candidates that pass ALL thresholds — never the same pool or token twice.",
            slots, deploy_amount, slots, active_count, config.risk.max_positions, deploy_amount, slots,
        )
    } else {
        format!(
            "Screen Meteora DLMM pools and deploy {:.4} SOL to the best candidate.          Active positions: {}. Max: {}.          BE FAST: call get_top_candidates ONCE, then immediately call deploy_position with amount_y={:.4}. The candidates already passed every screening filter — do NOT do extra per-token research or analysis.          Deploy ONLY if a candidate passes ALL thresholds.",
            deploy_amount, active_count, config.risk.max_positions, deploy_amount,
        )
    };

    if let Some(strategy) = active_strategy {
        format!("{}\n{}", base, strategy.prompt_summary())
    } else {
        base
    }
}

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
        let msg = format!(
            "At max positions ({}/{}). Skipping.",
            active_count, config.risk.max_positions
        );
        info("cycle", &msg);
        return Ok(msg);
    }

    let mut deploy_amount = compute_deploy_amount(config, wallet_sol);
    if deploy_amount <= 0.0 {
        if crate::tools::dlmm::is_dry_run(config) {
            // Dry-run does not require real balance: simulate with the configured
            // deploy size so the full screen → decide → simulated-deploy flow runs.
            // The deploy transaction itself stays blocked by the dry-run guard.
            deploy_amount = if config.management.deploy_amount_sol > 0.0 {
                config.management.deploy_amount_sol
            } else {
                0.5
            };
            info(
                "cycle",
                &format!(
                    "DRY_RUN: wallet balance {:.2} SOL is insufficient; simulating deploy amount {:.2} SOL",
                    wallet_sol, deploy_amount
                ),
            );
        } else {
            let msg = format!("Not enough SOL ({:.2}) to deploy.", wallet_sol);
            info("cycle", &msg);
            return Ok(msg);
        }
    }

    let active_strategy = crate::strategy_library::get_active_strategy()
        .ok()
        .flatten();
    let goal = build_screening_goal(
        config,
        deploy_amount,
        active_count,
        active_strategy.as_ref(),
    );

    let agent = AgentLoop::new();
    let result = agent
        .run(
            &goal,
            AgentRole::Screener,
            AgentRunContext {
                config,
                llm,
                positions,
                pool_memory,
                wallet_address,
            },
        )
        .await?;

    info(
        "cycle",
        &format!("Screening result: {}", result.chars().take(300).collect::<String>()),
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screening_goal_includes_active_strategy_context_when_available() {
        let config = Config::default();
        let active = crate::strategy_library::StrategyEntry {
            id: "panda_strat".to_string(),
            name: "Panda Strat".to_string(),
            author: "top-lper".to_string(),
            lp_strategy: "curve".to_string(),
            token_criteria: serde_json::json!({"notes": "volatile narrative pools"}),
            entry: serde_json::json!({"condition": "after pullback"}),
            range: serde_json::json!({"type": "wide"}),
            exit: serde_json::json!({"notes": "take partial profits"}),
            best_for: "volatile narrative pools".to_string(),
            raw: String::new(),
            added_at: String::new(),
            updated_at: String::new(),
        };

        let goal = build_screening_goal(&config, 0.25, 1, Some(&active));

        assert!(goal.contains("STRATEGY CONTEXT: Panda Strat"));
        assert!(goal.contains("entry: after pullback"));
        assert!(goal.contains("exit: take partial profits"));
        assert!(goal.contains("best for: volatile narrative pools"));
    }

    #[test]
    fn pnl_poll_exits_queue_guarded_close_instructions() {
        let mut positions = PositionState::default();
        positions.add(crate::state::positions::TrackedPosition {
            id: "Pos111".to_string(),
            pool_address: "Pool111".to_string(),
            base_mint: "Base111".to_string(),
            lower_bin: 10,
            upper_bin: 20,
            amount_sol: 0.25,
            ..Default::default()
        });

        let queued = queue_pnl_exit_close_instructions(
            &mut positions,
            &[("Pos111".to_string(), "stop loss <script>".to_string())],
        );

        assert_eq!(queued, 1);
        let pos = positions.positions.get("Pos111").expect("position exists");
        assert_eq!(pos.instruction.as_deref(), Some("CLOSE: stop loss"));
    }

    #[test]
    fn management_action_block_includes_instruction_text_for_close_queue() {
        let snapshot = ManagementPositionSnapshot {
            id: "Pos111".to_string(),
            pnl_pct: Some(-4.2),
            fee_tvl: Some(0.0001),
            unclaimed_fees_usd: Some(0.0),
            instruction: Some("CLOSE: stop loss".to_string()),
            upper_bin: 20,
            lower_bin: 10,
            active_bin: 8,
            minutes_out_of_range: 12,
        };

        let block = format_management_action_block(&snapshot, &PositionAction::Instruction);

        assert!(block.contains("action: INSTRUCTION"));
        assert!(block.contains("instruction: CLOSE: stop loss"));
    }
}
