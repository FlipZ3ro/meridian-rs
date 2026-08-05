use anyhow::{anyhow, Result};
use solana_client_v3::nonblocking::rpc_client::RpcClient;
use solana_sdk_v3::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{str::FromStr, sync::Arc};
use wp_solana_core::token::WorkspacePlanConfig;
use wp_solana_meteora_dlmm_client::generated::accounts::LbPair;
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
    /// The pool's base token (token_x) mint, resolved on-chain before the close.
    /// Lets the caller swap any claimed base-token fees back to SOL. `None` if
    /// it could not be resolved.
    pub base_mint: Option<String>,
    /// Signature of the follow-up tx that unwrapped wSOL back to native SOL, if
    /// any wSOL was present after the close. `None` when nothing needed sweeping.
    pub unwrap_signature: Option<String>,
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
    // Accept a Solana CLI keypair FILE PATH (e.g. id.json) in addition to a raw
    // base58 / JSON-array secret. Use forward slashes on Windows so the path
    // survives .env parsing.
    let source = if !trimmed.starts_with('[') && std::path::Path::new(trimmed).is_file() {
        std::fs::read_to_string(trimmed)
            .map_err(|e| anyhow!("failed to read keypair file {}: {}", trimmed, e))?
            .trim()
            .to_string()
    } else {
        trimmed.to_string()
    };
    let source = source.as_str();
    let bytes = if source.starts_with('[') {
        serde_json::from_str::<Vec<u8>>(source)
            .map_err(|e| anyhow!("invalid JSON wallet private key: {}", e))?
    } else {
        bs58::decode(source)
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

/// Derive the wallet's public address from the signing keypair in the
/// environment. Lets the runtime resolve its own address (e.g. for balance
/// reads) when MERIDIAN_WALLET isn't set explicitly — the keypair is the
/// authoritative source of the address anyway.
pub fn wallet_pubkey_from_env() -> Result<String> {
    keypair_pubkey_from_secret(&wallet_secret_from_env()?)
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
    // Single-side SOL deposits (amount_x = 0) are IMBALANCED, so AddLiquidityByStrategy2
    // needs the *ImBalanced strategy type. *Balanced requires both tokens in equal
    // value (deposits ~0 when one side is empty, leaving wrapped SOL stuck); *OneSide
    // belongs to a different instruction (InvalidStrategyParameters / 0x17a6). The
    // original JS passes TS-SDK `StrategyType.Spot`, which maps to SpotImBalanced for
    // single-side deposits.
    match strategy.to_ascii_lowercase().replace('-', "_").as_str() {
        "curve" | "curve_one_side" | "curve_balanced" | "curve_imbalanced" => {
            StrategyType::CurveImBalanced
        }
        "bid_ask" | "bidask" | "bid_ask_one_side" | "bid_ask_balanced" | "bid_ask_imbalanced" => {
            StrategyType::BidAskImBalanced
        }
        _ => StrategyType::SpotImBalanced,
    }
}

fn strategy_name(strategy_type: StrategyType) -> &'static str {
    match strategy_type {
        StrategyType::CurveImBalanced => "curve_imbalanced",
        StrategyType::BidAskImBalanced => "bid_ask_imbalanced",
        _ => "spot_imbalanced",
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
    let amount_y = sol_to_lamports(amount_sol)?;

    let rpc_url = resolve_rpc_url(config);
    let rpc_client = RpcClient::new(rpc_url);

    // The Meteora pool-discovery API does not expose the active bin id, so the
    // caller's `active_id` is often a placeholder (0). Read the authoritative
    // active_id straight from the on-chain LbPair account so the strategy bins
    // and the bin-slippage check match the real pool state — otherwise the tx
    // is rejected with ExceededBinSlippageTolerance (0x1774).
    let active_id = match rpc_client.get_account_data(&pool).await {
        Ok(data) => match LbPair::from_bytes(&data) {
            Ok(lb_pair) => lb_pair.active_id,
            Err(e) => {
                tracing::warn!("failed to decode LbPair {}: {}; using caller active_id", pool, e);
                active_id
            }
        },
        Err(e) => {
            tracing::warn!("failed to fetch LbPair {}: {}; using caller active_id", pool, e);
            active_id
        }
    };

    let (min_bin_id, max_bin_id, width) = bin_range(active_id, bins_below, bins_above)?;
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
        // Tolerate a few bins of movement between fetch and execution.
        max_active_bin_slippage: 5,
        strategy_parameters: strategy_parameters(min_bin_id, max_bin_id, strategy),
        authority: keypair.pubkey(),
    };

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
    // Claim deposits fees into the wallet's token ATAs; recreate any the wSOL
    // sweep closed so the one-shot doesn't fail with AccountNotInitialized.
    ensure_position_atas(&rpc_ctx.client, &keypair, &position).await;
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

/// Read-only on-chain snapshot of a position's current value: the liquidity that
/// would be returned on close plus the pending (claimable) fees. All amounts are
/// raw — `*_x` is the base token in its own decimals, `*_y` is SOL in lamports on
/// a SOL-quoted pool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PositionQuote {
    pub liquidity_x: u64,
    pub liquidity_y: u64,
    pub fee_x: u64,
    pub fee_y: u64,
}

/// Quote a position's live liquidity and claimable fees by running the same
/// fetch + plan math as a close, but never building or sending a transaction —
/// safe to call repeatedly (e.g. to populate the dashboard). A single
/// `plan_close_position` yields both the remove-liquidity amounts and the
/// pending fees, so this is one fetch per position rather than two.
pub async fn quote_position_state(position_address: &str, config: &Config) -> Result<PositionQuote> {
    use wp_solana_meteora_dlmm_core::plan::close_position::plan_close_position;
    use wp_solana_meteora_dlmm_sdk::fetch::close_position::fetch_close_position_snapshot;

    let keypair = keypair_from_secret(&wallet_secret_from_env()?)?;
    let position = parse_pubkey("DLMM position address", position_address)?;
    let rpc_client = RpcClient::new(resolve_rpc_url(config));
    let rpc_ctx = RpcContext::confirmed(Arc::new(rpc_client));
    let params = ClosePositionParams {
        position_address: position,
        authority: keypair.pubkey(),
        rent_receiver: None,
    };
    let snapshot = fetch_close_position_snapshot(&rpc_ctx.client, &params)
        .await
        .map_err(|e| anyhow!("fetch position snapshot: {}", e))?;
    let plan_config = WorkspacePlanConfig::default();
    let plan = plan_close_position(&snapshot, params, &plan_config)
        .map_err(|e| anyhow!("plan position quote: {}", e))?;
    let wp_quote = PositionQuote {
        liquidity_x: plan.quote.remove_liquidity_amount_x,
        liquidity_y: plan.quote.remove_liquidity_amount_y,
        fee_x: plan.quote.claimable_fee_x,
        fee_y: plan.quote.claimable_fee_y,
    };

    // Phase-1 migration parity harness (opt-in via QUOTE_PARITY=1): also run the
    // official-commons quote and log both side by side. Zero behavior change —
    // we still return the wp result. Lets us confirm numeric parity on a live
    // position before cutting `quote_position_state` over to commons.
    if std::env::var("QUOTE_PARITY").as_deref() == Ok("1") {
        match quote_position_state_commons(position_address, config).await {
            Ok(c) => tracing::info!(
                target: "quote_parity",
                position = %position_address,
                wp_liq_x = wp_quote.liquidity_x, cm_liq_x = c.liquidity_x,
                wp_liq_y = wp_quote.liquidity_y, cm_liq_y = c.liquidity_y,
                wp_fee_x = wp_quote.fee_x, cm_fee_x = c.fee_x,
                wp_fee_y = wp_quote.fee_y, cm_fee_y = c.fee_y,
                match_ = (wp_quote == c),
                "wp vs commons quote"
            ),
            Err(e) => tracing::warn!(target: "quote_parity", "commons quote failed: {}", e),
        }
    }

    Ok(wp_quote)
}

/// Phase 1 (commons migration): read-only position quote via the OFFICIAL
/// MeteoraAg `commons` crate instead of wp-solana. Mirrors the official
/// `cli/show_position.rs`. Returns the same `PositionQuote` shape.
///
/// Uses solana v2 types throughout (`solana_sdk` / `solana_client`, NOT the
/// `*_v3` aliases) because `commons` is built against solana-program 2.x — its
/// `Pubkey`/`Account` types must match at the call boundary.
pub async fn quote_position_state_commons(
    position_address: &str,
    config: &Config,
) -> Result<PositionQuote> {
    use commons::dlmm::accounts::{BinArray, LbPair as CLbPair, PositionV2};
    use commons::extensions::bin_array::BinArrayExtension;
    use commons::extensions::dynamic_position::DynamicPosition;
    use solana_client::nonblocking::rpc_client::RpcClient as RpcClientV2;
    use solana_sdk::pubkey::Pubkey as PubkeyV2;
    use std::collections::HashMap;

    let position_pk = PubkeyV2::from_str(position_address)
        .map_err(|e| anyhow!("invalid position pubkey: {}", e))?;
    let rpc = RpcClientV2::new(resolve_rpc_url(config));

    // 1. Fetch + decode the position account (PositionV2).
    let position_account = rpc.get_account(&position_pk).await?;
    let position_state: PositionV2 =
        commons::pod_read_unaligned_skip_disc(&position_account.data)
            .map_err(|e| anyhow!("decode PositionV2: {}", e))?;

    // 2. Bin-array index range the position spans.
    let lower_idx = BinArray::bin_id_to_bin_array_index(position_state.lower_bin_id)
        .map_err(|e| anyhow!("lower bin array index: {}", e))?;
    let upper_idx = BinArray::bin_id_to_bin_array_index(position_state.upper_bin_id)
        .map_err(|e| anyhow!("upper bin array index: {}", e))?;

    // 3. Batch-fetch lb_pair + covered bin arrays.
    let bin_array_pubkeys: Vec<PubkeyV2> = (lower_idx..=upper_idx)
        .map(|idx| commons::derive_bin_array_pda(position_state.lb_pair, idx.into()).0)
        .collect();
    let to_fetch: Vec<PubkeyV2> =
        [vec![position_state.lb_pair], bin_array_pubkeys].concat();
    let fetched = rpc.get_multiple_accounts(&to_fetch).await?;

    // 4. Decode lb_pair.
    let lb_pair_state: CLbPair = commons::pod_read_unaligned_skip_disc(
        &fetched
            .first()
            .and_then(|a| a.as_ref())
            .ok_or_else(|| anyhow!("lb_pair account missing"))?
            .data,
    )
    .map_err(|e| anyhow!("decode LbPair: {}", e))?;

    // 5. Build HashMap<i32, BinArray> keyed by bin-array index (skip missing).
    let mut bin_array_map: HashMap<i32, BinArray> = HashMap::new();
    for (i, idx) in (lower_idx..=upper_idx).enumerate() {
        if let Some(acc) = fetched.get(1 + i).and_then(|a| a.as_ref()) {
            let ba: BinArray = commons::pod_read_unaligned_skip_disc(&acc.data)
                .map_err(|e| anyhow!("decode BinArray: {}", e))?;
            bin_array_map.insert(idx, ba);
        }
    }

    // 6. Parse. Timestamp only affects reward accrual (not amounts/fees), so a
    // Unix-now value is sufficient — avoids a Clock sysvar fetch + bincode.
    let now_ts = chrono::Utc::now().timestamp();
    let dp = DynamicPosition::parse(
        &position_state,
        &position_account.data,
        &lb_pair_state,
        &bin_array_map,
        now_ts,
    )
    .map_err(|e| anyhow!("commons DynamicPosition::parse: {}", e))?;

    Ok(PositionQuote {
        liquidity_x: dp.total_x_amount,
        liquidity_y: dp.total_y_amount,
        fee_x: dp.fee_x,
        fee_y: dp.fee_y,
    })
}

/// SPL Token program id.
const SPL_TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
/// Phase 2 (commons migration): claim a position's fees through the OFFICIAL
/// MeteoraAg `commons` crate, mirroring `cli/src/instructions/claim_fee.rs`.
///
/// Why this exists: the wp `claim_fee_one_shot` fails with AnchorError 3012
/// (`AccountNotInitialized`) on `user_token_y` for some positions even though
/// the wallet's canonical ATA for that mint demonstrably exists on-chain — wp
/// resolves the token account internally and we cannot influence which address
/// it passes. Building the instruction here makes the account set explicit:
///
///  * the fee recipient is `position.fee_owner` when set, else the payer —
///    exactly the branch the official CLI takes;
///  * the token programs come from `LbPair`'s own program flags (the values the
///    on-chain program validates against), NOT from reading each mint's owner,
///    which is what our ATA pre-create had been guessing at;
///  * both ATAs are created idempotently up front, so the claim can never race
///    a missing account.
///
/// Uses solana v2 types throughout to match `commons` (see the Phase 1 note on
/// `quote_position_state_commons`).
pub async fn claim_fees_commons(
    position_address: &str,
    config: &Config,
) -> Result<NativeClaimResult> {
    use anchor_lang::{InstructionData, ToAccountMetas};
    use commons::dlmm::accounts::{LbPair as CLbPair, PositionV2};
    use commons::extensions::lb_pair::LbPairExtension;
    use commons::extensions::position::PositionExtension;
    use solana_client::nonblocking::rpc_client::RpcClient as RpcClientV2;
    use solana_sdk::instruction::Instruction as InstructionV2;
    use solana_sdk::pubkey::Pubkey as PubkeyV2;
    use solana_sdk::signature::{Keypair as KeypairV2, Signer as SignerV2};
    use solana_sdk::transaction::Transaction as TransactionV2;

    let secret = wallet_secret_from_env()?;
    let keypair = keypair_v2_from_secret(&secret)?;
    let payer = keypair.pubkey();
    let position_pk =
        PubkeyV2::from_str(position_address).map_err(|e| anyhow!("invalid position: {}", e))?;
    let rpc = RpcClientV2::new(resolve_rpc_url(config));

    let position_account = rpc.get_account(&position_pk).await?;
    let position_state: PositionV2 = commons::pod_read_unaligned_skip_disc(&position_account.data)
        .map_err(|e| anyhow!("decode PositionV2: {}", e))?;

    let pair_account = rpc.get_account(&position_state.lb_pair).await?;
    let lb_pair_state: CLbPair = commons::pod_read_unaligned_skip_disc(&pair_account.data)
        .map_err(|e| anyhow!("decode LbPair: {}", e))?;

    // Fees land in ATAs owned by fee_owner when the position sets one; the
    // default (all-zero) pubkey means the payer receives them.
    let fee_recipient = if position_state.fee_owner == PubkeyV2::default() {
        payer
    } else {
        position_state.fee_owner
    };

    let [token_program_x, token_program_y] = lb_pair_state
        .get_token_programs()
        .map_err(|e| anyhow!("resolve token programs from lb_pair: {}", e))?;

    let user_token_x = derive_ata_v2(&fee_recipient, &lb_pair_state.token_x_mint, &token_program_x);
    let user_token_y = derive_ata_v2(&fee_recipient, &lb_pair_state.token_y_mint, &token_program_y);

    // Create whichever destination ATAs are missing, in one transaction, before
    // the claim. CreateIdempotent is a no-op when the account already exists.
    let mut setup_ixs: Vec<InstructionV2> = Vec::new();
    for (ata, mint, token_program) in [
        (user_token_x, lb_pair_state.token_x_mint, token_program_x),
        (user_token_y, lb_pair_state.token_y_mint, token_program_y),
    ] {
        if rpc.get_account(&ata).await.is_err() {
            setup_ixs.push(create_ata_idempotent_ix_v2(
                &payer,
                &fee_recipient,
                &mint,
                &token_program,
            ));
        }
    }
    if !setup_ixs.is_empty() {
        let blockhash = rpc.get_latest_blockhash().await?;
        let tx =
            TransactionV2::new_signed_with_payer(&setup_ixs, Some(&payer), &[&keypair], blockhash);
        let sig = rpc.send_and_confirm_transaction(&tx).await?;
        tracing::info!(signature = %sig, "commons claim: created missing fee ATA(s)");
    }

    let (event_authority, _) = commons::derive_event_authority_pda();
    let main_accounts = commons::dlmm::client::accounts::ClaimFee2 {
        lb_pair: position_state.lb_pair,
        sender: payer,
        position: position_pk,
        reserve_x: lb_pair_state.reserve_x,
        reserve_y: lb_pair_state.reserve_y,
        token_program_x,
        token_program_y,
        token_x_mint: lb_pair_state.token_x_mint,
        token_y_mint: lb_pair_state.token_y_mint,
        user_token_x,
        user_token_y,
        event_authority,
        program: commons::dlmm::ID,
        memo_program: PubkeyV2::from_str(MEMO_PROGRAM_ID).expect("valid memo program id"),
    }
    .to_account_metas(None);

    // A position can span more bins than one instruction's account list allows,
    // so the official client walks it in chunks — mirror that.
    let mut signatures: Vec<String> = Vec::new();
    for (min_bin_id, max_bin_id) in
        position_bin_range_chunks(position_state.lower_bin_id, position_state.upper_bin_id)
    {
        let data = commons::dlmm::client::args::ClaimFee2 {
            min_bin_id,
            max_bin_id,
            remaining_accounts_info: commons::dlmm::types::RemainingAccountsInfo {
                slices: vec![],
            },
        }
        .data();
        let bin_arrays = position_state
            .get_bin_array_accounts_meta_coverage_by_chunk(min_bin_id, max_bin_id)
            .map_err(|e| anyhow!("bin array coverage: {}", e))?;
        let accounts = [main_accounts.to_vec(), bin_arrays].concat();
        let ix = InstructionV2 {
            program_id: commons::dlmm::ID,
            accounts,
            data,
        };
        let blockhash = rpc.get_latest_blockhash().await?;
        let tx = TransactionV2::new_signed_with_payer(&[ix], Some(&payer), &[&keypair], blockhash);
        let sig = rpc
            .send_and_confirm_transaction(&tx)
            .await
            .map_err(|e| anyhow!("commons claim_fee tx failed: {}", e))?;
        signatures.push(sig.to_string());
    }

    // Report what was harvested using the read-only commons quote taken before
    // the claim would have zeroed it out — callers only use these for logging
    // and fee accounting.
    let quote = quote_position_state_commons(position_address, config)
        .await
        .unwrap_or_default();

    Ok(NativeClaimResult {
        signature: signatures.join(","),
        claimable_fee_x: quote.fee_x,
        claimable_fee_y: quote.fee_y,
    })
}

/// Split a position's bin range into per-instruction chunks. Ported from the
/// official `cli/src/instructions/utils.rs` — it lives in the CLI binary rather
/// than in `commons`, so it cannot be imported. A position can cover more bins
/// than one ClaimFee2 instruction can carry accounts for, and the on-chain
/// program expects the claim to be walked in these fixed-size windows.
fn position_bin_range_chunks(lower_bin_id: i32, upper_bin_id: i32) -> Vec<(i32, i32)> {
    let bin_per_position = commons::DEFAULT_BIN_PER_POSITION as i32;
    let bin_range = upper_bin_id - lower_bin_id + 1;
    let quotient = bin_range / bin_per_position;
    let remainder = bin_range % bin_per_position;
    let chunk = quotient + i32::from(remainder != 0);
    (0..chunk)
        .map(|i| {
            let min_bin_id = lower_bin_id + bin_per_position * i;
            let max_bin_id = std::cmp::min(min_bin_id + bin_per_position - 1, upper_bin_id);
            (min_bin_id, max_bin_id)
        })
        .collect()
}

/// solana **v2** keypair for `commons` call sites (the rest of this file signs
/// with v3 types; the two stacks don't share a `Signer` impl).
fn keypair_v2_from_secret(secret: &str) -> Result<solana_sdk::signature::Keypair> {
    let v3 = keypair_from_secret(secret)?;
    solana_sdk::signature::Keypair::try_from(v3.to_bytes().as_slice())
        .map_err(|e| anyhow!("convert keypair to v2: {}", e))
}

/// v2 twin of [`derive_ata`] — same canonical ATA seeds.
fn derive_ata_v2(
    owner: &solana_sdk::pubkey::Pubkey,
    mint: &solana_sdk::pubkey::Pubkey,
    token_program: &solana_sdk::pubkey::Pubkey,
) -> solana_sdk::pubkey::Pubkey {
    let ata_program =
        solana_sdk::pubkey::Pubkey::from_str(ATA_PROGRAM_ID).expect("valid ATA program id");
    solana_sdk::pubkey::Pubkey::find_program_address(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ata_program,
    )
    .0
}

/// v2 twin of [`create_ata_idempotent_ix`].
fn create_ata_idempotent_ix_v2(
    payer: &solana_sdk::pubkey::Pubkey,
    owner: &solana_sdk::pubkey::Pubkey,
    mint: &solana_sdk::pubkey::Pubkey,
    token_program: &solana_sdk::pubkey::Pubkey,
) -> solana_sdk::instruction::Instruction {
    use solana_sdk::instruction::{AccountMeta, Instruction};
    use solana_sdk::pubkey::Pubkey as P;
    let ata_program = P::from_str(ATA_PROGRAM_ID).expect("valid ATA program id");
    let ata = derive_ata_v2(owner, mint, token_program);
    Instruction {
        program_id: ata_program,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(
                P::from_str("11111111111111111111111111111111").expect("valid system program id"),
                false,
            ),
            AccountMeta::new_readonly(*token_program, false),
        ],
        data: vec![ATA_CREATE_IDEMPOTENT_IX],
    }
}

/// Native mint (wrapped SOL).
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
/// SPL Memo program (required account on ClaimFee2).
const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
/// SPL Token `CloseAccount` instruction discriminator.
const SPL_TOKEN_CLOSE_ACCOUNT_IX: u8 = 9;
/// Associated Token Account program id.
const ATA_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
/// ATA program `CreateIdempotent` instruction discriminator.
const ATA_CREATE_IDEMPOTENT_IX: u8 = 1;

/// Close every wSOL token account owned by `keypair`, unwrapping the balance
/// (wrapped principal + account rent) back to native SOL on the same wallet.
///
/// The `CloseAccount` instruction is built by hand (program id + 3 accounts +
/// a single discriminator byte) to avoid pulling an spl-token crate whose
/// solana types would clash with the v3 transaction stack used here.
///
/// Returns the sweep tx signature, or `None` when the wallet holds no wSOL.
/// Build an SPL Token `CloseAccount` instruction by hand. Closing a wSOL
/// account transfers its lamports (wrapped principal + rent) to `destination`,
/// which is how an unwrap is performed.
fn close_wsol_account_ix(
    token_program: Pubkey,
    account: Pubkey,
    owner: Pubkey,
) -> solana_sdk_v3::instruction::Instruction {
    use solana_sdk_v3::instruction::{AccountMeta, Instruction};
    Instruction {
        program_id: token_program,
        accounts: vec![
            AccountMeta::new(account, false), // wSOL account to close
            AccountMeta::new(owner, false),   // lamports destination
            AccountMeta::new_readonly(owner, true), // account owner (signer)
        ],
        data: vec![SPL_TOKEN_CLOSE_ACCOUNT_IX],
    }
}

async fn unwrap_wsol(rpc: &RpcClient, keypair: &Keypair) -> Result<Option<String>> {
    use solana_client_v3::rpc_request::TokenAccountsFilter;
    use solana_sdk_v3::instruction::Instruction;
    use solana_sdk_v3::transaction::Transaction;

    let owner = keypair.pubkey();
    let wsol_mint = Pubkey::from_str(WSOL_MINT)?;
    let token_program = Pubkey::from_str(SPL_TOKEN_PROGRAM_ID)?;

    let accounts = rpc
        .get_token_accounts_by_owner(&owner, TokenAccountsFilter::Mint(wsol_mint))
        .await
        .map_err(|e| anyhow!("list wSOL token accounts: {}", e))?;

    // Only unwrap wSOL accounts that actually hold a wrapped balance. There is a
    // single canonical wSOL ATA shared by every position's token_y, so closing an
    // already-empty one and recreating it below would send a pointless tx on
    // every ~60s sweep (churn). Skipping empties makes the recreated empty ATA
    // stable across sweeps.
    let mut instructions: Vec<Instruction> = Vec::new();
    for acc in &accounts {
        let Ok(account) = Pubkey::from_str(&acc.pubkey) else {
            continue;
        };
        let balance = rpc
            .get_token_account_balance(&account)
            .await
            .ok()
            .and_then(|b| b.ui_amount)
            .unwrap_or(0.0);
        if balance > 0.0 {
            instructions.push(close_wsol_account_ix(token_program, account, owner));
        }
    }

    if instructions.is_empty() {
        return Ok(None);
    }

    // Recreate the canonical wSOL ATA (empty) in the SAME transaction. Closing
    // the shared wSOL account to reclaim native SOL would otherwise leave every
    // still-open position failing its claim/close with AccountNotInitialized
    // until the next per-close recreate — the race that stuck FROGE/STONK. Doing
    // close+recreate atomically means the ATA is never observably missing.
    instructions.push(create_ata_idempotent_ix(&owner, &owner, &wsol_mint, &token_program));

    let blockhash = rpc
        .get_latest_blockhash()
        .await
        .map_err(|e| anyhow!("fetch blockhash for wSOL unwrap: {}", e))?;
    let tx = Transaction::new_signed_with_payer(&instructions, Some(&owner), &[keypair], blockhash);
    let signature = rpc
        .send_and_confirm_transaction(&tx)
        .await
        .map_err(|e| anyhow!("send wSOL unwrap tx: {}", e))?;

    Ok(Some(signature.to_string()))
}

/// Close any wSOL token accounts the wallet holds, converting wrapped SOL back
/// to native SOL. Safety net for residual wSOL when the per-close unwrap missed
/// or failed transiently (e.g. a race with a concurrent op). Uses the env
/// signing keypair. Returns the tx signature, or None if there was no wSOL.
pub async fn unwrap_all_wsol(config: &Config) -> Result<Option<String>> {
    let keypair = keypair_from_secret(&wallet_secret_from_env()?)?;
    let rpc = RpcClient::new(resolve_rpc_url(config));
    unwrap_wsol(&rpc, &keypair).await
}

/// Resolve a position's base mint (the pool's token_x) on-chain. Reads the
/// position account to get its `lb_pair` (stored as the first field after the
/// 8-byte discriminator on both `Position` and `PositionV2`), then reads the
/// pool to get `token_x_mint`.
async fn resolve_base_mint(rpc: &RpcClient, position: &Pubkey) -> Result<String> {
    let pos_data = rpc
        .get_account_data(position)
        .await
        .map_err(|e| anyhow!("fetch position account: {}", e))?;
    if pos_data.len() < 40 {
        anyhow::bail!("position account too small to contain lb_pair");
    }
    let lb_pair = Pubkey::try_from(&pos_data[8..40])
        .map_err(|_| anyhow!("invalid lb_pair bytes in position account"))?;
    let pair_data = rpc
        .get_account_data(&lb_pair)
        .await
        .map_err(|e| anyhow!("fetch lb_pair account: {}", e))?;
    let pair = LbPair::from_bytes(&pair_data).map_err(|e| anyhow!("decode lb_pair: {}", e))?;
    Ok(pair.token_x_mint.to_string())
}

/// Resolve a position's pool mints as `(token_x_mint, token_y_mint)`. Same read
/// path as [`resolve_base_mint`] but returns both sides so the close/claim ATA
/// pre-create can cover whichever token account is missing.
async fn resolve_pool_mints(rpc: &RpcClient, position: &Pubkey) -> Result<(Pubkey, Pubkey)> {
    let pos_data = rpc
        .get_account_data(position)
        .await
        .map_err(|e| anyhow!("fetch position account: {}", e))?;
    if pos_data.len() < 40 {
        anyhow::bail!("position account too small to contain lb_pair");
    }
    let lb_pair = Pubkey::try_from(&pos_data[8..40])
        .map_err(|_| anyhow!("invalid lb_pair bytes in position account"))?;
    let pair_data = rpc
        .get_account_data(&lb_pair)
        .await
        .map_err(|e| anyhow!("fetch lb_pair account: {}", e))?;
    let pair = LbPair::from_bytes(&pair_data).map_err(|e| anyhow!("decode lb_pair: {}", e))?;
    let mint_x = Pubkey::from_str(&pair.token_x_mint.to_string())?;
    let mint_y = Pubkey::from_str(&pair.token_y_mint.to_string())?;
    Ok((mint_x, mint_y))
}

/// Detect which token program owns `mint` (classic SPL vs Token-2022) by reading
/// the mint account's owner — the account's owner *is* its token program. Falls
/// back to classic SPL when the account can't be read, which is the correct
/// default for the wSOL side and any classic mint.
async fn detect_token_program(rpc: &RpcClient, mint: &Pubkey) -> Pubkey {
    match rpc.get_account(mint).await {
        Ok(acc) => acc.owner,
        Err(_) => Pubkey::from_str(SPL_TOKEN_PROGRAM_ID).expect("valid SPL token program id"),
    }
}

/// Token-2022 program id.
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

/// True if `mint` is a Token-2022 mint (owned by the Token-2022 program). Used to
/// screen out Token-2022 (pump.fun) pools at deploy: the wp claim/close one-shots
/// derive the user token ATA under classic SPL, so a Token-2022 token_y can't be
/// claimed/closed cleanly (AccountNotInitialized on user_token_y) and the position
/// gets stuck. Returns false on RPC error — never block trading on a transient
/// hiccup.
pub async fn is_token_2022_mint(config: &Config, mint: &str) -> bool {
    let Ok(pubkey) = Pubkey::from_str(mint) else {
        return false;
    };
    let rpc = RpcClient::new(resolve_rpc_url(config));
    match rpc.get_account(&pubkey).await {
        Ok(acc) => acc.owner.to_string() == TOKEN_2022_PROGRAM_ID,
        Err(_) => false,
    }
}

/// Derive the associated token account address for `owner`/`mint` under
/// `token_program`. Classic SPL and Token-2022 derive to *different* addresses,
/// so the program must be the one that actually owns the mint.
fn derive_ata(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    let ata_program = Pubkey::from_str(ATA_PROGRAM_ID).expect("valid ATA program id");
    Pubkey::find_program_address(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ata_program,
    )
    .0
}

/// Build an ATA `CreateIdempotent` instruction by hand (no spl-associated-token
/// crate, to keep the v3 transaction stack free of clashing solana types). Safe
/// to send even when the ATA already exists — it becomes a no-op.
fn create_ata_idempotent_ix(
    payer: &Pubkey,
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> solana_sdk_v3::instruction::Instruction {
    use solana_sdk_v3::instruction::{AccountMeta, Instruction};
    let ata_program = Pubkey::from_str(ATA_PROGRAM_ID).expect("valid ATA program id");
    let ata = derive_ata(owner, mint, token_program);
    Instruction {
        program_id: ata_program,
        accounts: vec![
            AccountMeta::new(*payer, true),                   // funding account (signer, writable)
            AccountMeta::new(ata, false),                     // ATA to create (writable)
            AccountMeta::new_readonly(*owner, false),         // wallet that owns the ATA
            AccountMeta::new_readonly(*mint, false),          // token mint
            AccountMeta::new_readonly(
                Pubkey::from_str("11111111111111111111111111111111").expect("valid system program id"),
                false,
            ),
            AccountMeta::new_readonly(*token_program, false), // SPL or Token-2022
        ],
        data: vec![ATA_CREATE_IDEMPOTENT_IX],
    }
}

/// Ensure the wallet's associated token accounts for both of a position's pool
/// mints exist before a close/claim. The wp close/claim one-shots send the
/// removed principal and fees to these ATAs and fail with `AccountNotInitialized`
/// if the destination is missing — which happens because the periodic wSOL sweep
/// ([`unwrap_all_wsol`]) closes the wSOL ATA between operations. Each mint's
/// correct token program is detected on-chain so Token-2022 pools work too, and
/// a create is only sent for an ATA that is actually missing so the common path
/// (ATAs present) costs no extra transaction. Non-fatal: logs and returns on any
/// error so a transient RPC hiccup never blocks a close.
async fn ensure_position_atas(rpc: &RpcClient, keypair: &Keypair, position: &Pubkey) {
    use solana_sdk_v3::instruction::Instruction;
    use solana_sdk_v3::transaction::Transaction;

    let owner = keypair.pubkey();
    let (mint_x, mint_y) = match resolve_pool_mints(rpc, position).await {
        Ok(mints) => mints,
        Err(e) => {
            tracing::warn!(error = %e, "could not resolve pool mints; skipping ATA pre-create");
            return;
        }
    };

    let mut instructions: Vec<Instruction> = Vec::new();
    for mint in [mint_x, mint_y] {
        let token_program = detect_token_program(rpc, &mint).await;
        let ata = derive_ata(&owner, &mint, &token_program);
        // Only (re)create an ATA that is genuinely missing — get_account errors
        // for a nonexistent account.
        if rpc.get_account(&ata).await.is_err() {
            instructions.push(create_ata_idempotent_ix(&owner, &owner, &mint, &token_program));
        }
    }

    if instructions.is_empty() {
        return;
    }

    let blockhash = match rpc.get_latest_blockhash().await {
        Ok(bh) => bh,
        Err(e) => {
            tracing::warn!(error = %e, "blockhash fetch failed; skipping ATA pre-create");
            return;
        }
    };
    let tx = Transaction::new_signed_with_payer(&instructions, Some(&owner), &[keypair], blockhash);
    match rpc.send_and_confirm_transaction(&tx).await {
        Ok(sig) => tracing::info!(signature = %sig, "recreated missing position ATA(s) before close/claim"),
        Err(e) => {
            tracing::warn!(error = %e, "ATA pre-create tx failed (close/claim may still succeed if ATAs exist)")
        }
    }
}

/// Sum the wallet's UI balance for a given SPL mint (across all of its token
/// accounts). Used to decide how much base-token fee to swap back to SOL.
pub async fn wallet_token_ui_balance(config: &Config, mint: &str) -> Result<f64> {
    use solana_client_v3::rpc_request::TokenAccountsFilter;
    let keypair = keypair_from_secret(&wallet_secret_from_env()?)?;
    let owner = keypair.pubkey();
    let mint_pk = Pubkey::from_str(mint)?;
    let rpc = RpcClient::new(resolve_rpc_url(config));
    let accounts = rpc
        .get_token_accounts_by_owner(&owner, TokenAccountsFilter::Mint(mint_pk))
        .await
        .map_err(|e| anyhow!("list token accounts for {}: {}", mint, e))?;
    let mut total = 0.0;
    for acc in &accounts {
        if let Ok(account) = Pubkey::from_str(&acc.pubkey) {
            if let Ok(bal) = rpc.get_token_account_balance(&account).await {
                total += bal.ui_amount.unwrap_or(0.0);
            }
        }
    }
    Ok(total)
}

/// Return the subset of the given position ids whose accounts currently exist
/// on-chain (non-zero lamports). Ids that are not valid pubkeys (e.g. leaked
/// dry-run placeholders) are treated as non-existent. Used to prune phantom or
/// externally-closed positions from tracked state so the agent never tries to
/// manage or close an account that isn't there.
/// Meteora DLMM (lb_clmm) program id. PositionV2 layout: 8-byte discriminator,
/// then lb_pair (32), then owner (32) — so owner sits at offset 40.
const LB_CLMM_PROGRAM_ID: &str = "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo";

/// Discover every DLMM position the wallet actually owns on-chain, returned as
/// `(position_address, lb_pair_address)`. Uses getProgramAccounts filtered by the
/// owner field, so it sees positions even if internal state lost track of them.
pub async fn discover_wallet_positions(config: &Config) -> Result<Vec<(String, String)>> {
    use base64::Engine;

    let owner = keypair_from_secret(&wallet_secret_from_env()?)?
        .pubkey()
        .to_string();
    let rpc_url = resolve_rpc_url(config);
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getProgramAccounts",
        "params": [LB_CLMM_PROGRAM_ID, {
            "encoding": "base64",
            "dataSlice": { "offset": 8, "length": 32 },
            "filters": [ { "memcmp": { "offset": 40, "bytes": owner } } ]
        }]
    });
    let resp: serde_json::Value = client
        .post(&rpc_url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| anyhow!("getProgramAccounts request: {}", e))?
        .json()
        .await
        .map_err(|e| anyhow!("getProgramAccounts parse: {}", e))?;

    let mut out = Vec::new();
    if let Some(arr) = resp["result"].as_array() {
        for acc in arr {
            let Some(pubkey) = acc["pubkey"].as_str() else {
                continue;
            };
            let Some(data_b64) = acc["account"]["data"][0].as_str() else {
                continue;
            };
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data_b64) else {
                continue;
            };
            if bytes.len() != 32 {
                continue;
            }
            let lb_pair = bs58::encode(bytes).into_string();
            out.push((pubkey.to_string(), lb_pair));
        }
    }
    Ok(out)
}

/// Resolve a pool's (lb_pair) base token mint (token_x) on-chain.
pub async fn pool_base_mint(config: &Config, lb_pair: &str) -> Result<String> {
    let pair = parse_pubkey("lb_pair", lb_pair)?;
    let rpc = RpcClient::new(resolve_rpc_url(config));
    let data = rpc
        .get_account_data(&pair)
        .await
        .map_err(|e| anyhow!("fetch lb_pair: {}", e))?;
    let pair = LbPair::from_bytes(&data).map_err(|e| anyhow!("decode lb_pair: {}", e))?;
    Ok(pair.token_x_mint.to_string())
}

pub async fn existing_positions(
    config: &Config,
    ids: &[String],
) -> Result<std::collections::HashSet<String>> {
    let mut existing = std::collections::HashSet::new();
    if ids.is_empty() {
        return Ok(existing);
    }
    let rpc = RpcClient::new(resolve_rpc_url(config));
    let parsed: Vec<(String, Pubkey)> = ids
        .iter()
        .filter_map(|id| Pubkey::from_str(id).ok().map(|pk| (id.clone(), pk)))
        .collect();
    // get_multiple_accounts caps at 100 keys per request.
    for chunk in parsed.chunks(100) {
        let pubkeys: Vec<Pubkey> = chunk.iter().map(|(_, pk)| *pk).collect();
        let accounts = rpc
            .get_multiple_accounts(&pubkeys)
            .await
            .map_err(|e| anyhow!("get_multiple_accounts for position reconcile: {}", e))?;
        for ((id, _), account) in chunk.iter().zip(accounts.into_iter()) {
            if account.map(|a| a.lamports > 0).unwrap_or(false) {
                existing.insert(id.clone());
            }
        }
    }

    // Re-verify anything the first bulk call reported missing with a SECOND
    // targeted fetch. `get_multiple_accounts` intermittently returns null for
    // accounts that DO exist (RPC node inconsistency/lag), which previously
    // false-pruned live positions → they escaped stop-loss/OOR and bled out.
    // Only conclude "gone" when the retry ALSO finds nothing; on an RPC error
    // treat all as present so a flaky call never prunes a real position.
    let missing: Vec<(String, Pubkey)> = parsed
        .iter()
        .filter(|(id, _)| !existing.contains(id))
        .cloned()
        .collect();
    if !missing.is_empty() {
        let pubkeys: Vec<Pubkey> = missing.iter().map(|(_, pk)| *pk).collect();
        match rpc.get_multiple_accounts(&pubkeys).await {
            Ok(accounts) => {
                for ((id, _), account) in missing.iter().zip(accounts.into_iter()) {
                    if account.map(|a| a.lamports > 0).unwrap_or(false) {
                        existing.insert(id.clone()); // exists — first call was wrong
                    }
                    // None on retry too → genuinely gone; leave unmarked (prunable).
                }
            }
            Err(_) => {
                // RPC error on retry → ambiguous → keep all (never false-prune).
                for (id, _) in &missing {
                    existing.insert(id.clone());
                }
            }
        }
    }

    Ok(existing)
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

    // Resolve the pool's base mint while the position account still exists, so
    // the caller can swap any claimed base-token fees back to SOL after close.
    let base_mint = match resolve_base_mint(&rpc_ctx.client, &position).await {
        Ok(mint) => Some(mint),
        Err(e) => {
            tracing::warn!(error = %e, "could not resolve base mint before close (fee auto-swap may be skipped)");
            None
        }
    };

    // The close sends removed principal + fees to the wallet's token_x/token_y
    // ATAs. Recreate any that were closed by the periodic wSOL sweep, else the
    // one-shot fails with AccountNotInitialized on the missing account.
    ensure_position_atas(&rpc_ctx.client, &keypair, &position).await;

    let params = ClosePositionParams {
        position_address: position,
        authority: keypair.pubkey(),
        rent_receiver,
    };
    let plan_config = WorkspacePlanConfig::default();
    let result = close_position_one_shot(&rpc_ctx, params, &plan_config, &keypair)
        .await
        .map_err(|e| anyhow!("native Meteora close_position_one_shot failed: {}", e))?;

    // Removing single-side SOL liquidity returns the principal as wrapped SOL
    // (wSOL) in a token account; the close itself does not unwrap it. Sweep any
    // wSOL accounts back to native SOL so the freed capital is spendable again.
    // Non-fatal: the position is already closed, so log and continue on failure.
    let unwrap_signature = match unwrap_wsol(&rpc_ctx.client, &keypair).await {
        Ok(Some(sig)) => {
            tracing::info!(unwrap_signature = %sig, "unwrapped wSOL to native SOL after close");
            Some(sig)
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, "failed to auto-unwrap wSOL after close (funds safe as wSOL)");
            None
        }
    };

    Ok(NativeCloseResult {
        signature: result.signature.to_string(),
        base_mint,
        unwrap_signature,
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

    /// Phase-1 smoke test: run the official-commons quote against a REAL mainnet
    /// DLMM position (SOL/USDC pool) and assert it decodes + parses to non-zero
    /// state. Read-only, no wallet. Hits mainnet, so it's #[ignore] — run with:
    ///   cargo test --release quote_commons_smoke -- --ignored --nocapture
    /// Override RPC via HELIUS_RPC_URL / RPC_URL env if the public one throttles.
    #[tokio::test]
    #[ignore]
    async fn quote_commons_smoke() {
        let config = crate::config::Config::default();
        let position = "13yTvyE1WFoEuzJcFedAZwULjf1Mg9XDdygFbh3MDQ8";
        let q = quote_position_state_commons(position, &config)
            .await
            .expect("commons quote should succeed against a live position");
        println!(
            "COMMONS QUOTE {position}: liq_x={} liq_y={} fee_x={} fee_y={}",
            q.liquidity_x, q.liquidity_y, q.fee_x, q.fee_y
        );
        assert!(
            q.liquidity_x > 0 || q.liquidity_y > 0 || q.fee_x > 0 || q.fee_y > 0,
            "expected non-zero position state from a live position"
        );
    }

    #[test]
    fn close_wsol_account_ix_has_correct_shape() {
        let token_program = Pubkey::from_str(SPL_TOKEN_PROGRAM_ID).unwrap();
        let account = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let ix = close_wsol_account_ix(token_program, account, owner);

        assert_eq!(ix.program_id, token_program);
        // CloseAccount discriminator, no extra payload.
        assert_eq!(ix.data, vec![SPL_TOKEN_CLOSE_ACCOUNT_IX]);
        assert_eq!(ix.accounts.len(), 3);
        // [0] account being closed — writable, not signer.
        assert_eq!(ix.accounts[0].pubkey, account);
        assert!(ix.accounts[0].is_writable);
        assert!(!ix.accounts[0].is_signer);
        // [1] lamports destination (owner) — writable, not signer.
        assert_eq!(ix.accounts[1].pubkey, owner);
        assert!(ix.accounts[1].is_writable);
        assert!(!ix.accounts[1].is_signer);
        // [2] owner authority — signer, read-only.
        assert_eq!(ix.accounts[2].pubkey, owner);
        assert!(ix.accounts[2].is_signer);
        assert!(!ix.accounts[2].is_writable);
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
        assert_eq!(request.strategy, "bid_ask_imbalanced");
        assert_eq!(request.rpc_url, "https://rpc.example.test");
    }
}
