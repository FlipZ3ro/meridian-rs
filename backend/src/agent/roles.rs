use crate::agent::prompt::AgentRole;
use regex::Regex;
use std::collections::HashSet;

/// Get the set of tool names allowed for a role
pub fn get_tools_for_role(role: &AgentRole, goal: &str) -> Vec<String> {
    match role {
        AgentRole::Manager => MANAGER_TOOLS.iter().map(|s| s.to_string()).collect(),
        AgentRole::Screener => SCREENER_TOOLS.iter().map(|s| s.to_string()).collect(),
        AgentRole::General => get_general_tools(goal),
    }
}

static MANAGER_TOOLS: &[&str] = &[
    "get_recent_decisions",
    "get_performance_history",
    "close_position",
    "claim_fees",
    "swap_token",
    "get_position_pnl",
    "get_my_positions",
    "get_wallet_balance",
];

static SCREENER_TOOLS: &[&str] = &[
    "get_recent_decisions",
    "get_performance_history",
    "deploy_position",
    "get_active_bin",
    "get_top_candidates",
    "check_smart_wallets_on_pool",
    "study_top_lpers",
    "get_top_lpers",
    "get_token_holders",
    "get_token_narrative",
    "get_token_info",
    "search_pools",
    "get_pool_memory",
    "get_wallet_balance",
    "get_my_positions",
];

static STRATEGY_TOOLS: &[&str] = &[
    "list_strategies",
    "get_strategy",
    "add_strategy",
    "set_active_strategy",
    "remove_strategy",
];

struct IntentPattern {
    intent: &'static str,
    tools: &'static [&'static str],
    pattern: &'static str,
}

static INTENT_MAP: &[IntentPattern] = &[
    IntentPattern {
        intent: "deploy",
        tools: &[
            "deploy_position",
            "get_top_candidates",
            "get_active_bin",
            "get_pool_memory",
            "check_smart_wallets_on_pool",
            "get_token_holders",
            "get_token_narrative",
            "get_token_info",
            "search_pools",
            "get_wallet_balance",
            "get_my_positions",
            "add_pool_note",
        ],
        pattern: r"(?i)\b(deploy|open|add liquidity|lp into|invest in)\b",
    },
    IntentPattern {
        intent: "close",
        tools: &[
            "close_position",
            "get_my_positions",
            "get_position_pnl",
            "get_wallet_balance",
            "swap_token",
        ],
        pattern: r"(?i)\b(close|exit|withdraw|remove liquidity|shut down)\b",
    },
    IntentPattern {
        intent: "claim",
        tools: &[
            "claim_fees",
            "get_my_positions",
            "get_position_pnl",
            "get_wallet_balance",
        ],
        pattern: r"(?i)\b(claim|harvest|collect)\b.*\bfee",
    },
    IntentPattern {
        intent: "swap",
        tools: &["swap_token", "get_wallet_balance"],
        pattern: r"(?i)\b(swap|convert|sell|exchange)\b",
    },
    IntentPattern {
        intent: "balance",
        tools: &["get_wallet_balance", "get_my_positions"],
        pattern: r"(?i)\b(balance|wallet|sol|how much)\b",
    },
    IntentPattern {
        intent: "positions",
        tools: &["get_my_positions", "get_position_pnl", "get_wallet_balance"],
        pattern: r"(?i)\b(position|portfolio|open|pnl|yield|range)\b",
    },
    IntentPattern {
        intent: "strategy",
        tools: STRATEGY_TOOLS,
        pattern: r"(?i)\b(strategy|strategies|active strategy|lp strat)\b",
    },
    IntentPattern {
        intent: "study",
        tools: &[
            "study_top_lpers",
            "get_top_lpers",
            "get_top_candidates",
            "get_token_info",
            "search_pools",
            "discover_pools",
        ],
        pattern: r"(?i)\b(study top|top lpers?|best lpers?|who.?s lping|lp behavior|lpers?)\b",
    },
    IntentPattern {
        intent: "screen",
        tools: &[
            "get_top_candidates",
            "study_top_lpers",
            "get_top_lpers",
            "get_token_holders",
            "get_token_narrative",
            "get_token_info",
            "search_pools",
            "check_smart_wallets_on_pool",
            "get_my_positions",
            "discover_pools",
        ],
        pattern: r"(?i)\b(screen|candidate|find pool|search|research|token)\b",
    },
    IntentPattern {
        intent: "memory",
        tools: &["get_pool_memory", "add_pool_note"],
        pattern: r"(?i)\b(memory|pool history|note|remember)\b",
    },
];

fn get_general_tools(goal: &str) -> Vec<String> {
    let mut matched = HashSet::new();

    for ip in INTENT_MAP {
        if let Ok(re) = Regex::new(ip.pattern) {
            if re.is_match(goal) {
                for t in ip.tools {
                    matched.insert(t.to_string());
                }
            }
        }
    }

    if matched.is_empty() {
        // Fallback: all non-destructive tools
        return vec![
            "get_recent_decisions",
            "get_performance_history",
            "get_wallet_balance",
            "get_my_positions",
            "get_position_pnl",
            "get_top_candidates",
            "search_pools",
            "get_token_info",
            "get_token_holders",
            "get_token_narrative",
            "get_pool_memory",
            "check_smart_wallets_on_pool",
            "study_top_lpers",
            "get_top_lpers",
            "get_active_bin",
            "discover_pools",
        ]
        .into_iter()
        .map(String::from)
        .collect();
    }

    matched.insert("get_recent_decisions".to_string());
    matched.insert("get_performance_history".to_string());
    matched.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_decisions_tool_is_available_to_all_agent_roles() {
        for role in [AgentRole::Manager, AgentRole::Screener, AgentRole::General] {
            let tools = get_tools_for_role(&role, "general status check");
            assert!(
                tools.iter().any(|tool| tool == "get_recent_decisions"),
                "role {:?} should include get_recent_decisions; tools={:?}",
                role,
                tools
            );
        }
    }

    #[test]
    fn performance_history_tool_is_available_to_all_agent_roles() {
        for role in [AgentRole::Manager, AgentRole::Screener, AgentRole::General] {
            let tools = get_tools_for_role(&role, "show recent performance history");
            assert!(
                tools.iter().any(|tool| tool == "get_performance_history"),
                "role {:?} should include get_performance_history; tools={:?}",
                role,
                tools
            );
        }
    }

    #[test]
    fn top_lper_tools_are_available_for_screener_and_general_study_intent() {
        let screener_tools = get_tools_for_role(&AgentRole::Screener, "screen pools");
        let general_tools = get_tools_for_role(
            &AgentRole::General,
            "study top LPers and LP behavior for this pool",
        );

        for tools in [&screener_tools, &general_tools] {
            for name in ["study_top_lpers", "get_top_lpers"] {
                assert!(
                    tools.iter().any(|tool| tool == name),
                    "top-LPer intent should include {name}; tools={tools:?}"
                );
            }
        }
    }

    #[test]
    fn strategy_tools_are_available_for_general_strategy_intent() {
        let tools = get_tools_for_role(
            &AgentRole::General,
            "list strategies and set active strategy to panda strat",
        );
        for name in [
            "list_strategies",
            "get_strategy",
            "add_strategy",
            "set_active_strategy",
            "remove_strategy",
        ] {
            assert!(
                tools.iter().any(|tool| tool == name),
                "general strategy intent should include {name}; tools={tools:?}"
            );
        }
    }
}
