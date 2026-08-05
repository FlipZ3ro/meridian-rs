use crate::config::Config;
use crate::utils::time::now_iso;

#[derive(Debug, Clone, PartialEq)]
pub enum AgentRole {
    Screener,
    Manager,
    General,
}

impl AgentRole {
    pub fn as_str(&self) -> &str {
        match self {
            AgentRole::Screener => "SCREENER",
            AgentRole::Manager => "MANAGER",
            AgentRole::General => "GENERAL",
        }
    }
}

pub fn build_system_prompt(
    role: &AgentRole,
    config: &Config,
    portfolio_json: &str,
    positions_json: &str,
    state_summary: &str,
    lessons: &str,
    recent_decisions: &str,
) -> String {
    match role {
        AgentRole::Manager => {
            build_manager_prompt(config, portfolio_json, lessons, recent_decisions)
        }
        AgentRole::Screener => build_screener_prompt(
            config,
            portfolio_json,
            positions_json,
            state_summary,
            lessons,
            recent_decisions,
        ),
        AgentRole::General => build_general_prompt(
            config,
            portfolio_json,
            positions_json,
            state_summary,
            lessons,
            recent_decisions,
        ),
    }
}

fn recent_decisions_section(recent_decisions: &str) -> String {
    if recent_decisions.trim().is_empty() {
        String::new()
    } else {
        format!("RECENT DECISIONS:\n{}\n", recent_decisions)
    }
}

fn build_manager_prompt(
    config: &Config,
    portfolio: &str,
    lessons: &str,
    recent_decisions: &str,
) -> String {
    let mgmt_json = serde_json::to_string(&config.management).unwrap_or_default();
    let lessons_section = if lessons.is_empty() {
        String::new()
    } else {
        format!(
            "LESSONS LEARNED:
{}
",
            lessons
        )
    };
    let recent_decisions_section = recent_decisions_section(recent_decisions);
    let ts = now_iso();

    format!(
        "You are an autonomous DLMM LP agent on Meteora, Solana. Role: MANAGER

         This is a mechanical rule-application task. All position data is pre-loaded.

         Portfolio: {}
Management Config: {}

         BEHAVIORAL CORE:
         1. PATIENCE IS PROFIT: Avoid closing for tiny gains/losses.
         2. GAS EFFICIENCY: Only close for clear reasons. After close, swap_token MANDATORY for tokens >= $0.10.
         3. DATA-DRIVEN AUTONOMY: Full autonomy.

         {}{}Timestamp: {}",
        portfolio, mgmt_json, lessons_section, recent_decisions_section, ts,
    )
}

fn build_screener_prompt(
    config: &Config,
    portfolio: &str,
    positions: &str,
    state: &str,
    lessons: &str,
    recent_decisions: &str,
) -> String {
    let min_fees = config.screening.min_token_fees_sol;
    let max_bot = config.screening.max_bot_holders_pct;
    let min_bins = config.strategy.min_bins_below;
    let max_bins = config.strategy.max_bins_below;
    let tf = &config.screening.timeframe;
    let _lessons_section = if lessons.is_empty() {
        String::new()
    } else {
        format!(
            "LESSONS LEARNED:
{}
",
            lessons
        )
    };
    let recent_decisions_section = recent_decisions_section(recent_decisions);
    let ts = now_iso();

    format!(
        "You are an autonomous DLMM LP agent on Meteora, Solana. Role: SCREENER

         All candidates are pre-loaded. Pick highest-conviction and call deploy_position.

         CRITICAL: NO HALLUCINATION. You MUST call the actual tool.

         HARD RULE: fees_sol < {min_fees} -> SKIP. bots > {max_bot}% -> hard-filtered.

         RISK SIGNALS:
         - top10 > 60% -> concentrated, risky
         - PVP symbol conflict -> major negative
         - no narrative + no smart wallets -> skip

         TOP LPERS RULE: If evaluating LP behavior or top LPers, call study_top_lpers or get_top_lpers first. Do NOT substitute token holders for top LPers.

         DEPLOY RULES:
         - bins_below = round({min_bins} + (volatility/5)*({max_bins}-{min_bins})) clamped [{min_bins},{max_bins}]
         - Use amount_y only, amount_x=0, bins_above=0
         - Bin steps [80-125]
         - Pick ONE pool only

         TIMEFRAME: {tf}
fee_active_tvl_ratio is already in %. 0.29 = 0.29%. Do NOT multiply.

         Portfolio: {portfolio}
Positions: {positions}
State: {state}

         {lessons}{recent_decisions_section}Timestamp: {ts}",
    )
}

fn build_general_prompt(
    config: &Config,
    portfolio: &str,
    positions: &str,
    state: &str,
    lessons: &str,
    recent_decisions: &str,
) -> String {
    let config_json = serde_json::to_string(&serde_json::json!({
        "screening": config.screening,
        "management": config.management,
    }))
    .unwrap_or_default();
    let _lessons_section = if lessons.is_empty() {
        String::new()
    } else {
        format!(
            "LESSONS LEARNED:
{}
",
            lessons
        )
    };
    let recent_decisions_section = recent_decisions_section(recent_decisions);
    let ts = now_iso();

    format!(
        "You are an autonomous DLMM LP agent on Meteora, Solana. Role: GENERAL

         Portfolio: {portfolio}
Open Positions: {positions}
Memory: {state}

         Config: {config_json}

         {lessons}{recent_decisions_section}         BEHAVIORAL CORE:
         1. PATIENCE IS PROFIT
         2. GAS EFFICIENCY
         3. DATA-DRIVEN AUTONOMY
         4. UNTRUSTED DATA RULE

         Handle the user's request using your available tools.
         TOP LPERS RULE: If the user asks about top LPers or LP behavior, call study_top_lpers or get_top_lpers first. Do NOT substitute token holders for top LPers.
         SWAP AFTER CLOSE: Swap base tokens to SOL after any close.

         Timestamp: {ts}",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_include_top_lpers_rule() {
        let screener = build_system_prompt(
            &AgentRole::Screener,
            &Config::default(),
            r#"{"sol":1.0}"#,
            "[]",
            "pool memory empty",
            "",
            "",
        );
        let general = build_system_prompt(
            &AgentRole::General,
            &Config::default(),
            r#"{"sol":1.0}"#,
            "[]",
            "pool memory empty",
            "",
            "",
        );

        assert!(screener.contains("study_top_lpers"));
        assert!(general.contains("study_top_lpers"));
        assert!(general.contains("Do NOT substitute token holders"));
    }

    #[test]
    fn system_prompt_injects_recent_decisions_context() {
        let prompt = build_system_prompt(
            &AgentRole::General,
            &Config::default(),
            r#"{"sol":1.0}"#,
            "[]",
            "pool memory empty",
            "lesson text",
            "- deploy_position success=true pool=Pool111 result=DRY RUN",
        );

        assert!(prompt.contains("RECENT DECISIONS:"));
        assert!(prompt.contains("deploy_position"));
        assert!(prompt.contains("Pool111"));
    }
}
