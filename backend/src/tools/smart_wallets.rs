use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::utils::logger::module;

// ─── Constants ──────────────────────────────────────────────────

const WALLETS_FILE: &str = "smart-wallets.json";
const CACHE_TTL_SECS: u64 = 300; // 5 minutes

// ─── Types ──────────────────────────────────────────────────────

/// A tracked smart wallet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartWallet {
    pub address: String,
    pub label: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(rename = "type", default)]
    pub wallet_type: Option<String>,
    #[serde(default = "default_added_at")]
    pub added_at: String,
}

fn default_added_at() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Persistence container for smart wallets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartWalletStore {
    #[serde(default)]
    pub wallets: Vec<SmartWallet>,
}

/// Result of adding a wallet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddWalletResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet: Option<SmartWallet>,
}

/// Result of removing a wallet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveWalletResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<String>,
}

/// Result of listing wallets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListWalletsResult {
    pub total: usize,
    pub wallets: Vec<SmartWallet>,
}

/// A smart wallet that is LPing in a given pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolLpMatch {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub address: String,
}

/// Result of checking tracked wallets against a pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolCheckResult {
    pub pool: String,
    pub tracked_wallets: usize,
    pub in_pool: Vec<PoolLpMatch>,
    pub confidence_boost: bool,
    pub signal: String,
}

// ─── Position cache (global, 5-min TTL) ─────────────────────────

struct PositionCacheEntry {
    positions: Vec<String>,
    fetched_at: Instant,
}

fn position_cache() -> &'static Mutex<HashMap<String, PositionCacheEntry>> {
    static INSTANCE: OnceLock<Mutex<HashMap<String, PositionCacheEntry>>> = OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(HashMap::new()))
}

// ─── Implementation ─────────────────────────────────────────────

impl SmartWalletStore {
    /// Resolve the data directory for persistence files.
    fn data_dir() -> PathBuf {
        std::env::var("MERIDIAN_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    /// Validate a Solana address format (base58, 32-44 chars).
    pub fn validate_address(address: &str) -> bool {
        let re = regex::Regex::new(r"^[1-9A-HJ-NP-Za-km-z]{32,44}$").unwrap();
        re.is_match(address)
    }

    /// Load the wallet store from disk.
    pub fn load() -> Result<Self> {
        let path = Self::data_dir().join(WALLETS_FILE);

        if !path.exists() {
            return Ok(Self { wallets: vec![] });
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        let store: SmartWalletStore = serde_json::from_str(&content)
            .with_context(|| format!("Invalid {}: JSON parse error", path.display()))?;

        Ok(store)
    }

    /// Save the wallet store to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::data_dir().join(WALLETS_FILE);
        let json =
            serde_json::to_string_pretty(self).context("Failed to serialize wallet store")?;
        fs::write(&path, &json).with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    /// Add a new smart wallet to track.
    pub fn add_wallet(
        &mut self,
        address: &str,
        label: &str,
        category: Option<&str>,
        wallet_type: Option<&str>,
    ) -> Result<AddWalletResult> {
        if !Self::validate_address(address) {
            return Ok(AddWalletResult {
                success: false,
                error: Some("Invalid Solana address format".to_string()),
                wallet: None,
            });
        }

        if let Some(existing) = self.wallets.iter().find(|w| w.address == address) {
            return Ok(AddWalletResult {
                success: false,
                error: Some(format!("Already tracked as \"{}\"", existing.label)),
                wallet: None,
            });
        }

        let wallet = SmartWallet {
            address: address.to_string(),
            label: label.to_string(),
            category: category.map(String::from),
            wallet_type: wallet_type.map(String::from),
            added_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = wallet.clone();
        self.wallets.push(wallet);
        self.save()?;

        module::info(
            "smart_wallets",
            &format!(
                "Added wallet: {} ({}, type={})",
                result.label,
                result.category.as_deref().unwrap_or("alpha"),
                result.wallet_type.as_deref().unwrap_or("lp")
            ),
        );

        Ok(AddWalletResult {
            success: true,
            error: None,
            wallet: Some(result),
        })
    }

    /// Remove a wallet from tracking by address.
    pub fn remove_wallet(&mut self, address: &str) -> Result<RemoveWalletResult> {
        let idx = self.wallets.iter().position(|w| w.address == address);
        match idx {
            Some(i) => {
                let removed = self.wallets.remove(i);
                self.save()?;
                module::info(
                    "smart_wallets",
                    &format!("Removed wallet: {}", removed.label),
                );
                Ok(RemoveWalletResult {
                    success: true,
                    error: None,
                    removed: Some(removed.label),
                })
            }
            None => Ok(RemoveWalletResult {
                success: false,
                error: Some("Wallet not found".to_string()),
                removed: None,
            }),
        }
    }

    /// List all tracked wallets.
    pub fn list_wallets(&self) -> ListWalletsResult {
        ListWalletsResult {
            total: self.wallets.len(),
            wallets: self.wallets.clone(),
        }
    }

    /// Check if any tracked wallets are LPing in the given pool.
    ///
    /// Only LP-type wallets are checked (wallets without a type or with
    /// type == "lp"). Uses a 5-minute cache per wallet to avoid
    /// hammering RPC.
    pub async fn check_on_pool(
        &self,
        pool_address: &str,
        rpc_url: &str,
    ) -> Result<PoolCheckResult> {
        let lp_wallets: Vec<&SmartWallet> = self
            .wallets
            .iter()
            .filter(|w| w.wallet_type.as_deref() != Some("holder"))
            .collect();

        if lp_wallets.is_empty() {
            return Ok(PoolCheckResult {
                pool: pool_address.to_string(),
                tracked_wallets: 0,
                in_pool: vec![],
                confidence_boost: false,
                signal: "No smart wallets tracked yet — neutral signal".to_string(),
            });
        }

        let client = Client::new();
        let mut in_pool: Vec<PoolLpMatch> = Vec::new();

        for wallet in &lp_wallets {
            let positions = fetch_positions_cached(&client, rpc_url, &wallet.address).await;
            if positions.iter().any(|p| p == pool_address) {
                in_pool.push(PoolLpMatch {
                    name: wallet.label.clone(),
                    category: wallet.category.clone(),
                    address: wallet.address.clone(),
                });
            }
        }

        let confidence_boost = !in_pool.is_empty();
        let signal = if confidence_boost {
            let names: Vec<&str> = in_pool.iter().map(|w| w.name.as_str()).collect();
            format!(
                "{}/{} smart wallet(s) are in this pool: {} — STRONG signal",
                in_pool.len(),
                lp_wallets.len(),
                names.join(", ")
            )
        } else {
            format!(
                "0/{} smart wallets in this pool — neutral, rely on fundamentals",
                lp_wallets.len()
            )
        };

        module::info(
            "smart_wallets",
            &format!(
                "check_on_pool: {} — {}/{} in pool",
                pool_address,
                in_pool.len(),
                lp_wallets.len()
            ),
        );

        Ok(PoolCheckResult {
            pool: pool_address.to_string(),
            tracked_wallets: lp_wallets.len(),
            in_pool,
            confidence_boost,
            signal,
        })
    }
}

// ─── RPC helpers ────────────────────────────────────────────────

/// Fetch wallet positions with 5-minute caching.
async fn fetch_positions_cached(
    client: &Client,
    rpc_url: &str,
    wallet_address: &str,
) -> Vec<String> {
    // Check cache first
    {
        let cache = position_cache().lock().unwrap();
        if let Some(entry) = cache.get(wallet_address) {
            if entry.fetched_at.elapsed() < Duration::from_secs(CACHE_TTL_SECS) {
                return entry.positions.clone();
            }
        }
    }

    // Fetch from RPC
    let positions = match fetch_wallet_positions(client, rpc_url, wallet_address).await {
        Ok(pos) => pos,
        Err(e) => {
            module::warn(
                "smart_wallets",
                &format!(
                    "Failed to fetch positions for {}: {}",
                    &wallet_address[..8.min(wallet_address.len())],
                    e
                ),
            );
            vec![]
        }
    };

    // Update cache
    {
        let mut cache = position_cache().lock().unwrap();
        cache.insert(
            wallet_address.to_string(),
            PositionCacheEntry {
                positions: positions.clone(),
                fetched_at: Instant::now(),
            },
        );
    }

    positions
}

/// Fetch LP positions for a wallet via Helius RPC.
///
/// Returns a list of pool addresses the wallet has positions in.
async fn fetch_wallet_positions(
    client: &Client,
    rpc_url: &str,
    wallet_address: &str,
) -> Result<Vec<String>> {
    use serde_json::json;

    // Helius-enhanced RPC: getAssetsByOwner
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

    // In a production build, decode the DLMM position account data to
    // extract pool addresses. Here we use a heuristic: DLMM position
    // accounts store the pool address in their account metadata.
    let mut pool_addresses = Vec::new();
    if let Some(result) = rpc.result {
        for item in result.items {
            if let Some(content) = item.content {
                if let Some(meta) = content.metadata {
                    if let Some(name) = meta.name {
                        pool_addresses.push(name);
                    }
                }
            }
        }
    }

    Ok(pool_addresses)
}
