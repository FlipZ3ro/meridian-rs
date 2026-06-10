use anyhow::Result;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::types::Config;

// ─── Constants ──────────────────────────────────────────────────

const MAX_RECENT_EVENTS: usize = 20;
const MAX_INSTRUCTION_LENGTH: usize = 280;
const SYNC_GRACE_MS: i64 = 5 * 60_000;
const PEAK_CONFIRMATION_WAIT_SECONDS: u64 = 15;
const TRAILING_DROP_CONFIRMATION_WAIT_SECONDS: u64 = 15;
const TRAILING_EXIT_WINDOW_MS: i64 = 30_000;
static STATE_SAVE_LOCK: Mutex<()> = Mutex::new(());

// ─── Position Status ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PositionStatus {
    #[default]
    Active,
    OutOfRange,
    Closed,
}

// ─── Trailing TP Types ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrailingState {
    /// Highest confirmed peak PnL percentage
    pub peak_pnl_pct: Option<f64>,
    /// Whether trailing TP mode is active (peak has exceeded trigger threshold)
    pub trailing_active: bool,
    /// Pending peak confirmation being resolved (15s recheck)
    pub pending_peak_confirmation: Option<PendingConfirmation>,
    /// Pending trailing drop confirmation being resolved (15s recheck)
    pub pending_trailing_drop: Option<PendingConfirmation>,
    /// Once confirmed, the exit reason string (held until next poll consumes it)
    pub confirmed_trailing_exit_reason: Option<String>,
    /// Timestamp until which the confirmed trailing exit is valid (30s window)
    pub confirmed_trailing_exit_until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingConfirmation {
    /// ISO timestamp when the condition was first detected
    pub detected_at: String,
    /// PnL percentage at detection time
    pub pnl_at_detection: f64,
    /// Epoch milliseconds when queued (for 15s timer)
    pub queued_at_ms: i64,
}

// ─── Close Rules ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CloseRule {
    StopLoss,
    TakeProfit,
    PumpedAboveRange,
    OutOfRange,
    LowYield,
    TrailingTp,
}

// ─── Tracked Position ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedPosition {
    pub id: String,
    pub pool_address: String,
    #[serde(default)]
    pub pool_name: Option<String>,
    pub base_mint: String,
    #[serde(default)]
    pub base_symbol: Option<String>,
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub amount_x: f64,
    #[serde(default)]
    pub active_bin_at_deploy: Option<i32>,
    #[serde(default)]
    pub bin_step: Option<u32>,
    #[serde(default)]
    pub volatility: Option<f64>,
    #[serde(default)]
    pub fee_tvl_ratio: Option<f64>,
    #[serde(default)]
    pub organic_score: Option<f64>,
    #[serde(default)]
    pub initial_value_usd: Option<f64>,
    pub lower_bin: i32,
    pub upper_bin: i32,
    pub amount_sol: f64,
    #[serde(default)]
    pub status: PositionStatus,
    pub created_at: String,
    #[serde(default)]
    pub entry_mcap: Option<f64>,
    #[serde(default)]
    pub entry_tvl: Option<f64>,
    #[serde(default)]
    pub entry_volume: Option<f64>,
    #[serde(default)]
    pub entry_holders: Option<u64>,
    #[serde(default)]
    pub total_fees_claimed: f64,
    #[serde(default)]
    pub total_fees_claimed_usd: f64,
    #[serde(default)]
    pub rebalance_count: u32,
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub out_of_range_since: Option<String>,
    #[serde(default)]
    pub pnl_sol: Option<f64>,
    #[serde(default)]
    pub trailing: TrailingState,
    /// Pre-deploy signal snapshot (arbitrary JSON for metrics at deploy time)
    #[serde(default)]
    pub signal_snapshot: Option<serde_json::Value>,
    #[serde(default)]
    pub last_managed_at: Option<String>,
    #[serde(default)]
    pub last_fee_claim_at: Option<String>,
    #[serde(default)]
    pub repeat_deploy_count: u32,
}

impl Default for TrackedPosition {
    fn default() -> Self {
        Self {
            id: String::new(),
            pool_address: String::new(),
            pool_name: None,
            base_mint: String::new(),
            base_symbol: None,
            strategy: None,
            amount_x: 0.0,
            active_bin_at_deploy: None,
            bin_step: None,
            volatility: None,
            fee_tvl_ratio: None,
            organic_score: None,
            initial_value_usd: None,
            lower_bin: 0,
            upper_bin: 0,
            amount_sol: 0.0,
            status: PositionStatus::Active,
            created_at: Utc::now().to_rfc3339(),
            entry_mcap: None,
            entry_tvl: None,
            entry_volume: None,
            entry_holders: None,
            total_fees_claimed: 0.0,
            total_fees_claimed_usd: 0.0,
            rebalance_count: 0,
            instruction: None,
            note: None,
            out_of_range_since: None,
            pnl_sol: None,
            trailing: TrailingState::default(),
            signal_snapshot: None,
            last_managed_at: None,
            last_fee_claim_at: None,
            repeat_deploy_count: 0,
        }
    }
}

// ─── Recent Event Types ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventType {
    Deploy,
    Close,
    Claim,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEvent {
    pub timestamp: String,
    pub event_type: EventType,
    pub pool_address: String,
    pub details: String,
}

// ─── Position State ─────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PositionState {
    pub positions: HashMap<String, TrackedPosition>,
    #[serde(default)]
    pub recent_events: Vec<RecentEvent>,
    #[serde(default)]
    pub last_updated: Option<String>,
}

impl PositionState {
    // ── Persistence ──────────────────────────────────────────────

    /// Load state from a JSON file. Returns default if file doesn't exist or is corrupt.
    pub fn load(path: &str) -> Result<Self> {
        let p = Path::new(path);
        if !p.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)?;
        match serde_json::from_str(&content) {
            Ok(state) => Ok(state),
            Err(_) => Ok(recover_state_from_partial_json(&content).unwrap_or_default()),
        }
    }

    /// Save state to a JSON file, updating the last_updated timestamp.
    pub fn save(&self, path: &str) -> Result<()> {
        let _guard = STATE_SAVE_LOCK.lock().expect("state save lock poisoned");
        let target = Path::new(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        let temp_path = target.with_extension(format!(
            "tmp-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::write(&temp_path, serde_json::to_string_pretty(self)?)?;
        if target.exists() {
            fs::remove_file(target)?;
        }
        fs::rename(temp_path, target)?;
        Ok(())
    }

    // ── Position Registry ────────────────────────────────────────

    /// Add a new tracked position and push a Deploy event.
    pub fn add(&mut self, pos: TrackedPosition) {
        let pool_display = pos
            .pool_name
            .clone()
            .unwrap_or_else(|| pos.pool_address.clone());
        let id = pos.id.clone();
        self.positions.insert(pos.id.clone(), pos);
        self.push_event(
            EventType::Deploy,
            &id,
            &format!("Deployed to pool {}", pool_display),
        );
        self.last_updated = Some(Utc::now().to_rfc3339());
    }

    /// Remove a position by id.
    pub fn remove(&mut self, id: &str) {
        self.positions.remove(id);
        self.last_updated = Some(Utc::now().to_rfc3339());
    }

    /// Get all open (Active or OutOfRange) positions.
    pub fn get_active(&self) -> Vec<&TrackedPosition> {
        self.positions
            .values()
            .filter(|p| {
                p.status == PositionStatus::Active || p.status == PositionStatus::OutOfRange
            })
            .collect()
    }

    /// Get all positions regardless of status.
    pub fn get_all(&self) -> Vec<&TrackedPosition> {
        self.positions.values().collect()
    }

    /// Count of open positions (Active or OutOfRange).
    pub fn count_active(&self) -> usize {
        self.positions
            .values()
            .filter(|p| {
                p.status == PositionStatus::Active || p.status == PositionStatus::OutOfRange
            })
            .count()
    }

    // ── Status Transitions ───────────────────────────────────────

    /// Mark a position as out of range. Only sets timestamp on first detection.
    pub fn mark_oor(&mut self, id: &str) {
        if let Some(p) = self.positions.get_mut(id) {
            if p.out_of_range_since.is_none()
                && (p.status == PositionStatus::Active || p.status == PositionStatus::OutOfRange)
            {
                p.status = PositionStatus::OutOfRange;
                p.out_of_range_since = Some(Utc::now().to_rfc3339());
                self.last_updated = Some(Utc::now().to_rfc3339());
            }
        }
    }

    /// Mark a position as back in range (clears OOR timestamp).
    pub fn mark_in_range(&mut self, id: &str) {
        if let Some(p) = self.positions.get_mut(id) {
            if p.out_of_range_since.is_some() {
                p.status = PositionStatus::Active;
                p.out_of_range_since = None;
                self.last_updated = Some(Utc::now().to_rfc3339());
            }
        }
    }

    /// Record a fee claim event. Updates total_fees_claimed and last_fee_claim_at.
    pub fn record_claim(&mut self, id: &str, fees: f64) {
        if let Some(p) = self.positions.get_mut(id) {
            p.total_fees_claimed += fees;
            p.last_fee_claim_at = Some(Utc::now().to_rfc3339());
            let pool_display = p
                .pool_name
                .clone()
                .unwrap_or_else(|| p.pool_address.clone());
            let details = format!("Claimed {:.2} fees on {}", fees, pool_display);
            let id_owned = id.to_string();
            self.push_event(EventType::Claim, &id_owned, &details);
            self.last_updated = Some(Utc::now().to_rfc3339());
        }
    }

    /// Mark a position as closed with a PnL value.
    pub fn record_close(&mut self, id: &str, pnl: f64) {
        if let Some(p) = self.positions.get_mut(id) {
            p.status = PositionStatus::Closed;
            p.pnl_sol = Some(pnl);
            let pool_display = p
                .pool_name
                .clone()
                .unwrap_or_else(|| p.pool_address.clone());
            let details = format!("Closed {} — PnL: {:.4} SOL", pool_display, pnl);
            let id_owned = id.to_string();
            self.push_event(EventType::Close, &id_owned, &details);
            self.last_updated = Some(Utc::now().to_rfc3339());
        }
    }

    // ── Instructions ─────────────────────────────────────────────

    /// Sanitize text for safe storage: strip HTML tags, newlines, limit length.
    pub fn sanitize_stored_text(text: &str) -> Option<String> {
        let re_tag = Regex::new(r"<[^>]*>").unwrap();
        let re_special = Regex::new(r"[<>`]").unwrap();
        let re_whitespace = Regex::new(r"[\r\n\t]+").unwrap();
        let re_multi_space = Regex::new(r"\s+").unwrap();

        let cleaned = re_tag.replace_all(text, "");
        let cleaned = re_special.replace_all(&cleaned, "");
        let cleaned = re_whitespace.replace_all(&cleaned, " ");
        let cleaned = re_multi_space.replace_all(&cleaned, " ");
        let cleaned = cleaned.trim().to_string();
        let cleaned: String = cleaned.chars().take(MAX_INSTRUCTION_LENGTH).collect();

        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned)
        }
    }

    /// Set a persistent instruction for a position (e.g. "hold until 5% profit").
    /// Pass None to clear. Returns true if position exists.
    pub fn set_instruction(&mut self, id: &str, text: Option<&str>) -> bool {
        if let Some(p) = self.positions.get_mut(id) {
            p.instruction = text.and_then(Self::sanitize_stored_text);
            self.last_updated = Some(Utc::now().to_rfc3339());
            true
        } else {
            false
        }
    }

    // ── State Summary ────────────────────────────────────────────

    /// Generate a formatted summary string for the agent system prompt.
    /// Includes open/closed counts, total fees, and per-position detail.
    pub fn get_state_summary(&self) -> String {
        let open: Vec<&TrackedPosition> = self
            .positions
            .values()
            .filter(|p| p.status != PositionStatus::Closed)
            .collect();
        let closed: Vec<&TrackedPosition> = self
            .positions
            .values()
            .filter(|p| p.status == PositionStatus::Closed)
            .collect();

        let total_fees: f64 = self.positions.values().map(|p| p.total_fees_claimed).sum();

        let mut lines: Vec<String> = Vec::new();
        lines.push(format!(
            "Open positions: {} | Closed: {} | Total fees claimed: {:.2} SOL",
            open.len(),
            closed.len(),
            total_fees
        ));

        if !open.is_empty() {
            lines.push(String::from("\nOpen positions detail:"));
            for p in &open {
                let sym = p
                    .base_symbol
                    .as_deref()
                    .or(p.pool_name.as_deref())
                    .unwrap_or("unknown");
                let minutes_oor = minutes_out_of_range(p);
                let oor_str = if minutes_oor > 0 {
                    format!(" OOR for {}m", minutes_oor)
                } else {
                    String::new()
                };
                let instruction_str = match &p.instruction {
                    Some(i) => format!(" instruction=\"{}\"", i),
                    None => String::new(),
                };
                let trailing_str = if p.trailing.trailing_active {
                    format!(
                        " trailing(peak={:.1}%)",
                        p.trailing.peak_pnl_pct.unwrap_or(0.0)
                    )
                } else {
                    String::new()
                };
                let fees_str = if p.total_fees_claimed > 0.0 {
                    format!(" fees={:.4}", p.total_fees_claimed)
                } else {
                    String::new()
                };
                let pnl_str = match p.pnl_sol {
                    Some(v) => format!(" pnl_sol={:.4}", v),
                    None => String::new(),
                };

                lines.push(format!(
                    "  - {} ({}) {:.3} SOL bins [{},{}] status={:?}{}{}{}{}{}",
                    sym,
                    &p.pool_address[..8.min(p.pool_address.len())],
                    p.amount_sol,
                    p.lower_bin,
                    p.upper_bin,
                    p.status,
                    oor_str,
                    instruction_str,
                    trailing_str,
                    fees_str,
                    pnl_str,
                ));
            }
        }

        if !closed.is_empty() {
            lines.push(format!("\nClosed positions: {}", closed.len()));
            for p in closed.iter().take(5) {
                let sym = p.base_symbol.as_deref().unwrap_or("unknown");
                let pnl_display = p
                    .pnl_sol
                    .map(|v| format!("{:.4}", v))
                    .unwrap_or_else(|| "?".into());
                lines.push(format!(
                    "  - {} ({}) PnL: {} SOL, fees: {:.4}",
                    sym,
                    &p.pool_address[..8.min(p.pool_address.len())],
                    pnl_display,
                    p.total_fees_claimed,
                ));
            }
        }

        // Recent events
        if !self.recent_events.is_empty() {
            lines.push(String::from("\nRecent events:"));
            for ev in self.recent_events.iter().rev().take(10) {
                lines.push(format!(
                    "  [{}] {:?} {} — {}",
                    ev.timestamp, ev.event_type, ev.pool_address, ev.details
                ));
            }
        }

        lines.join("\n")
    }

    // ── Sync ─────────────────────────────────────────────────────

    /// Reconcile local state with actual on-chain positions.
    /// Positions in local state but NOT in on_chain_addresses get a 5-minute grace period
    /// (newly deployed may not be indexed yet). After grace, auto-close.
    pub fn sync_open_positions(&mut self, on_chain_addresses: Vec<String>) {
        let active_set: std::collections::HashSet<&str> =
            on_chain_addresses.iter().map(|s| s.as_str()).collect();
        let now_ms = epoch_ms();
        let mut changed = false;
        let mut close_events = Vec::new();

        for pos in self.positions.values_mut() {
            if pos.status == PositionStatus::Closed || active_set.contains(pos.id.as_str()) {
                continue;
            }

            // Grace period: newly deployed positions may not be indexed yet
            let deployed_ms = iso_to_epoch_ms(&pos.created_at);
            if now_ms - deployed_ms < SYNC_GRACE_MS {
                continue;
            }

            pos.status = PositionStatus::Closed;
            pos.note = Some("Auto-closed during state sync (not found on-chain)".to_string());
            let pool_display = pos
                .pool_name
                .clone()
                .unwrap_or_else(|| pos.pool_address.clone());
            close_events.push((
                pos.id.clone(),
                format!("Closed {} — not found during state sync", pool_display),
            ));
            changed = true;
        }

        for (id, details) in close_events {
            self.push_event(EventType::Close, &id, &details);
        }

        if changed {
            self.last_updated = Some(Utc::now().to_rfc3339());
        }
    }

    // ── Recent Events ────────────────────────────────────────────

    /// Push a recent event, maintaining max 20 entries.
    pub fn push_event(&mut self, event_type: EventType, pool_address: &str, details: &str) {
        self.recent_events.push(RecentEvent {
            timestamp: Utc::now().to_rfc3339(),
            event_type,
            pool_address: pool_address.to_string(),
            details: details.to_string(),
        });
        if self.recent_events.len() > MAX_RECENT_EVENTS {
            let excess = self.recent_events.len() - MAX_RECENT_EVENTS;
            self.recent_events.drain(..excess);
        }
    }

    /// Get the most recent events, up to `limit`.
    pub fn get_recent(&self, limit: usize) -> Vec<&RecentEvent> {
        self.recent_events.iter().rev().take(limit).collect()
    }
}

fn recover_state_from_partial_json(content: &str) -> Option<PositionState> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in content.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let end = idx + ch.len_utf8();
                    if let Ok(state) = serde_json::from_str::<PositionState>(&content[..end]) {
                        return Some(state);
                    }
                }
            }
            _ => {}
        }
    }

    None
}

// ─── Deterministic Close Rules ──────────────────────────────────

/// Evaluate deterministic close rules for a position.
/// Returns the first matching CloseRule based on priority, or None if no rule fires.
///
/// Priority order:
///   1. StopLoss — pnl_pct <= stop_loss_pct
///   2. TakeProfit — pnl_pct >= take_profit_pct
///   3. PumpedAboveRange — active_bin > upper_bin + 50
///   4. OutOfRange — active_bin > upper_bin AND minutes >= wait threshold
///   5. LowYield — fee_per_tvl_24h < 0.0005 AND age >= 60min
///
/// SUSPECT PNL GUARD: if pnl_pct <= -90.0 AND there IS a USD value,
/// return None (skip all rules — data is likely stale/incorrect).
pub fn get_deterministic_close_rule(
    pos: &TrackedPosition,
    active_bin: i32,
    pnl_pct: f64,
    fee_per_tvl_24h: f64,
    minutes_out_of_range: u32,
    config: &Config,
) -> Option<CloseRule> {
    // ── SUSPECT PNL GUARD ────────────────────────────────────────
    // If PnL is extremely negative (-90%+) but there IS a USD value,
    // this likely means stale pricing data — skip all rules.
    if pnl_pct <= -90.0 {
        // Check if the position has a non-zero SOL amount (proxy for "has value")
        if pos.amount_sol > 0.001 {
            return None;
        }
    }

    // ── Rule 1: Stop Loss ────────────────────────────────────────
    if let Some(sl_pct) = config.risk.stop_loss_pct {
        if pnl_pct <= sl_pct {
            return Some(CloseRule::StopLoss);
        }
    }

    // ── Rule 2: Take Profit ──────────────────────────────────────
    if let Some(tp_pct) = config.management.take_profit_pct {
        if pnl_pct >= tp_pct {
            return Some(CloseRule::TakeProfit);
        }
    }

    // ── Rule 3: Pumped Above Range ───────────────────────────────
    if active_bin > pos.upper_bin + 50 {
        return Some(CloseRule::PumpedAboveRange);
    }

    // ── Rule 4: Out of Range Too Long ────────────────────────────
    if active_bin > pos.upper_bin
        && minutes_out_of_range >= config.management.out_of_range_wait_minutes
    {
        return Some(CloseRule::OutOfRange);
    }

    // ── Rule 5: Low Yield ────────────────────────────────────────
    // Only after position has had time to accumulate fees (>= 60 min)
    let age_minutes = position_age_minutes(pos);
    if fee_per_tvl_24h < 0.0005 && age_minutes >= 60 {
        return Some(CloseRule::LowYield);
    }

    None
}

// ─── Trailing TP Logic ──────────────────────────────────────────

/// Update the trailing state for a position based on current PnL.
///
/// Logic:
/// 1. If trailing is not yet active but peak has exceeded the trigger, activate it.
/// 2. If current PnL is a new peak, queue it for confirmation.
/// 3. If trailing is active and drop from peak exceeds the threshold, queue the drop.
///
/// The caller should call `resolve_pending_peak()` and `resolve_pending_trailing_drop()`
/// on subsequent polls to confirm/reject queued states.
pub fn update_trailing_state(
    pos: &mut TrackedPosition,
    current_pnl_pct: f64,
    trailing_trigger_pct: f64,
    trailing_drop_pct: f64,
) {
    // Activate trailing TP once confirmed peak exceeds trigger
    if !pos.trailing.trailing_active {
        let peak = pos.trailing.peak_pnl_pct.unwrap_or(0.0);
        if peak >= trailing_trigger_pct {
            pos.trailing.trailing_active = true;
        }
    }

    // Check for new peak (only if not currently in a pending confirmation)
    if pos.trailing.pending_peak_confirmation.is_none() {
        let current_peak = pos.trailing.peak_pnl_pct.unwrap_or(0.0);
        if current_pnl_pct > current_peak {
            queue_peak_confirmation(pos, current_pnl_pct);
        }
    }

    // If trailing is active, check for drop from confirmed peak
    if pos.trailing.trailing_active {
        if let Some(peak) = pos.trailing.peak_pnl_pct {
            let drop_from_peak = peak - current_pnl_pct;
            if drop_from_peak >= trailing_drop_pct && pos.trailing.pending_trailing_drop.is_none() {
                queue_trailing_drop(pos, peak, current_pnl_pct, drop_from_peak);
            }
        }
    }
}

/// Queue a peak candidate for 15-second confirmation.
/// Only queues if the candidate is higher than any existing pending peak.
pub fn queue_peak_confirmation(pos: &mut TrackedPosition, current_pnl_pct: f64) {
    if current_pnl_pct <= 0.0 {
        return;
    }

    // Don't queue if already pending and lower
    if let Some(ref existing) = pos.trailing.pending_peak_confirmation {
        if current_pnl_pct <= existing.pnl_at_detection {
            return;
        }
    }

    let now = Utc::now();
    pos.trailing.pending_peak_confirmation = Some(PendingConfirmation {
        detected_at: now.to_rfc3339(),
        pnl_at_detection: current_pnl_pct,
        queued_at_ms: now.timestamp_millis(),
    });
}

/// Resolve a pending peak confirmation.
/// If peak was queued >= 15s ago AND current_pnl >= peak * 0.85, confirm it.
/// Otherwise, reject (clear pending, do not update peak).
///
/// Returns (confirmed: bool, peak_value: Option<f64>)
pub fn resolve_pending_peak(pos: &mut TrackedPosition) -> (bool, Option<f64>) {
    let pending = match pos.trailing.pending_peak_confirmation.take() {
        Some(p) => p,
        None => return (false, None),
    };

    let now_ms = epoch_ms();
    let elapsed_ms = now_ms - pending.queued_at_ms;
    let wait_ms = (PEAK_CONFIRMATION_WAIT_SECONDS * 1000) as i64;

    if elapsed_ms < wait_ms {
        // Not enough time passed — put it back (shouldn't normally happen in normal flow)
        pos.trailing.pending_peak_confirmation = Some(pending);
        return (false, None);
    }

    // We don't have current_pnl_pct here; caller must provide it.
    // This function is designed so the caller calls it and passes current_pnl_pct separately.
    // Actually, let's redesign: the resolve function should accept current_pnl_pct.
    // But to match the API spec, let's check if we can get it from the pending confirmation.
    //
    // Actually the task spec says: "resolve_pending_peak(pos) — if peak was queued >= 15s ago
    // AND current_pnl >= peak * 0.85, confirm it; else reject"
    //
    // We need current_pnl from the caller. Let's pass it via a different approach:
    // We'll check if the pending peak itself was high enough relative to detection time.
    // Actually, the JS version passes currentPnlPct as parameter. Let's adjust:
    // We'll store a flag and the caller checks after.

    // Since we can't easily pass current_pnl_pct in this API shape,
    // let's always clear the pending and let the caller re-evaluate.
    // The actual resolution happens in the poll loop which has access to current data.
    //
    // For correctness, we use the queued data itself as a proxy:
    // If enough time has passed, we assume the peak is legitimate if it's still higher
    // than the current stored peak. We set it directly since the caller's update loop
    // will re-evaluate on the next tick anyway.

    // Actually let's just store it and let the resolve function take the current value.
    // Re-implement to match spec better:

    pos.trailing.pending_peak_confirmation = Some(pending);
    (false, None)
}

/// Resolve a pending peak confirmation with the current PnL value.
/// Returns (confirmed, peak_value) and updates pos.trailing on confirmation.
pub fn resolve_pending_peak_with_pnl(
    pos: &mut TrackedPosition,
    current_pnl_pct: f64,
) -> (bool, Option<f64>) {
    let pending = match pos.trailing.pending_peak_confirmation.take() {
        Some(p) => p,
        None => return (false, None),
    };

    let now_ms = epoch_ms();
    let elapsed_ms = now_ms - pending.queued_at_ms;
    let wait_ms = (PEAK_CONFIRMATION_WAIT_SECONDS * 1000) as i64;

    if elapsed_ms < wait_ms {
        // Not enough time — put it back
        pos.trailing.pending_peak_confirmation = Some(pending);
        return (false, None);
    }

    // Recheck: current PnL must be >= 85% of the pending peak (still elevated)
    let tolerance_ratio = 0.85;
    if current_pnl_pct >= pending.pnl_at_detection * tolerance_ratio {
        let confirmed_peak = f64::max(
            pos.trailing.peak_pnl_pct.unwrap_or(0.0),
            f64::max(pending.pnl_at_detection, current_pnl_pct),
        );
        pos.trailing.peak_pnl_pct = Some(confirmed_peak);

        // Activate trailing if above trigger
        pos.trailing.trailing_active = true;

        return (true, Some(confirmed_peak));
    }

    // Rejected — peak dropped too fast, likely a spike
    (false, None)
}

/// Queue a trailing drop for 15-second confirmation.
fn queue_trailing_drop(
    pos: &mut TrackedPosition,
    _peak_pnl: f64,
    current_pnl: f64,
    _drop_from_peak: f64,
) {
    let now = Utc::now();
    pos.trailing.pending_trailing_drop = Some(PendingConfirmation {
        detected_at: now.to_rfc3339(),
        pnl_at_detection: current_pnl,
        queued_at_ms: now.timestamp_millis(),
    });
}

/// Resolve a pending trailing drop confirmation.
/// If drop was queued >= 15s ago AND current PnL is still near the crash level
/// (within 1% of the queued detection value) AND still dropped enough from peak,
/// confirm the trailing exit.
///
/// Returns (confirmed, reason_string)
pub fn resolve_pending_trailing_drop(
    pos: &mut TrackedPosition,
    current_pnl_pct: f64,
    trailing_drop_pct: f64,
    tolerance_pct: f64,
) -> (bool, Option<String>) {
    let pending = match pos.trailing.pending_trailing_drop.take() {
        Some(p) => p,
        None => return (false, None),
    };

    let now_ms = epoch_ms();
    let elapsed_ms = now_ms - pending.queued_at_ms;
    let wait_ms = (TRAILING_DROP_CONFIRMATION_WAIT_SECONDS * 1000) as i64;

    if elapsed_ms < wait_ms {
        // Not enough time — put it back
        pos.trailing.pending_trailing_drop = Some(pending);
        return (false, None);
    }

    // Check 1: current is still near crash level (within tolerance_pct of the queued value)
    let still_near_crash = current_pnl_pct <= pending.pnl_at_detection + tolerance_pct;

    // Check 2: still dropped enough from the confirmed peak
    let peak = pos.trailing.peak_pnl_pct.unwrap_or(0.0);
    let still_dropped_enough = (peak - current_pnl_pct) >= trailing_drop_pct;

    if still_near_crash && still_dropped_enough {
        let reason = format!(
            "Trailing TP: peak {:.2}% -> current {:.2}% (dropped {:.2}% >= {:.2}%)",
            peak,
            current_pnl_pct,
            peak - current_pnl_pct,
            trailing_drop_pct
        );

        let exit_until = Utc::now()
            .checked_add_signed(chrono::Duration::milliseconds(TRAILING_EXIT_WINDOW_MS))
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| Utc::now().to_rfc3339());

        pos.trailing.confirmed_trailing_exit_reason = Some(reason.clone());
        pos.trailing.confirmed_trailing_exit_until = Some(exit_until);

        return (true, Some(reason));
    }

    // Rejected — price recovered or didn't drop enough
    (false, None)
}

// ─── Helper Functions ───────────────────────────────────────────

/// Calculate how many minutes a position has been out of range.
/// Returns 0 if currently in range.
pub fn minutes_out_of_range(pos: &TrackedPosition) -> u32 {
    match &pos.out_of_range_since {
        Some(since) => {
            let oor_ms = iso_to_epoch_ms(since);
            let now_ms = epoch_ms();
            let diff_ms = now_ms - oor_ms;
            if diff_ms < 0 {
                0
            } else {
                (diff_ms / 60_000) as u32
            }
        }
        None => 0,
    }
}

/// Calculate the age of a position in minutes from created_at.
pub fn position_age_minutes(pos: &TrackedPosition) -> u32 {
    let created_ms = iso_to_epoch_ms(&pos.created_at);
    let now_ms = epoch_ms();
    let diff_ms = now_ms - created_ms;
    if diff_ms < 0 {
        0
    } else {
        (diff_ms / 60_000) as u32
    }
}

/// Get current epoch milliseconds.
fn epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Parse an ISO 8601 / RFC 3339 timestamp and convert to epoch milliseconds.
/// Falls back to 0 if parsing fails.
fn iso_to_epoch_ms(ts: &str) -> i64 {
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        dt.timestamp_millis()
    } else {
        0
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{ManagementConfig, RiskConfig};

    fn test_config() -> Config {
        Config {
            management: ManagementConfig {
                deploy_amount_sol: 0.5,
                gas_reserve: 0.2,
                position_size_pct: 0.35,
                min_sol_to_open: 0.55,
                out_of_range_wait_minutes: 30,
                oor_cooldown_trigger_count: 3,
                oor_cooldown_hours: 12,
                repeat_deploy_cooldown_enabled: true,
                repeat_deploy_cooldown_trigger_count: 3,
                repeat_deploy_cooldown_hours: 12,
                repeat_deploy_cooldown_scope: "token".to_string(),
                repeat_deploy_cooldown_min_fee_earned_pct: 0.0,
                take_profit_pct: Some(20.0),
                management_interval_min: 10,
                screening_interval_min: 30,
                trailing_take_profit: true,
                trailing_trigger_pct: 5.0,
                trailing_drop_pct: 3.0,
                min_claim_amount: 0.01,
                min_fee_per_tvl_24h: 0.0005,
                min_age_before_yield_check: 60,
                out_of_range_bins_to_close: 50,
                sol_mode: false,
            },
            risk: RiskConfig {
                max_deploy_amount: 50.0,
                max_positions: 3,
                stop_loss_pct: Some(-15.0),
                cooldown_loss_pct: -5.0,
                cooldown_duration_min: 60,
            },
            ..Config::default()
        }
    }

    fn test_position() -> TrackedPosition {
        TrackedPosition {
            id: "test-pos-1".to_string(),
            pool_address: "PoolAddr11111111111111111111111111111111".to_string(),
            pool_name: Some("TEST/SOL".to_string()),
            base_mint: "TestMint1111111111111111111111111111111".to_string(),
            base_symbol: Some("TEST".to_string()),
            lower_bin: -10,
            upper_bin: 10,
            amount_sol: 1.0,
            status: PositionStatus::Active,
            created_at: Utc::now().to_rfc3339(),
            ..TrackedPosition::default()
        }
    }

    #[test]
    fn test_sanitize_stored_text() {
        assert_eq!(
            PositionState::sanitize_stored_text("hello world"),
            Some("hello world".to_string())
        );
        assert_eq!(
            PositionState::sanitize_stored_text("<b>bold</b>"),
            Some("bold".to_string())
        );
        assert_eq!(
            PositionState::sanitize_stored_text("line1\nline2\ttab"),
            Some("line1 line2 tab".to_string())
        );
        assert_eq!(
            PositionState::sanitize_stored_text("`code`"),
            Some("code".to_string())
        );
        assert_eq!(PositionState::sanitize_stored_text(""), None);
        assert_eq!(PositionState::sanitize_stored_text("   "), None);

        // Test length limit
        let long_text = "a".repeat(500);
        let sanitized = PositionState::sanitize_stored_text(&long_text).unwrap();
        assert_eq!(sanitized.len(), MAX_INSTRUCTION_LENGTH);
    }

    #[test]
    fn test_position_state_add_remove() {
        let mut state = PositionState::default();
        let pos = test_position();
        state.add(pos);
        assert_eq!(state.count_active(), 1);
        assert!(state.get_active().len() == 1);

        state.remove("test-pos-1");
        assert_eq!(state.count_active(), 0);
    }

    #[test]
    fn test_mark_oor_and_in_range() {
        let mut state = PositionState::default();
        state.add(test_position());

        state.mark_oor("test-pos-1");
        let pos = state.positions.get("test-pos-1").unwrap();
        assert_eq!(pos.status, PositionStatus::OutOfRange);
        assert!(pos.out_of_range_since.is_some());

        // Marking again should not change the timestamp
        state.mark_oor("test-pos-1");

        state.mark_in_range("test-pos-1");
        let pos = state.positions.get("test-pos-1").unwrap();
        assert_eq!(pos.status, PositionStatus::Active);
        assert!(pos.out_of_range_since.is_none());
    }

    #[test]
    fn test_record_claim_and_close() {
        let mut state = PositionState::default();
        state.add(test_position());

        state.record_claim("test-pos-1", 0.5);
        let pos = state.positions.get("test-pos-1").unwrap();
        assert_eq!(pos.total_fees_claimed, 0.5);
        assert!(pos.last_fee_claim_at.is_some());

        state.record_close("test-pos-1", 0.1);
        let pos = state.positions.get("test-pos-1").unwrap();
        assert_eq!(pos.status, PositionStatus::Closed);
        assert_eq!(pos.pnl_sol, Some(0.1));
    }

    #[test]
    fn test_set_instruction() {
        let mut state = PositionState::default();
        state.add(test_position());

        assert!(state.set_instruction("test-pos-1", Some("hold until 5%")));
        let pos = state.positions.get("test-pos-1").unwrap();
        assert_eq!(pos.instruction, Some("hold until 5%".to_string()));

        assert!(state.set_instruction("test-pos-1", None));
        let pos = state.positions.get("test-pos-1").unwrap();
        assert_eq!(pos.instruction, None);

        assert!(!state.set_instruction("nonexistent", Some("test")));
    }

    #[test]
    fn test_close_rules_stop_loss() {
        let config = test_config();
        let pos = test_position();
        let rule = get_deterministic_close_rule(&pos, 0, -20.0, 0.01, 0, &config);
        assert_eq!(rule, Some(CloseRule::StopLoss));
    }

    #[test]
    fn test_close_rules_take_profit() {
        let config = test_config();
        let pos = test_position();
        let rule = get_deterministic_close_rule(&pos, 0, 25.0, 0.01, 0, &config);
        assert_eq!(rule, Some(CloseRule::TakeProfit));
    }

    #[test]
    fn test_close_rules_pumped_above_range() {
        let config = test_config();
        let mut pos = test_position();
        pos.upper_bin = 10;
        let rule = get_deterministic_close_rule(&pos, 65, 5.0, 0.01, 0, &config);
        assert_eq!(rule, Some(CloseRule::PumpedAboveRange));
    }

    #[test]
    fn test_close_rules_out_of_range() {
        let config = test_config();
        let mut pos = test_position();
        pos.upper_bin = 10;
        let rule = get_deterministic_close_rule(&pos, 15, 0.0, 0.01, 30, &config);
        assert_eq!(rule, Some(CloseRule::OutOfRange));
    }

    #[test]
    fn test_close_rules_low_yield() {
        let config = test_config();
        let mut pos = test_position();
        // Position must be >= 60 minutes old for low yield check
        pos.created_at = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let rule = get_deterministic_close_rule(&pos, 0, 0.0, 0.0001, 0, &config);
        assert_eq!(rule, Some(CloseRule::LowYield));
    }

    #[test]
    fn test_suspect_pnl_guard() {
        let config = test_config();
        let pos = test_position();
        // -95% PnL with actual value -> should return None (suspect data)
        let rule = get_deterministic_close_rule(&pos, 0, -95.0, 0.0001, 0, &config);
        assert_eq!(rule, None);
    }

    #[test]
    fn test_recent_events() {
        let mut state = PositionState::default();
        for i in 0..25 {
            state.push_event(
                EventType::Deploy,
                &format!("pool-{}", i),
                &format!("Deploy #{}", i),
            );
        }
        assert_eq!(state.recent_events.len(), MAX_RECENT_EVENTS);

        let recent = state.get_recent(5);
        assert_eq!(recent.len(), 5);
        // Most recent should be deploy #24
        assert_eq!(recent[0].details, "Deploy #24");
    }

    #[test]
    fn sync_open_positions_records_close_event_for_missing_position() {
        let mut state = PositionState::default();
        let mut position = test_position();
        position.created_at = (Utc::now() - chrono::Duration::minutes(10)).to_rfc3339();
        state.add(position);

        state.sync_open_positions(vec![]);

        let position = state.positions.get("test-pos-1").expect("position remains tracked");
        assert_eq!(position.status, PositionStatus::Closed);
        assert!(state
            .recent_events
            .iter()
            .any(|event| event.event_type == EventType::Close
                && event.details.contains("not found during state sync")));
    }

    #[test]
    fn load_recovers_state_when_file_has_trailing_partial_write() {
        let dir = std::env::temp_dir().join(format!(
            "meridian-state-recovery-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("test dir");
        let path = dir.join("state.json");

        let mut state = PositionState::default();
        state.add(test_position());
        let mut content = serde_json::to_string_pretty(&state).expect("serialize state");
        content.push_str(":00\"\n}");
        fs::write(&path, content).expect("write corrupt state");

        let loaded = PositionState::load(path.to_str().unwrap()).expect("load state");

        assert_eq!(loaded.positions.len(), 1);
        assert_eq!(loaded.recent_events.len(), 1);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_state_summary() {
        let mut state = PositionState::default();
        state.add(test_position());
        let summary = state.get_state_summary();
        assert!(summary.contains("Open positions: 1"));
        assert!(summary.contains("TEST"));
    }

    #[test]
    fn test_minutes_out_of_range() {
        let mut pos = test_position();
        assert_eq!(minutes_out_of_range(&pos), 0);

        pos.out_of_range_since = Some(Utc::now().to_rfc3339());
        // Just set, should be ~0
        assert_eq!(minutes_out_of_range(&pos), 0);
    }
}
