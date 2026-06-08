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
            "Close a DLMM position and withdraw liquidity",
            json!({
                "type": "object",
                "properties": {
                    "position_id": {"type": "string", "description": "The position ID to close"}
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
