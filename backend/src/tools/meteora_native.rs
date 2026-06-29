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
    Ok(PositionQuote {
        liquidity_x: plan.quote.remove_liquidity_amount_x,
        liquidity_y: plan.quote.remove_liquidity_amount_y,
        fee_x: plan.quote.claimable_fee_x,
        fee_y: plan.quote.claimable_fee_y,
    })
}

/// SPL Token program id.
const SPL_TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
/// Token-2022 (Token Extensions) program id.
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
/// Native mint (wrapped SOL).
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
/// SPL Token `CloseAccount` instruction discriminator.
const SPL_TOKEN_CLOSE_ACCOUNT_IX: u8 = 9;

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

    let instructions: Vec<Instruction> = accounts
        .iter()
        .filter_map(|acc| Pubkey::from_str(&acc.pubkey).ok())
        .map(|account| close_wsol_account_ix(token_program, account, owner))
        .collect();

    if instructions.is_empty() {
        return Ok(None);
    }

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

/// Whether `mint` is owned by the Token-2022 program (vs the legacy SPL Token
/// program). The deploy SDK (`add_liquidity_one_shot`) only builds legacy-token
/// instructions, so Token-2022 mints fail on-chain with "incorrect program id"
/// and must be skipped at pre-flight.
pub async fn is_token_2022(config: &Config, mint: &str) -> Result<bool> {
    let mint_pk = Pubkey::from_str(mint)?;
    let rpc = RpcClient::new(resolve_rpc_url(config));
    let account = rpc
        .get_account(&mint_pk)
        .await
        .map_err(|e| anyhow!("fetch mint account {}: {}", mint, e))?;
    Ok(account.owner.to_string() == TOKEN_2022_PROGRAM_ID)
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
