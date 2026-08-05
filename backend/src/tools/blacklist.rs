use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::utils::logger::module;

// ─── Constants ──────────────────────────────────────────────────

const BLACKLIST_FILE: &str = "token-blacklist.json";
const DEV_BLOCKLIST_FILE: &str = "dev-blocklist.json";

// ─── Types ──────────────────────────────────────────────────────

/// Entry stored for each blacklisted mint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistEntry {
    pub symbol: String,
    pub reason: String,
    pub added_at: String,
    pub added_by: String,
}

/// Entry stored for each blocked developer wallet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedDevEntry {
    pub label: String,
    pub reason: String,
    pub added_at: String,
}

/// Unified store managing both token blacklist and dev blocklist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistStore {
    #[serde(default)]
    pub mints: HashMap<String, BlacklistEntry>,
    #[serde(default)]
    pub blocked_devs: HashMap<String, BlockedDevEntry>,
}

/// Compact representation returned by `list_blacklist`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistListResult {
    pub count: usize,
    pub blacklist: Vec<BlacklistItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistItem {
    pub mint: String,
    pub symbol: String,
    pub reason: String,
    pub added_at: String,
}

/// Compact representation returned by `list_blocked_devs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedDevsListResult {
    pub count: usize,
    pub blocked_devs: Vec<BlockedDevItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedDevItem {
    pub wallet: String,
    pub label: String,
    pub reason: String,
    pub added_at: String,
}

// ─── Implementation ─────────────────────────────────────────────

impl BlacklistStore {
    /// Resolve the blacklist file path. Checks `MERIDIAN_DATA_DIR` env var,
    /// then falls back to the current working directory.
    fn data_dir() -> PathBuf {
        std::env::var("MERIDIAN_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    /// Load the combined blacklist store from disk.
    /// If the file doesn't exist, returns an empty store.
    pub fn load() -> Result<Self> {
        let path = Self::data_dir().join(BLACKLIST_FILE);

        if !path.exists() {
            return Ok(Self {
                mints: HashMap::new(),
                blocked_devs: HashMap::new(),
            });
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        let store: BlacklistStore = serde_json::from_str(&content)
            .with_context(|| format!("Invalid {}: {}", path.display(), "JSON parse error"))?;

        Ok(store)
    }

    /// Load only the dev blocklist portion from its separate file.
    pub fn load_dev_blocklist() -> Result<HashMap<String, BlockedDevEntry>> {
        let path = Self::data_dir().join(DEV_BLOCKLIST_FILE);

        if !path.exists() {
            return Ok(HashMap::new());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        let entries: HashMap<String, BlockedDevEntry> = serde_json::from_str(&content)
            .with_context(|| "Invalid dev blocklist: JSON parse error".to_string())?;

        Ok(entries)
    }

    /// Save the combined blacklist store to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::data_dir().join(BLACKLIST_FILE);
        let json =
            serde_json::to_string_pretty(self).context("Failed to serialize blacklist store")?;
        fs::write(&path, &json).with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    /// Save the dev blocklist to its separate file.
    pub fn save_dev_blocklist(&self) -> Result<()> {
        let path = Self::data_dir().join(DEV_BLOCKLIST_FILE);
        let json = serde_json::to_string_pretty(&self.blocked_devs)
            .context("Failed to serialize dev blocklist")?;
        fs::write(&path, &json).with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    // ─── Token blacklist methods ──────────────────────────────

    /// Returns `true` if the mint is on the blacklist.
    /// Used in screening before returning pools to the LLM.
    pub fn is_blacklisted(&self, mint: &str) -> bool {
        if mint.is_empty() {
            return false;
        }
        self.mints.contains_key(mint)
    }

    /// Add a mint to the blacklist.
    pub fn add_to_blacklist(
        &mut self,
        mint: &str,
        symbol: Option<&str>,
        reason: Option<&str>,
    ) -> Result<BlacklistAddResult> {
        if mint.is_empty() {
            return Err(anyhow::anyhow!("mint required"));
        }

        if let Some(existing) = self.mints.get(mint) {
            return Ok(BlacklistAddResult::AlreadyBlacklisted {
                mint: mint.to_string(),
                symbol: existing.symbol.clone(),
                reason: existing.reason.clone(),
            });
        }

        let entry = BlacklistEntry {
            symbol: symbol.unwrap_or("UNKNOWN").to_string(),
            reason: reason.unwrap_or("no reason provided").to_string(),
            added_at: chrono::Utc::now().to_rfc3339(),
            added_by: "agent".to_string(),
        };

        let result_sym = entry.symbol.clone();
        let result_reason = entry.reason.clone();
        self.mints.insert(mint.to_string(), entry);
        self.save()?;

        module::info(
            "blacklist",
            &format!("Blacklisted {} ({}): {}", result_sym, mint, result_reason),
        );

        Ok(BlacklistAddResult::Added {
            mint: mint.to_string(),
            symbol: result_sym,
            reason: result_reason,
        })
    }

    /// Remove a mint from the blacklist.
    pub fn remove_from_blacklist(&mut self, mint: &str) -> Result<BlacklistRemoveResult> {
        if mint.is_empty() {
            return Err(anyhow::anyhow!("mint required"));
        }

        match self.mints.remove(mint) {
            Some(entry) => {
                self.save()?;
                module::info(
                    "blacklist",
                    &format!("Removed {} ({}) from blacklist", entry.symbol, mint),
                );
                Ok(BlacklistRemoveResult::Removed {
                    mint: mint.to_string(),
                    was_symbol: entry.symbol,
                    was_reason: entry.reason,
                })
            }
            None => Err(anyhow::anyhow!("Mint {} not found on blacklist", mint)),
        }
    }

    /// List all blacklisted mints.
    pub fn list_blacklist(&self) -> BlacklistListResult {
        let items: Vec<BlacklistItem> = self
            .mints
            .iter()
            .map(|(mint, entry)| BlacklistItem {
                mint: mint.clone(),
                symbol: entry.symbol.clone(),
                reason: entry.reason.clone(),
                added_at: entry.added_at.clone(),
            })
            .collect();

        BlacklistListResult {
            count: items.len(),
            blacklist: items,
        }
    }

    // ─── Dev blocklist methods ────────────────────────────────

    /// Returns `true` if the dev wallet is on the blocklist.
    pub fn is_dev_blocked(&self, dev_wallet: &str) -> bool {
        if dev_wallet.is_empty() {
            return false;
        }
        self.blocked_devs.contains_key(dev_wallet)
    }

    /// Block a developer wallet address.
    pub fn block_dev(
        &mut self,
        wallet: &str,
        reason: Option<&str>,
        label: Option<&str>,
    ) -> Result<DevBlockResult> {
        if wallet.is_empty() {
            return Err(anyhow::anyhow!("wallet required"));
        }

        if let Some(existing) = self.blocked_devs.get(wallet) {
            return Ok(DevBlockResult::AlreadyBlocked {
                wallet: wallet.to_string(),
                label: existing.label.clone(),
                reason: existing.reason.clone(),
            });
        }

        let entry = BlockedDevEntry {
            label: label.unwrap_or("unknown").to_string(),
            reason: reason.unwrap_or("no reason provided").to_string(),
            added_at: chrono::Utc::now().to_rfc3339(),
        };

        let result_label = entry.label.clone();
        let result_reason = entry.reason.clone();
        self.blocked_devs.insert(wallet.to_string(), entry);
        self.save_dev_blocklist()?;

        module::info(
            "dev_blocklist",
            &format!(
                "Blocked deployer {} ({}): {}",
                result_label, wallet, result_reason
            ),
        );

        Ok(DevBlockResult::Blocked {
            wallet: wallet.to_string(),
            label: result_label,
            reason: result_reason,
        })
    }

    /// Unblock a developer wallet address.
    pub fn unblock_dev(&mut self, wallet: &str) -> Result<DevUnblockResult> {
        if wallet.is_empty() {
            return Err(anyhow::anyhow!("wallet required"));
        }

        match self.blocked_devs.remove(wallet) {
            Some(entry) => {
                self.save_dev_blocklist()?;
                module::info(
                    "dev_blocklist",
                    &format!(
                        "Removed deployer {} ({}) from blocklist",
                        entry.label, wallet
                    ),
                );
                Ok(DevUnblockResult::Unblocked {
                    wallet: wallet.to_string(),
                    was_label: entry.label,
                    was_reason: entry.reason,
                })
            }
            None => Err(anyhow::anyhow!("Wallet {} not on dev blocklist", wallet)),
        }
    }

    /// List all blocked developer wallets.
    pub fn list_blocked_devs(&self) -> BlockedDevsListResult {
        let items: Vec<BlockedDevItem> = self
            .blocked_devs
            .iter()
            .map(|(wallet, entry)| BlockedDevItem {
                wallet: wallet.clone(),
                label: entry.label.clone(),
                reason: entry.reason.clone(),
                added_at: entry.added_at.clone(),
            })
            .collect();

        BlockedDevsListResult {
            count: items.len(),
            blocked_devs: items,
        }
    }
}

// ─── Result types ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BlacklistAddResult {
    #[serde(rename = "added")]
    Added {
        mint: String,
        symbol: String,
        reason: String,
    },
    #[serde(rename = "already_blacklisted")]
    AlreadyBlacklisted {
        mint: String,
        symbol: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BlacklistRemoveResult {
    #[serde(rename = "removed")]
    Removed {
        mint: String,
        was_symbol: String,
        was_reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DevBlockResult {
    #[serde(rename = "blocked")]
    Blocked {
        wallet: String,
        label: String,
        reason: String,
    },
    #[serde(rename = "already_blocked")]
    AlreadyBlocked {
        wallet: String,
        label: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DevUnblockResult {
    #[serde(rename = "unblocked")]
    Unblocked {
        wallet: String,
        was_label: String,
        was_reason: String,
    },
}

// ─── Convenience free functions (backward-compatible API) ───────

/// Check if a mint is on the blacklist (loads from disk each call).
pub fn is_blacklisted(mint: &str) -> bool {
    match BlacklistStore::load() {
        Ok(store) => store.is_blacklisted(mint),
        Err(e) => {
            module::error("blacklist", &format!("Failed to load blacklist: {}", e));
            false
        }
    }
}

/// Add a token to the blacklist.
pub fn add_to_blacklist(
    mint: &str,
    symbol: Option<&str>,
    reason: Option<&str>,
) -> Result<BlacklistAddResult> {
    let mut store = BlacklistStore::load()?;
    store.add_to_blacklist(mint, symbol, reason)
}

/// Remove a token from the blacklist.
pub fn remove_from_blacklist(mint: &str) -> Result<BlacklistRemoveResult> {
    let mut store = BlacklistStore::load()?;
    store.remove_from_blacklist(mint)
}

/// List all blacklisted tokens.
pub fn list_blacklist() -> BlacklistListResult {
    match BlacklistStore::load() {
        Ok(store) => store.list_blacklist(),
        Err(e) => {
            module::error("blacklist", &format!("Failed to load blacklist: {}", e));
            BlacklistListResult {
                count: 0,
                blacklist: vec![],
            }
        }
    }
}

/// Check if a dev wallet is blocked.
pub fn is_dev_blocked(dev_wallet: &str) -> bool {
    match BlacklistStore::load() {
        Ok(store) => store.is_dev_blocked(dev_wallet),
        Err(e) => {
            module::error(
                "dev_blocklist",
                &format!("Failed to load dev blocklist: {}", e),
            );
            false
        }
    }
}

/// Block a developer wallet.
pub fn block_dev(
    wallet: &str,
    reason: Option<&str>,
    label: Option<&str>,
) -> Result<DevBlockResult> {
    let mut store = BlacklistStore::load()?;
    store.block_dev(wallet, reason, label)
}

/// Unblock a developer wallet.
pub fn unblock_dev(wallet: &str) -> Result<DevUnblockResult> {
    let mut store = BlacklistStore::load()?;
    store.unblock_dev(wallet)
}

/// List all blocked developers.
pub fn list_blocked_devs() -> BlockedDevsListResult {
    match BlacklistStore::load() {
        Ok(store) => store.list_blocked_devs(),
        Err(e) => {
            module::error(
                "dev_blocklist",
                &format!("Failed to load dev blocklist: {}", e),
            );
            BlockedDevsListResult {
                count: 0,
                blocked_devs: vec![],
            }
        }
    }
}
