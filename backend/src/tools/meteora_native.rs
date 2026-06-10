use anyhow::{anyhow, Result};
use solana_client_v3::nonblocking::rpc_client::RpcClient;
use solana_sdk_v3::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{str::FromStr, sync::Arc};
use wp_solana_core::token::WorkspacePlanConfig;
use wp_solana_meteora_dlmm_client::generated::types::{StrategyParameters, StrategyType};
use wp_solana_meteora_dlmm_core::plan::{
    add_liquidity::{AddLiquidityParams, NewPositionConfig},
    claim_fee::ClaimFeeParams,
    close_position::ClosePositionParams,
};
use wp_solana_meteora_dlmm_sdk::orchestrate::{
    add_liquidity::add_liquidity_one_shot, claim_fee::claim_fee_one_shot,
    close_position::close_position_one_shot,
};
use wp_solana_rpc::RpcContext;

use crate::config::Config;

const DEFAULT_RPC_URL: &str = "https://api.mainnet-beta.solana.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeClaimRequest {
    pub position_address: String,
    pub authority: String,
    pub rpc_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCloseRequest {
    pub position_address: String,
    pub authority: String,
    pub rent_receiver: Option<String>,
    pub rpc_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDeployRequest {
    pub pool_address: String,
    pub position_address: String,
    pub authority: String,
    pub rpc_url: String,
    pub amount_x: u64,
    pub amount_y: u64,
    pub active_id: i32,
    pub min_bin_id: i32,
    pub max_bin_id: i32,
    pub width: i32,
    pub strategy: String,
    pub max_active_bin_slippage: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeClaimResult {
    pub signature: String,
    pub claimable_fee_x: u64,
    pub claimable_fee_y: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCloseResult {
    pub signature: String,
    pub remove_liquidity_amount_x: u64,
    pub remove_liquidity_amount_y: u64,
    pub claimable_fee_x: u64,
    pub claimable_fee_y: u64,
    pub claimable_rewards: [u64; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDeployResult {
    pub signature: String,
    pub position_address: String,
}

pub fn keypair_from_secret(secret: &str) -> Result<Keypair> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        anyhow::bail!("wallet private key is empty");
    }
    let bytes = if trimmed.starts_with('[') {
        serde_json::from_str::<Vec<u8>>(trimmed)
            .map_err(|e| anyhow!("invalid JSON wallet private key: {}", e))?
    } else {
        bs58::decode(trimmed)
            .into_vec()
            .map_err(|e| anyhow!("invalid base58 wallet private key: {}", e))?
    };
    if bytes.len() != 64 {
        anyhow::bail!(
            "wallet private key must decode to 64 bytes, got {} bytes",
            bytes.len()
        );
    }
    Keypair::try_from(bytes.as_slice()).map_err(|e| anyhow!("invalid wallet private key: {}", e))
}

pub fn keypair_pubkey_from_secret(secret: &str) -> Result<String> {
    Ok(keypair_from_secret(secret)?.pubkey().to_string())
}

pub fn wallet_secret_from_env() -> Result<String> {
    ["WALLET_PRIVATE_KEY", "MERIDIAN_WALLET_PRIVATE_KEY"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.trim().is_empty()))
        .ok_or_else(|| anyhow!("WALLET_PRIVATE_KEY or MERIDIAN_WALLET_PRIVATE_KEY is required for native Meteora transactions"))
}

pub fn resolve_rpc_url(config: &Config) -> String {
    config
        .api
        .helius_rpc_url
        .clone()
        .or_else(|| std::env::var("HELIUS_RPC_URL").ok())
        .or_else(|| std::env::var("RPC_URL").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_RPC_URL.to_string())
}

fn parse_pubkey(label: &str, value: &str) -> Result<Pubkey> {
    Pubkey::from_str(value).map_err(|e| anyhow!("invalid {} {}: {}", label, value, e))
}

fn sol_to_lamports(amount_sol: f64) -> Result<u64> {
    if !amount_sol.is_finite() || amount_sol <= 0.0 {
        anyhow::bail!("amount_sol must be a positive finite number");
    }
    Ok((amount_sol * 1_000_000_000.0).floor() as u64)
}

fn strategy_type_from_name(strategy: &str) -> StrategyType {
    match strategy.to_ascii_lowercase().replace('-', "_").as_str() {
        "curve" | "curve_one_side" => StrategyType::CurveOneSide,
        "bid_ask" | "bidask" | "bid_ask_one_side" => StrategyType::BidAskOneSide,
        _ => StrategyType::SpotOneSide,
    }
}

fn strategy_name(strategy_type: StrategyType) -> &'static str {
    match strategy_type {
        StrategyType::CurveOneSide => "curve_one_side",
        StrategyType::BidAskOneSide => "bid_ask_one_side",
        _ => "spot_one_side",
    }
}

fn strategy_parameters(min_bin_id: i32, max_bin_id: i32, strategy: &str) -> StrategyParameters {
    StrategyParameters {
        min_bin_id,
        max_bin_id,
        strategy_type: strategy_type_from_name(strategy),
        parameteres: [0; 64],
    }
}

fn bin_range(active_id: i32, bins_below: i64, bins_above: i64) -> Result<(i32, i32, i32)> {
    if bins_below < 0 || bins_above < 0 {
        anyhow::bail!("bins_below and bins_above must be non-negative");
    }
    let min_bin_id = active_id
        .checked_sub(bins_below as i32)
        .ok_or_else(|| anyhow!("min bin underflow"))?;
    let max_bin_id = active_id
        .checked_add(bins_above as i32)
        .ok_or_else(|| anyhow!("max bin overflow"))?;
    let width = max_bin_id
        .checked_sub(min_bin_id)
        .and_then(|span| span.checked_add(1))
        .ok_or_else(|| anyhow!("invalid bin range"))?;
    if width <= 0 {
        anyhow::bail!("bin range width must be positive");
    }
    Ok((min_bin_id, max_bin_id, width))
}

#[derive(Debug, Clone, Copy)]
pub struct NativeDeployBuildInput<'a> {
    pub pool_address: &'a str,
    pub amount_sol: f64,
    pub active_id: i32,
    pub bins_below: i64,
    pub bins_above: i64,
    pub strategy: &'a str,
}

pub fn build_deploy_request(
    input: NativeDeployBuildInput<'_>,
    config: &Config,
    wallet_secret: &str,
) -> Result<NativeDeployRequest> {
    let keypair = keypair_from_secret(wallet_secret)?;
    let pool = parse_pubkey("DLMM pool address", input.pool_address)?;
    let position_keypair = Keypair::new();
    let (min_bin_id, max_bin_id, width) =
        bin_range(input.active_id, input.bins_below, input.bins_above)?;
    let strategy_type = strategy_type_from_name(input.strategy);

    Ok(NativeDeployRequest {
        pool_address: pool.to_string(),
        position_address: position_keypair.pubkey().to_string(),
        authority: keypair.pubkey().to_string(),
        rpc_url: resolve_rpc_url(config),
        amount_x: 0,
        amount_y: sol_to_lamports(input.amount_sol)?,
        active_id: input.active_id,
        min_bin_id,
        max_bin_id,
        width,
        strategy: strategy_name(strategy_type).to_string(),
        max_active_bin_slippage: 1,
    })
}

pub fn build_claim_request(
    position_address: &str,
    config: &Config,
    wallet_secret: &str,
) -> Result<NativeClaimRequest> {
    let keypair = keypair_from_secret(wallet_secret)?;
    let position = parse_pubkey("DLMM position address", position_address)?;

    Ok(NativeClaimRequest {
        position_address: position.to_string(),
        authority: keypair.pubkey().to_string(),
        rpc_url: resolve_rpc_url(config),
    })
}

pub fn build_close_request(
    position_address: &str,
    config: &Config,
    wallet_secret: &str,
    rent_receiver: Option<&str>,
) -> Result<NativeCloseRequest> {
    let keypair = keypair_from_secret(wallet_secret)?;
    let position = parse_pubkey("DLMM position address", position_address)?;
    let authority = keypair.pubkey().to_string();
    let rent_receiver = rent_receiver
        .map(|value| parse_pubkey("rent receiver", value).map(|pubkey| pubkey.to_string()))
        .transpose()?
        .or_else(|| Some(authority.clone()));

    Ok(NativeCloseRequest {
        position_address: position.to_string(),
        authority,
        rent_receiver,
        rpc_url: resolve_rpc_url(config),
    })
}

pub async fn deploy_position(
    pool_address: &str,
    amount_sol: f64,
    active_id: i32,
    bins_below: i64,
    bins_above: i64,
    strategy: &str,
    config: &Config,
) -> Result<NativeDeployResult> {
    let wallet_secret = wallet_secret_from_env()?;
    let keypair = keypair_from_secret(&wallet_secret)?;
    let pool = parse_pubkey("DLMM pool address", pool_address)?;
    let (min_bin_id, max_bin_id, width) = bin_range(active_id, bins_below, bins_above)?;
    let amount_y = sol_to_lamports(amount_sol)?;
    let position_keypair = Keypair::new();
    let position_address = position_keypair.pubkey();
    let params = AddLiquidityParams {
        lb_pair_address: pool,
        position_address,
        new_position: Some(NewPositionConfig {
            position_keypair,
            lower_bin_id: min_bin_id,
            width,
        }),
        amount_x: 0,
        amount_y,
        active_id,
        max_active_bin_slippage: 1,
        strategy_parameters: strategy_parameters(min_bin_id, max_bin_id, strategy),
        authority: keypair.pubkey(),
    };

    let rpc_url = resolve_rpc_url(config);
    let rpc_client = RpcClient::new(rpc_url);
    let rpc_ctx = RpcContext::confirmed(Arc::new(rpc_client));
    let plan_config = WorkspacePlanConfig::default();
    let result = add_liquidity_one_shot(&rpc_ctx, params, &plan_config, &keypair)
        .await
        .map_err(|e| anyhow!("native Meteora add_liquidity_one_shot failed: {}", e))?;

    Ok(NativeDeployResult {
        signature: result.signature.to_string(),
        position_address: result.position_address.to_string(),
    })
}

pub async fn claim_fees(position_address: &str, config: &Config) -> Result<NativeClaimResult> {
    let wallet_secret = wallet_secret_from_env()?;
    let keypair = keypair_from_secret(&wallet_secret)?;
    let position = parse_pubkey("DLMM position address", position_address)?;
    let rpc_url = resolve_rpc_url(config);
    let rpc_client = RpcClient::new(rpc_url);
    let rpc_ctx = RpcContext::confirmed(Arc::new(rpc_client));
    let params = ClaimFeeParams {
        position_address: position,
        authority: keypair.pubkey(),
    };
    let plan_config = WorkspacePlanConfig::default();
    let result = claim_fee_one_shot(&rpc_ctx, params, &plan_config, &keypair)
        .await
        .map_err(|e| anyhow!("native Meteora claim_fee_one_shot failed: {}", e))?;

    Ok(NativeClaimResult {
        signature: result.signature.to_string(),
        claimable_fee_x: result.quote.claimable_fee_x,
        claimable_fee_y: result.quote.claimable_fee_y,
    })
}

pub async fn close_position(
    position_address: &str,
    config: &Config,
    rent_receiver: Option<&str>,
) -> Result<NativeCloseResult> {
    let wallet_secret = wallet_secret_from_env()?;
    let keypair = keypair_from_secret(&wallet_secret)?;
    let position = parse_pubkey("DLMM position address", position_address)?;
    let rent_receiver = rent_receiver
        .map(|value| parse_pubkey("rent receiver", value))
        .transpose()?;
    let rpc_url = resolve_rpc_url(config);
    let rpc_client = RpcClient::new(rpc_url);
    let rpc_ctx = RpcContext::confirmed(Arc::new(rpc_client));
    let params = ClosePositionParams {
        position_address: position,
        authority: keypair.pubkey(),
        rent_receiver,
    };
    let plan_config = WorkspacePlanConfig::default();
    let result = close_position_one_shot(&rpc_ctx, params, &plan_config, &keypair)
        .await
        .map_err(|e| anyhow!("native Meteora close_position_one_shot failed: {}", e))?;

    Ok(NativeCloseResult {
        signature: result.signature.to_string(),
        remove_liquidity_amount_x: result.quote.remove_liquidity_amount_x,
        remove_liquidity_amount_y: result.quote.remove_liquidity_amount_y,
        claimable_fee_x: result.quote.claimable_fee_x,
        claimable_fee_y: result.quote.claimable_fee_y,
        claimable_rewards: result.quote.claimable_rewards,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::{Keypair, Signer};

    #[test]
    fn keypair_pubkey_from_secret_matches_existing_wallet_secret() {
        let keypair = Keypair::new();
        let pubkey = keypair_pubkey_from_secret(&keypair.to_base58_string())
            .expect("v2 wallet secret should parse into native SDK keypair");

        assert_eq!(pubkey, keypair.pubkey().to_string());
    }

    #[test]
    fn build_claim_request_uses_position_and_wallet_authority() {
        let keypair = Keypair::new();
        let position = solana_sdk::pubkey::Pubkey::new_unique().to_string();
        let config = crate::config::Config {
            api: crate::config::types::ApiConfig {
                helius_rpc_url: Some("https://rpc.example.test".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let request = build_claim_request(&position, &config, &keypair.to_base58_string())
            .expect("claim request should be built from string inputs");

        assert_eq!(request.position_address, position);
        assert_eq!(request.authority, keypair.pubkey().to_string());
        assert_eq!(request.rpc_url, "https://rpc.example.test");
    }

    #[test]
    fn resolve_rpc_url_prefers_config_then_env_then_default() {
        let config = crate::config::Config {
            api: crate::config::types::ApiConfig {
                helius_rpc_url: Some("https://configured-rpc.example.test".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(
            resolve_rpc_url(&config),
            "https://configured-rpc.example.test"
        );
    }

    #[test]
    fn build_close_request_defaults_rent_receiver_to_authority() {
        let keypair = Keypair::new();
        let position = solana_sdk::pubkey::Pubkey::new_unique().to_string();
        let config = crate::config::Config {
            api: crate::config::types::ApiConfig {
                helius_rpc_url: Some("https://rpc.example.test".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let request = build_close_request(&position, &config, &keypair.to_base58_string(), None)
            .expect("close request should be built from string inputs");
        let authority = keypair.pubkey().to_string();

        assert_eq!(request.position_address, position);
        assert_eq!(request.authority, authority);
        assert_eq!(request.rent_receiver.as_deref(), Some(authority.as_str()));
        assert_eq!(request.rpc_url, "https://rpc.example.test");
    }

    #[test]
    fn build_deploy_request_maps_sol_amount_bins_and_strategy() {
        let keypair = Keypair::new();
        let pool = solana_sdk::pubkey::Pubkey::new_unique().to_string();
        let config = crate::config::Config {
            api: crate::config::types::ApiConfig {
                helius_rpc_url: Some("https://rpc.example.test".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let request = build_deploy_request(
            NativeDeployBuildInput {
                pool_address: &pool,
                amount_sol: 0.25,
                active_id: 100,
                bins_below: 35,
                bins_above: 0,
                strategy: "bid_ask",
            },
            &config,
            &keypair.to_base58_string(),
        )
        .expect("deploy request should be built from string inputs");

        assert_eq!(request.pool_address, pool);
        assert_eq!(request.authority, keypair.pubkey().to_string());
        assert_eq!(request.amount_x, 0);
        assert_eq!(request.amount_y, 250_000_000);
        assert_eq!(request.active_id, 100);
        assert_eq!(request.min_bin_id, 65);
        assert_eq!(request.max_bin_id, 100);
        assert_eq!(request.width, 36);
        assert_eq!(request.strategy, "bid_ask_one_side");
        assert_eq!(request.rpc_url, "https://rpc.example.test");
    }
}
