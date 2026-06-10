use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub mint: String,
    pub symbol: String,
    pub name: Option<String>,
    pub decimals: u8,
    pub price_usd: f64,
    pub market_cap: Option<f64>,
    pub organic_score: Option<f64>,
    pub holder_count: Option<u64>,
    pub launchpad: Option<String>,
    pub dev: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JupiterAsset {
    pub id: String,
    #[serde(rename = "type")]
    pub asset_type: Option<String>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<u8>,
    pub organic_score: Option<f64>,
    pub holder_count: Option<u64>,
    pub mcap: Option<f64>,
    pub fdv: Option<f64>,
    pub launchpad: Option<String>,
    pub launchpad_platform: Option<String>,
    pub dev: Option<String>,
    pub created_at: Option<String>,
    pub fees: Option<f64>,
    pub liquidity: Option<f64>,
}
