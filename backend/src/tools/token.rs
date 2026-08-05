use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::utils::logger::module;

// ─── Constants ──────────────────────────────────────────────────

const DATAPI_BASE: &str = "https://datapi.jup.ag/v1";
const AGENT_MERIDIAN_BASE: &str = "https://api.agentmeridian.xyz/api";

// ─── API response types ─────────────────────────────────────────

/// Raw response shape from Jupiter datapi `/assets/search`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupiterSearchResult {
    id: String,
    name: Option<String>,
    symbol: Option<String>,
    mcap: Option<f64>,
    #[serde(rename = "usdPrice")]
    usd_price: Option<f64>,
    liquidity: Option<f64>,
    holder_count: Option<u64>,
    organic_score: Option<f64>,
    organic_score_label: Option<String>,
    launchpad: Option<String>,
    graduated_pool: Option<String>,
    fees: Option<f64>,
    total_supply: Option<f64>,
    circ_supply: Option<f64>,
    #[serde(rename = "audit")]
    audit: Option<JupiterAudit>,
    #[serde(rename = "stats1h")]
    stats_1h: Option<JupiterStats1h>,
    #[serde(rename = "stats24h")]
    stats_24h: Option<JupiterStats24h>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupiterAudit {
    mint_authority_disabled: Option<bool>,
    freeze_authority_disabled: Option<bool>,
    top_holders_percentage: Option<f64>,
    bot_holders_percentage: Option<f64>,
    dev_migrations: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupiterStats1h {
    price_change: Option<f64>,
    buy_volume: Option<f64>,
    sell_volume: Option<f64>,
    num_organic_buyers: Option<u32>,
    num_net_buyers: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupiterStats24h {
    num_net_buyers: Option<i32>,
}

/// Normalised token info returned by `get_token_info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfoSearchResult {
    pub found: bool,
    pub query: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub results: Vec<TokenInfo>,
}

/// Normalised token info returned by `get_token_info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub mint: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub mcap: Option<f64>,
    pub price: Option<f64>,
    pub liquidity: Option<f64>,
    pub holders: Option<u64>,
    pub organic_score: Option<f64>,
    pub organic_label: Option<String>,
    pub launchpad: Option<String>,
    pub graduated: bool,
    pub global_fees_sol: Option<f64>,
    pub audit: Option<TokenAudit>,
    pub stats_1h: Option<TokenStats1h>,
    pub stats_24h_net_buyers: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenAudit {
    pub mint_disabled: Option<bool>,
    pub freeze_disabled: Option<bool>,
    pub top_holders_pct: Option<f64>,
    pub bot_holders_pct: Option<f64>,
    pub dev_migrations: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenStats1h {
    pub price_change: Option<f64>,
    pub buy_vol: Option<f64>,
    pub sell_vol: Option<f64>,
    pub buyers: Option<u32>,
    pub net_buyers: Option<i32>,
}

/// Normalised holder entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHolderFunding {
    pub address: String,
    pub amount: Option<String>,
    pub slot: Option<u64>,
}

/// Normalised holder entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHolder {
    pub address: String,
    pub amount: Option<String>,
    pub pct: Option<f64>,
    pub sol_balance: Option<String>,
    pub tags: Option<Vec<String>>,
    pub is_pool: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub funding: Option<TokenHolderFunding>,
}

/// Response from the ChainInsight narrative endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenNarrative {
    pub mint: String,
    pub narrative: Option<String>,
    pub status: Option<String>,
}

/// Full holder-analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartWalletTokenHolding {
    pub name: String,
    pub category: Option<String>,
    pub address: String,
    pub pct: Option<f64>,
    pub sol_balance: Option<String>,
    pub pnl: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct TokenHolderMeta {
    pub total_supply: Option<f64>,
    pub global_fees_sol: Option<f64>,
}

/// Full holder-analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHoldersResult {
    pub mint: String,
    pub global_fees_sol: Option<f64>,
    pub total_fetched: usize,
    pub showing: usize,
    pub top_10_real_holders_pct: f64,
    pub smart_wallets_holding: Vec<SmartWalletTokenHolding>,
    pub holders: Vec<TokenHolder>,
}

/// Smart-wallet check result for a pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartWalletPoolCheck {
    pub pool: String,
    pub tracked_wallets: usize,
    pub in_pool: Vec<String>,
    pub confidence_boost: bool,
    pub signal: String,
}

// ─── Functions ──────────────────────────────────────────────────

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn token_info_from_jupiter(first: JupiterSearchResult) -> TokenInfo {
    let audit = first.audit.map(|a| TokenAudit {
        mint_disabled: a.mint_authority_disabled,
        freeze_disabled: a.freeze_authority_disabled,
        top_holders_pct: a.top_holders_percentage,
        bot_holders_pct: a.bot_holders_percentage,
        dev_migrations: a.dev_migrations,
    });

    let stats_1h = first.stats_1h.map(|s| TokenStats1h {
        price_change: s.price_change,
        buy_vol: s.buy_volume,
        sell_vol: s.sell_volume,
        buyers: s.num_organic_buyers,
        net_buyers: s.num_net_buyers,
    });

    let global_fees = first.fees.map(round2);

    TokenInfo {
        mint: first.id,
        name: first.name,
        symbol: first.symbol,
        mcap: first.mcap,
        price: first.usd_price,
        liquidity: first.liquidity,
        holders: first.holder_count,
        organic_score: first.organic_score,
        organic_label: first.organic_score_label,
        launchpad: first.launchpad,
        graduated: first.graduated_pool.is_some(),
        global_fees_sol: global_fees,
        audit,
        stats_1h,
        stats_24h_net_buyers: first.stats_24h.and_then(|s| s.num_net_buyers),
    }
}

fn token_info_search_result_from_jupiter(
    query: &str,
    data: Vec<JupiterSearchResult>,
) -> TokenInfoSearchResult {
    let results: Vec<TokenInfo> = data
        .into_iter()
        .take(5)
        .map(token_info_from_jupiter)
        .collect();
    TokenInfoSearchResult {
        found: !results.is_empty(),
        query: query.to_string(),
        results,
    }
}

/// Search Jupiter datapi for a mint address (or symbol/name) and return
/// the JS-compatible search envelope.
pub async fn search_token_info(query: &str) -> Result<TokenInfoSearchResult> {
    let client = Client::new();
    let url = format!("{}/assets/search?query={}", DATAPI_BASE, query);

    let resp = client
        .get(&url)
        .send()
        .await
        .context("Token search API request failed")?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("Token search API error: {}", resp.status()));
    }

    let data: Vec<JupiterSearchResult> = resp
        .json()
        .await
        .context("Failed to parse token search response")?;
    Ok(token_info_search_result_from_jupiter(query, data))
}

/// Search Jupiter datapi for a mint address (or symbol/name) and return
/// condensed token info useful for confidence scoring.
pub async fn get_token_info(mint: &str) -> Result<TokenInfo> {
    let search = search_token_info(mint).await?;
    let info = search
        .results
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Token not found for query: {}", mint))?;

    module::info(
        "token",
        &format!(
            "get_token_info: {} ({})",
            info.symbol.as_deref().unwrap_or("?"),
            &info.mint[..8.min(info.mint.len())]
        ),
    );
    Ok(info)
}

/// Fetch top holders for a token mint.
///
/// * `limit` – maximum holders to return (clamped to 100).
pub async fn get_token_holders(mint: &str, limit: usize) -> Result<TokenHoldersResult> {
    let client = Client::new();
    let limit = limit.min(100);

    // Fetch holders + token info in parallel via tokio::join
    let (holders_url, search_url) = (
        format!("{}/holders/{}?limit=100", DATAPI_BASE, mint),
        format!("{}/assets/search?query={}", DATAPI_BASE, mint),
    );

    let (holders_resp, token_resp) = tokio::join!(
        client.get(&holders_url).send(),
        client.get(&search_url).send(),
    );

    let holders_resp = holders_resp.context("Holders API request failed")?;
    if !holders_resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "Holders API error: {}",
            holders_resp.status()
        ));
    }

    // Token info is optional (for total supply / fees)
    let token_meta = if let Ok(resp) = token_resp {
        if resp.status().is_success() {
            let arr: Vec<JupiterSearchResult> = resp.json().await.unwrap_or_default();
            let token = arr.first();
            TokenHolderMeta {
                total_supply: token.and_then(|t| t.total_supply.or(t.circ_supply)),
                global_fees_sol: token.and_then(|t| t.fees.map(round2)),
            }
        } else {
            TokenHolderMeta {
                total_supply: None,
                global_fees_sol: None,
            }
        }
    } else {
        TokenHolderMeta {
            total_supply: None,
            global_fees_sol: None,
        }
    };

    // The holders response can be an array or an object with `holders`/`data`
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum HoldersBody {
        List(Vec<HolderRaw>),
        Wrapper { holders: Vec<HolderRaw> },
        WrapperData { data: Vec<HolderRaw> },
        Empty,
    }

    let holders_raw: HoldersBody = holders_resp.json().await.unwrap_or(HoldersBody::Empty);
    let raw_list = match holders_raw {
        HoldersBody::List(v) => v,
        HoldersBody::Wrapper { holders: v } => v,
        HoldersBody::WrapperData { data: v } => v,
        HoldersBody::Empty => vec![],
    };

    let result = token_holders_result_from_raw(mint, raw_list, token_meta, limit, vec![]);

    module::info(
        "token",
        &format!(
            "get_token_holders: {} — fetched {} showing {}",
            mint, result.total_fetched, result.showing
        ),
    );

    Ok(result)
}

fn token_holders_result_from_raw(
    mint: &str,
    raw_list: Vec<HolderRaw>,
    token_meta: TokenHolderMeta,
    limit: usize,
    smart_wallets_holding: Vec<SmartWalletTokenHolding>,
) -> TokenHoldersResult {
    let pool_re = regex::Regex::new(r"(?i)pool|amm|liquidity|raydium|orca|meteora").unwrap();

    let total_fetched = raw_list.len();
    let mapped: Vec<TokenHolder> = raw_list
        .into_iter()
        .take(limit.min(100))
        .map(|h| {
            let tags: Vec<String> = h
                .tags
                .unwrap_or_default()
                .into_iter()
                .map(|t| t.name.unwrap_or(t.id.unwrap_or_default()))
                .collect();

            let is_pool = tags.iter().any(|t| pool_re.is_match(t));

            let pct = if let Some(ts) = token_meta.total_supply {
                h.amount
                    .as_deref()
                    .and_then(|a| a.parse::<f64>().ok())
                    .map(|amount| round4((amount / ts) * 100.0))
            } else {
                h.percentage.or(h.pct).map(round4)
            };

            let funding = h.address_info.and_then(|info| {
                info.funding_address.map(|address| TokenHolderFunding {
                    address,
                    amount: info.funding_amount,
                    slot: info.funding_slot,
                })
            });

            TokenHolder {
                address: h.address.or(h.wallet).unwrap_or_default(),
                amount: h.amount,
                pct,
                sol_balance: h.sol_balance_display.or(h.sol_balance),
                tags: if tags.is_empty() { None } else { Some(tags) },
                is_pool,
                funding,
            }
        })
        .collect();

    let real_holders: Vec<&TokenHolder> = mapped.iter().filter(|h| !h.is_pool).collect();
    let top10_pct: f64 = real_holders
        .iter()
        .take(10)
        .map(|h| h.pct.unwrap_or(0.0))
        .sum();

    TokenHoldersResult {
        mint: mint.to_string(),
        global_fees_sol: token_meta.global_fees_sol.map(round2),
        total_fetched,
        showing: mapped.len(),
        top_10_real_holders_pct: round2(top10_pct),
        smart_wallets_holding,
        holders: mapped,
    }
}

/// Get the narrative/story behind a token from Jupiter ChainInsight.
pub async fn get_token_narrative(mint: &str) -> Result<TokenNarrative> {
    let client = Client::new();
    let url = format!("{}/chaininsight/narrative/{}", DATAPI_BASE, mint);

    let resp = client
        .get(&url)
        .send()
        .await
        .context("Narrative API request failed")?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("Narrative API error: {}", resp.status()));
    }

    #[derive(Deserialize)]
    struct NarrativeRaw {
        narrative: Option<String>,
        status: Option<String>,
    }

    let raw: NarrativeRaw = resp
        .json()
        .await
        .context("Failed to parse narrative response")?;

    let result = TokenNarrative {
        mint: mint.to_string(),
        narrative: raw.narrative,
        status: raw.status,
    };

    module::info(
        "token",
        &format!(
            "get_token_narrative: {} — has_narrative={}",
            mint,
            result.narrative.is_some()
        ),
    );

    Ok(result)
}

/// Check if any tracked smart wallets are LPing in the given pool.
///
/// This delegates to the smart-wallets module's `check_on_pool` function.
/// The pool_address is expected to be a valid Solana address string.
pub async fn check_smart_wallets_on_pool(pool_address: &str) -> Result<SmartWalletPoolCheck> {
    let store = super::smart_wallets::SmartWalletStore::load()?;
    let wallets = store.wallets;

    // Filter to LP-type wallets (those without a type or with type == "lp")
    let lp_wallets: Vec<&super::smart_wallets::SmartWallet> = wallets
        .iter()
        .filter(|w| w.wallet_type.as_deref() != Some("holder"))
        .collect();

    if lp_wallets.is_empty() {
        return Ok(SmartWalletPoolCheck {
            pool: pool_address.to_string(),
            tracked_wallets: 0,
            in_pool: vec![],
            confidence_boost: false,
            signal: "No smart wallets tracked yet — neutral signal".to_string(),
        });
    }

    // Check each wallet's positions via Helius RPC
    let rpc_url = std::env::var("HELIUS_RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());

    let client = Client::new();
    let mut in_pool: Vec<String> = Vec::new();

    for wallet in &lp_wallets {
        match fetch_wallet_positions(&client, &rpc_url, &wallet.address).await {
            Ok(positions) => {
                if positions.iter().any(|p| p == pool_address) {
                    in_pool.push(wallet.label.clone());
                }
            }
            Err(e) => {
                module::warn(
                    "token",
                    &format!(
                        "check_smart_wallets: failed to query wallet {}: {}",
                        &wallet.address[..8],
                        e
                    ),
                );
            }
        }
    }

    let confidence_boost = !in_pool.is_empty();
    let signal = if confidence_boost {
        format!(
            "{}/{} smart wallet(s) are in this pool: {} — STRONG signal",
            in_pool.len(),
            lp_wallets.len(),
            in_pool.join(", ")
        )
    } else {
        format!(
            "0/{} smart wallets in this pool — neutral, rely on fundamentals",
            lp_wallets.len()
        )
    };

    module::info(
        "token",
        &format!(
            "check_smart_wallets_on_pool: {} — {} in pool",
            pool_address,
            in_pool.len()
        ),
    );

    Ok(SmartWalletPoolCheck {
        pool: pool_address.to_string(),
        tracked_wallets: lp_wallets.len(),
        in_pool,
        confidence_boost,
        signal,
    })
}

// ─── Internal helpers ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HolderRaw {
    address: Option<String>,
    wallet: Option<String>,
    amount: Option<String>,
    percentage: Option<f64>,
    pct: Option<f64>,
    sol_balance_display: Option<String>,
    sol_balance: Option<String>,
    address_info: Option<HolderAddressInfo>,
    #[serde(default)]
    tags: Option<Vec<HolderTag>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HolderAddressInfo {
    funding_address: Option<String>,
    funding_amount: Option<String>,
    funding_slot: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct HolderTag {
    name: Option<String>,
    id: Option<String>,
}

/// Fetch positions for a wallet by calling the Helius DAS (getAssetsByOwner).
/// Returns a list of pool addresses the wallet has LP positions in.
async fn fetch_wallet_positions(
    client: &Client,
    rpc_url: &str,
    wallet_address: &str,
) -> Result<Vec<String>> {
    use serde_json::json;

    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getAssetsByOwner",
        "params": {
            "ownerAddress": wallet_address,
            "page": 1,
            "limit": 1000,
            "options": {
                "showFungible": true
            }
        }
    });

    let resp = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .context("RPC request failed")?;

    #[derive(Deserialize)]
    struct RpcResponse {
        result: Option<RpcResult>,
        error: Option<serde_json::Value>,
    }

    #[derive(Deserialize)]
    struct RpcResult {
        items: Vec<RpcAsset>,
    }

    #[derive(Deserialize)]
    struct RpcAsset {
        interface: Option<String>,
        content: Option<RpcContent>,
    }

    #[derive(Deserialize)]
    struct RpcContent {
        metadata: Option<RpcMetadata>,
    }

    #[derive(Deserialize)]
    struct RpcMetadata {
        name: Option<String>,
    }

    let rpc: RpcResponse = resp.json().await.context("Failed to parse RPC response")?;

    if let Some(err) = rpc.error {
        return Err(anyhow::anyhow!("RPC error: {}", err));
    }

    // This is a simplified heuristic — in production you'd decode the DLMM
    // position account data to find pool addresses. For now we look for
    // assets whose name contains the pool address pattern.
    let mut pool_addresses = Vec::new();
    if let Some(result) = rpc.result {
        for item in result.items {
            if let Some(content) = item.content {
                if let Some(meta) = content.metadata {
                    if let Some(name) = meta.name {
                        // DLMM positions typically have pool addresses in their names
                        // This is a heuristic — real implementation would decode account data
                        pool_addresses.push(name);
                    }
                }
            }
        }
    }

    Ok(pool_addresses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn token_search_result_preserves_audit_stats_and_global_fees() {
        let raw: Vec<JupiterSearchResult> = serde_json::from_value(json!([
            {
                "id": "MintMoon",
                "name": "Moon Token",
                "symbol": "MOON",
                "mcap": 123456,
                "usdPrice": 0.0123,
                "liquidity": 9999,
                "holderCount": 777,
                "organicScore": 88,
                "organicScoreLabel": "high",
                "launchpad": "meteora",
                "graduatedPool": "PoolMoon",
                "fees": 12.3456,
                "audit": {
                    "mintAuthorityDisabled": true,
                    "freezeAuthorityDisabled": true,
                    "topHoldersPercentage": 42.4242,
                    "botHoldersPercentage": 3.2121,
                    "devMigrations": 2
                },
                "stats1h": {
                    "priceChange": 4.567,
                    "buyVolume": 1111.1,
                    "sellVolume": 222.2,
                    "numOrganicBuyers": 44,
                    "numNetBuyers": 11
                },
                "stats24h": { "numNetBuyers": 99 }
            }
        ]))
        .expect("fixture parses");

        let result = token_info_search_result_from_jupiter("MOON", raw);

        assert!(result.found);
        assert_eq!(result.query, "MOON");
        assert_eq!(result.results.len(), 1);
        let token = &result.results[0];
        assert_eq!(token.mint, "MintMoon");
        assert_eq!(token.global_fees_sol, Some(12.35));
        assert!(token.graduated);
        assert_eq!(token.audit.as_ref().unwrap().top_holders_pct, Some(42.4242));
        assert_eq!(token.stats_1h.as_ref().unwrap().net_buyers, Some(11));
        assert_eq!(token.stats_24h_net_buyers, Some(99));
    }

    #[test]
    fn holder_mapping_uses_wallet_fallback_funding_fees_and_smart_wallets() {
        let holders: Vec<HolderRaw> = serde_json::from_value(json!([
            {
                "wallet": "SmartWallet111",
                "amount": "100",
                "solBalanceDisplay": "1.5 SOL",
                "addressInfo": {
                    "fundingAddress": "Funder111",
                    "fundingAmount": "2.0",
                    "fundingSlot": 123
                },
                "tags": [{ "name": "KOL" }]
            },
            {
                "address": "PoolHolder111",
                "amount": "500",
                "solBalance": "9.9",
                "tags": [{ "name": "Meteora Pool" }]
            }
        ]))
        .expect("holder fixture parses");
        let smart_wallets = vec![SmartWalletTokenHolding {
            name: "Sharp LP".to_string(),
            category: Some("kol".to_string()),
            address: "SmartWallet111".to_string(),
            pct: Some(10.0),
            sol_balance: Some("1.5 SOL".to_string()),
            pnl: Some(json!({"total_pnl": 42})),
        }];

        let result = token_holders_result_from_raw(
            "MintMoon",
            holders,
            TokenHolderMeta {
                total_supply: Some(1_000.0),
                global_fees_sol: Some(44.444),
            },
            20,
            smart_wallets,
        );

        assert_eq!(result.global_fees_sol, Some(44.44));
        assert_eq!(result.total_fetched, 2);
        assert_eq!(result.showing, 2);
        assert_eq!(result.top_10_real_holders_pct, 10.0);
        assert_eq!(result.holders[0].address, "SmartWallet111");
        assert_eq!(result.holders[0].pct, Some(10.0));
        assert_eq!(
            result.holders[0].funding.as_ref().unwrap().address,
            "Funder111"
        );
        assert!(result.holders[1].is_pool);
        assert_eq!(result.smart_wallets_holding.len(), 1);
        assert_eq!(result.smart_wallets_holding[0].name, "Sharp LP");
    }
}
