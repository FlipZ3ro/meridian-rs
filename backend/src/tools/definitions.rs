use crate::llm::ToolDefinition;
use serde_json::json;

/// Build all tool definitions for OpenAI function calling format
pub fn get_all_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        tool(
            "get_wallet_balance",
            "Get SOL and token balances for the wallet",
            json!({
                "type": "object", "properties": {}, "required": []
            }),
        ),
        tool(
            "get_recent_decisions",
            "Get recent decisions from decision-log.json so the agent avoids repeating actions",
            json!({
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "description": "Maximum recent decisions to return (default 5)"}
                },
                "required": []
            }),
        ),
        tool(
            "get_performance_history",
            "Retrieve recent closed-position performance history with PnL, fees, range efficiency, and close reasons",
            json!({
                "type": "object",
                "properties": {
                    "hours": {"type": "number", "description": "How many hours back to look (default 24; use 168 for 7 days)"},
                    "limit": {"type": "integer", "description": "Maximum closed positions to return (default 50)"}
                },
                "required": []
            }),
        ),
        tool(
            "get_my_positions",
            "Get all open DLMM positions for the wallet",
            json!({
                "type": "object", "properties": {}, "required": []
            }),
        ),
        tool(
            "get_position_pnl",
            "Get PnL and fees for a specific position",
            json!({
                "type": "object",
                "properties": {
                    "position_id": {"type": "string", "description": "The position ID or pool address"}
                },
                "required": ["position_id"]
            }),
        ),
        tool(
            "deploy_position",
            "Deploy SOL into a DLMM pool with specified bins",
            json!({
                "type": "object",
                "properties": {
                    "pool_address": {"type": "string", "description": "The DLMM pool address"},
                    "amount_y": {"type": "number", "description": "Amount of SOL (quote token) to deploy"},
                    "amount_sol": {"type": "number", "description": "Amount of SOL to deploy (alternative to amount_y)"},
                    "bins_below": {"type": "integer", "description": "Number of bins below active bin"},
                    "bins_above": {"type": "integer", "description": "Number of bins above active bin (set to 0)"}
                },
                "required": ["pool_address"]
            }),
        ),
        tool(
            "close_position",
            "Close a DLMM position and withdraw liquidity; auto-swaps base token to SOL unless skip_swap is true",
            json!({
                "type": "object",
                "properties": {
                    "position_id": {"type": "string", "description": "The tracked position ID to close"},
                    "position_address": {"type": "string", "description": "The DLMM position address to close (alternative to position_id)"},
                    "reason": {"type": "string", "description": "Reason for closing, used for audit/pool-memory notes"},
                    "skip_swap": {"type": "boolean", "description": "If true, skip automatic base-token swap back to SOL after close"}
                },
                "required": ["position_id"]
            }),
        ),
        tool(
            "claim_fees",
            "Claim accumulated fees from a position",
            json!({
                "type": "object",
                "properties": {
                    "position_id": {"type": "string", "description": "The position ID to claim fees from"}
                },
                "required": ["position_id"]
            }),
        ),
        tool(
            "swap_token",
            "Swap a token to SOL via Jupiter",
            json!({
                "type": "object",
                "properties": {
                    "mint": {"type": "string", "description": "Token mint address to swap"},
                    "amount": {"type": "number", "description": "Amount to swap (0 = all)"}
                },
                "required": ["mint"]
            }),
        ),
        tool(
            "get_top_candidates",
            "Screen Meteora DLMM pools and get top candidates",
            json!({
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "description": "Max candidates to return (default 3)"}
                },
                "required": []
            }),
        ),
        tool(
            "search_pools",
            "Search for pools by token symbol or address",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query (symbol or mint address)"}
                },
                "required": ["query"]
            }),
        ),
        tool(
            "get_active_bin",
            "Get the active bin for a pool",
            json!({
                "type": "object",
                "properties": {
                    "pool_address": {"type": "string", "description": "Pool address"}
                },
                "required": ["pool_address"]
            }),
        ),
        tool(
            "get_token_info",
            "Get token metadata from Jupiter",
            json!({
                "type": "object",
                "properties": {
                    "mint": {"type": "string", "description": "Token mint address"}
                },
                "required": ["mint"]
            }),
        ),
        tool(
            "get_token_holders",
            "Get top token holders",
            json!({
                "type": "object",
                "properties": {
                    "mint": {"type": "string", "description": "Token mint address"}
                },
                "required": ["mint"]
            }),
        ),
        tool(
            "get_token_narrative",
            "Get narrative/description for a token",
            json!({
                "type": "object",
                "properties": {
                    "mint": {"type": "string", "description": "Token mint address"}
                },
                "required": ["mint"]
            }),
        ),
        tool(
            "check_smart_wallets_on_pool",
            "Check if any tracked smart wallets are LPing in a pool",
            json!({
                "type": "object",
                "properties": {
                    "pool_address": {"type": "string", "description": "Pool address to check"}
                },
                "required": ["pool_address"]
            }),
        ),
        tool(
            "get_top_lpers",
            "Get the top open LPers for a pool by address using Agent Meridian LPAgent data",
            json!({
                "type": "object",
                "properties": {
                    "pool_address": {"type": "string", "description": "Pool address to look up top LPers for"},
                    "limit": {"type": "integer", "description": "Number of top LPers to return (default 5)"}
                },
                "required": ["pool_address"]
            }),
        ),
        tool(
            "study_top_lpers",
            "Fetch and analyze top open LPers for a pool to learn behavior patterns, hold times, win rates, and range styles before deploying",
            json!({
                "type": "object",
                "properties": {
                    "pool_address": {"type": "string", "description": "Pool address to study top LPers for"},
                    "limit": {"type": "integer", "description": "Number of top LPers to study (default 4)"}
                },
                "required": ["pool_address"]
            }),
        ),
        tool(
            "get_pool_memory",
            "Get pool history and notes",
            json!({
                "type": "object",
                "properties": {}, "required": []
            }),
        ),
        tool(
            "add_pool_note",
            "Add a note to a pool's memory",
            json!({
                "type": "object",
                "properties": {
                    "pool_address": {"type": "string"},
                    "base_mint": {"type": "string"},
                    "note": {"type": "string"}
                },
                "required": ["pool_address", "base_mint", "note"]
            }),
        ),
        tool(
            "discover_pools",
            "Fetch raw pool data from Meteora Pool Discovery API",
            json!({
                "type": "object",
                "properties": {
                    "page_size": {"type": "integer", "description": "Number of pools to fetch (default 50)"}
                },
                "required": []
            }),
        ),
        tool(
            "add_liquidity",
            "Add more liquidity to an existing position (increase size)",
            json!({
                "type": "object",
                "properties": {
                    "position_id": {"type": "string", "description": "Position ID to add liquidity to"},
                    "amount_sol": {"type": "number", "description": "Amount of SOL to add"}
                },
                "required": ["position_id", "amount_sol"]
            }),
        ),
        tool(
            "withdraw_liquidity",
            "Partially withdraw liquidity from a position (reduce size or harvest)",
            json!({
                "type": "object",
                "properties": {
                    "position_id": {"type": "string", "description": "Position ID to withdraw from"},
                    "pct": {"type": "number", "description": "Percentage to withdraw (0-100)"}
                },
                "required": ["position_id", "pct"]
            }),
        ),
        tool(
            "add_strategy",
            "Save a new LP strategy to the strategy library after parsing strategy text or user instructions",
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Short slug e.g. overnight_classic_bid_ask or panda_strat"},
                    "name": {"type": "string", "description": "Human-readable strategy name"},
                    "author": {"type": "string", "description": "Strategy author or source"},
                    "lp_strategy": {"type": "string", "enum": ["bid_ask", "spot", "curve", "any", "mixed"], "description": "LP strategy type"},
                    "token_criteria": {"type": "object", "description": "Token selection criteria and notes"},
                    "entry": {"type": "object", "description": "Entry conditions"},
                    "range": {"type": "object", "description": "Bin range / distribution guidance"},
                    "exit": {"type": "object", "description": "Exit rules"},
                    "best_for": {"type": "string", "description": "Ideal market conditions"},
                    "raw": {"type": "string", "description": "Original tweet or text"}
                },
                "required": ["id", "name"]
            }),
        ),
        tool(
            "list_strategies",
            "List saved LP strategies and show which one is active",
            json!({"type": "object", "properties": {}, "required": []}),
        ),
        tool(
            "get_strategy",
            "Get full details for a saved LP strategy",
            json!({
                "type": "object",
                "properties": {"id": {"type": "string", "description": "Strategy ID"}},
                "required": ["id"]
            }),
        ),
        tool(
            "set_active_strategy",
            "Set which saved strategy should guide the next screening/deployment cycle",
            json!({
                "type": "object",
                "properties": {"id": {"type": "string", "description": "Strategy ID to activate"}},
                "required": ["id"]
            }),
        ),
        tool(
            "remove_strategy",
            "Remove a strategy from the library",
            json!({
                "type": "object",
                "properties": {"id": {"type": "string", "description": "Strategy ID to remove"}},
                "required": ["id"]
            }),
        ),
        tool(
            "update_config",
            "Update a bot configuration parameter at runtime. Self-tuning.",
            json!({
                "type": "object",
                "properties": {
                    "key": {"type": "string", "description": "Config key (e.g. 'screening.minOrganic', 'risk.maxDeployAmount')"},
                    "value": {"description": "New value (auto-coerced to correct type)"}
                },
                "required": ["key", "value"]
            }),
        ),
        tool(
            "blacklist_token",
            "Blacklist a token mint to prevent future deploys",
            json!({
                "type": "object",
                "properties": {
                    "mint": {"type": "string"},
                    "reason": {"type": "string"}
                },
                "required": ["mint"]
            }),
        ),
        tool(
            "unblock_dev",
            "Remove a developer wallet from the dev blocklist",
            json!({
                "type": "object",
                "properties": {
                    "wallet": {"type": "string"}
                },
                "required": ["wallet"]
            }),
        ),
        tool(
            "get_wallet_positions",
            "Fetch DLMM positions for any wallet (competitive analysis)",
            json!({
                "type": "object",
                "properties": {
                    "wallet": {"type": "string", "description": "Wallet address to query"}
                },
                "required": []
            }),
        ),
    ]
}

fn tool(name: &str, desc: &str, params: serde_json::Value) -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".to_string(),
        function: crate::llm::ToolFunction {
            name: name.to_string(),
            description: desc.to_string(),
            parameters: params,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_decisions_tool_is_defined_with_limit_parameter() {
        let tools = get_all_tool_definitions();
        let tool = tools
            .iter()
            .find(|tool| tool.function.name == "get_recent_decisions")
            .expect("get_recent_decisions should be exposed as an LLM tool");

        assert!(tool
            .function
            .description
            .to_ascii_lowercase()
            .contains("recent decisions"));
        assert_eq!(
            tool.function.parameters["properties"]["limit"]["type"],
            "integer"
        );
        assert_eq!(
            tool.function.parameters["required"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn performance_history_tool_is_defined_with_hours_and_limit_parameters() {
        let tools = get_all_tool_definitions();
        let tool = tools
            .iter()
            .find(|tool| tool.function.name == "get_performance_history")
            .expect("get_performance_history should be exposed as an LLM tool");

        assert!(tool
            .function
            .description
            .to_ascii_lowercase()
            .contains("performance"));
        assert_eq!(
            tool.function.parameters["properties"]["hours"]["type"],
            "number"
        );
        assert_eq!(
            tool.function.parameters["properties"]["limit"]["type"],
            "integer"
        );
    }

    #[test]
    fn top_lper_tools_are_defined_for_llm_use() {
        let tools = get_all_tool_definitions();
        for name in ["get_top_lpers", "study_top_lpers"] {
            let tool = tools
                .iter()
                .find(|tool| tool.function.name == name)
                .unwrap_or_else(|| panic!("{name} should be exposed as an LLM tool"));
            assert!(tool
                .function
                .description
                .to_ascii_lowercase()
                .contains("lpers"));
            assert_eq!(
                tool.function.parameters["properties"]["pool_address"]["type"],
                "string"
            );
            assert_eq!(
                tool.function.parameters["properties"]["limit"]["type"],
                "integer"
            );
            assert_eq!(tool.function.parameters["required"][0], "pool_address");
        }
    }

    #[test]
    fn strategy_library_tools_are_defined_for_llm_use() {
        let tools = get_all_tool_definitions();
        for name in [
            "add_strategy",
            "list_strategies",
            "get_strategy",
            "set_active_strategy",
            "remove_strategy",
        ] {
            assert!(
                tools.iter().any(|tool| tool.function.name == name),
                "{name} should be exposed as an LLM tool"
            );
        }

        let add = tools
            .iter()
            .find(|tool| tool.function.name == "add_strategy")
            .expect("add_strategy should exist");
        assert_eq!(add.function.parameters["required"][0], "id");
        assert_eq!(add.function.parameters["required"][1], "name");
        assert_eq!(
            add.function.parameters["properties"]["lp_strategy"]["enum"][0],
            "bid_ask"
        );
    }
}
