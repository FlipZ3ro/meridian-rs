use serde::{Deserialize, Serialize};

/// Raw pool from Meteora Pool Discovery API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RawPool {
    pub pool_address: Option<String>,
    pub name: Option<String>,
    pub pool_type: Option<String>,
    pub tvl: Option<f64>,
    pub active_tvl: Option<f64>,
    pub volume: Option<f64>,
    pub fee: Option<f64>,
    pub fee_active_tvl_ratio: Option<f64>,
    pub volatility: Option<f64>,
    pub base_token_holders: Option<u64>,
    pub base_token_mcap: Option<f64>,
    pub base_token_organic_score: Option<f64>,
    pub quote_token_organic_score: Option<f64>,
    pub base_token_has_critical_warnings: Option<bool>,
    pub quote_token_has_critical_warnings: Option<bool>,
    pub base_token_has_high_supply_concentration: Option<bool>,
    pub base_token_has_high_single_ownership: Option<bool>,
    pub base_token_launchpad: Option<String>,
    pub dlmm_bin_step: Option<u16>,
    pub token_x: Option<TokenX>,
    pub token_y: Option<TokenY>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenX {
    pub address: Option<String>,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub market_cap: Option<f64>,
    pub organic_score: Option<f64>,
    pub launchpad: Option<String>,
    pub launchpad_platform: Option<String>,
    pub created_at: Option<f64>,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenY {
    pub address: Option<String>,
    pub symbol: Option<String>,
    pub organic_score: Option<f64>,
}

/// Condensed pool data for LLM consumption (token-optimized)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CondensedPool {
    pub name: String,
    pub pool_address: String,
    pub base: PoolToken,
    pub quote_organic: f64,
    pub tvl: f64,
    pub volume: f64,
    pub fee_active_tvl_ratio: f64,
    pub volatility: f64,
    pub bin_step: u16,
    pub score: f64,
    pub holders: u64,
    pub mcap: f64,
    pub fees_sol: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launchpad: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundlers_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top10_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord_signal: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_pvp: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pvp_risk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pvp_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pvp_rival_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pvp_rival_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pvp_rival_pool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pvp_rival_tvl: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pvp_rival_holders: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pvp_rival_fees: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolToken {
    pub mint: String,
    pub symbol: String,
    pub organic: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// Pool Discovery API response wrapper
#[derive(Debug, Deserialize)]
pub struct PoolDiscoveryResponse {
    pub data: Option<Vec<RawPool>>,
}

/// Candidate scored and ready for the screener LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredCandidate {
    #[serde(flatten)]
    pub pool: CondensedPool,
    pub active_bin: Option<ActiveBin>,
    pub narrative: Option<String>,
    pub smart_wallets: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveBin {
    pub bin_id: i32,
    pub price: f64,
}
