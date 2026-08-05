use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::meridian_data_path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrategyEntry {
    pub id: String,
    pub name: String,
    #[serde(default = "default_author")]
    pub author: String,
    #[serde(default = "default_lp_strategy")]
    pub lp_strategy: String,
    #[serde(default)]
    pub token_criteria: Value,
    #[serde(default)]
    pub entry: Value,
    #[serde(default)]
    pub range: Value,
    #[serde(default)]
    pub exit: Value,
    #[serde(default)]
    pub best_for: String,
    #[serde(default)]
    pub raw: String,
    #[serde(default)]
    pub added_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrategyLibraryStore {
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub strategies: BTreeMap<String, StrategyEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrategyInput {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub lp_strategy: Option<String>,
    #[serde(default)]
    pub token_criteria: Value,
    #[serde(default)]
    pub entry: Value,
    #[serde(default)]
    pub range: Value,
    #[serde(default)]
    pub exit: Value,
    #[serde(default)]
    pub best_for: Option<String>,
    #[serde(default)]
    pub raw: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrategySaveResult {
    pub saved: bool,
    pub id: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrategySummary {
    pub id: String,
    pub name: String,
    pub author: String,
    pub lp_strategy: String,
    pub best_for: String,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrategyListResult {
    pub active: Option<String>,
    pub count: usize,
    pub strategies: Vec<StrategySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveStrategyResult {
    pub active: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemovedStrategyResult {
    pub removed: bool,
    pub id: String,
    pub name: String,
    pub new_active: Option<String>,
}

impl Default for StrategyLibraryStore {
    fn default() -> Self {
        let mut store = Self {
            active: Some("custom_ratio_spot".to_string()),
            strategies: BTreeMap::new(),
        };
        store.ensure_default_strategies();
        store
    }
}

impl StrategyEntry {
    pub fn prompt_summary(&self) -> String {
        format!(
            "STRATEGY CONTEXT: {} — entry: {} | exit: {} | best for: {}",
            self.name,
            object_string(&self.entry, "condition").unwrap_or("n/a"),
            object_string(&self.exit, "notes").unwrap_or("n/a"),
            if self.best_for.trim().is_empty() {
                "n/a"
            } else {
                self.best_for.as_str()
            }
        )
    }
}

impl StrategyLibraryStore {
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let raw = std::fs::read_to_string(path)?;
            let mut store: Self = serde_json::from_str(&raw)?;
            let changed = store.ensure_default_strategies();
            if store.active.is_none() && !store.strategies.is_empty() {
                store.active = Some(first_strategy_id(&store.strategies));
            }
            if changed {
                store.save(path)?;
            }
            Ok(store)
        } else {
            let store = Self::default();
            store.save(path)?;
            Ok(store)
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn list_strategies(&self) -> StrategyListResult {
        let mut strategies = self
            .strategies
            .values()
            .map(|strategy| StrategySummary {
                id: strategy.id.clone(),
                name: strategy.name.clone(),
                author: strategy.author.clone(),
                lp_strategy: strategy.lp_strategy.clone(),
                best_for: strategy.best_for.clone(),
                active: self.active.as_deref() == Some(strategy.id.as_str()),
                added_at: date_prefix(&strategy.added_at),
            })
            .collect::<Vec<_>>();
        strategies.sort_by(|left, right| {
            right
                .active
                .cmp(&left.active)
                .then_with(|| left.id.cmp(&right.id))
        });
        StrategyListResult {
            active: self.active.clone(),
            count: strategies.len(),
            strategies,
        }
    }

    pub fn get_strategy(&self, id: &str) -> Option<StrategyEntry> {
        self.strategies.get(id).cloned()
    }

    pub fn get_active_strategy(&self) -> Option<StrategyEntry> {
        let active = self.active.as_deref()?;
        self.get_strategy(active)
    }

    pub fn add_strategy(&mut self, input: StrategyInput) -> Result<StrategySaveResult> {
        if input.id.trim().is_empty() || input.name.trim().is_empty() {
            return Err(anyhow!("id and name are required"));
        }
        let id = slugify(&input.id);
        if id.is_empty() {
            return Err(anyhow!("strategy id must contain letters or numbers"));
        }
        let now = chrono::Utc::now().to_rfc3339();
        let existing_added_at = self
            .strategies
            .get(&id)
            .map(|strategy| strategy.added_at.clone());
        let entry = StrategyEntry {
            id: id.clone(),
            name: input.name,
            author: input.author.unwrap_or_else(default_author),
            lp_strategy: normalize_lp_strategy(input.lp_strategy.as_deref().unwrap_or("bid_ask")),
            token_criteria: object_or_empty(input.token_criteria),
            entry: object_or_empty(input.entry),
            range: object_or_empty(input.range),
            exit: object_or_empty(input.exit),
            best_for: input.best_for.unwrap_or_default(),
            raw: input.raw.unwrap_or_default(),
            added_at: existing_added_at.unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        self.strategies.insert(id.clone(), entry.clone());
        if self.active.is_none() {
            self.active = Some(id.clone());
        }
        Ok(StrategySaveResult {
            saved: true,
            id,
            name: entry.name,
            active: self.active.as_deref() == Some(entry.id.as_str()),
        })
    }

    pub fn set_active_strategy(&mut self, id: &str) -> Result<ActiveStrategyResult> {
        let strategy = self
            .strategies
            .get(id)
            .ok_or_else(|| anyhow!("Strategy \"{}\" not found", id))?;
        self.active = Some(id.to_string());
        Ok(ActiveStrategyResult {
            active: id.to_string(),
            name: strategy.name.clone(),
        })
    }

    pub fn remove_strategy(&mut self, id: &str) -> Result<RemovedStrategyResult> {
        let removed = self
            .strategies
            .remove(id)
            .ok_or_else(|| anyhow!("Strategy \"{}\" not found", id))?;
        if self.active.as_deref() == Some(id) {
            self.active = self.strategies.keys().next().cloned();
        }
        Ok(RemovedStrategyResult {
            removed: true,
            id: removed.id,
            name: removed.name,
            new_active: self.active.clone(),
        })
    }

    fn ensure_default_strategies(&mut self) -> bool {
        let mut changed = false;
        let now = chrono::Utc::now().to_rfc3339();
        for strategy in default_strategy_entries(&now) {
            if !self.strategies.contains_key(&strategy.id) {
                self.strategies.insert(strategy.id.clone(), strategy);
                changed = true;
            }
        }
        if self.active.is_none() {
            self.active = Some("custom_ratio_spot".to_string());
            changed = true;
        }
        changed
    }
}

pub fn strategy_library_path() -> PathBuf {
    meridian_data_path("strategy-library.json")
}

pub fn load_default_store() -> Result<StrategyLibraryStore> {
    StrategyLibraryStore::load(&strategy_library_path())
}

pub fn add_strategy(input: StrategyInput) -> Result<StrategySaveResult> {
    let path = strategy_library_path();
    let mut store = StrategyLibraryStore::load(&path)?;
    let result = store.add_strategy(input)?;
    store.save(&path)?;
    Ok(result)
}

pub fn list_strategies() -> Result<StrategyListResult> {
    Ok(load_default_store()?.list_strategies())
}

pub fn get_strategy(id: &str) -> Result<StrategyEntry> {
    load_default_store()?
        .get_strategy(id)
        .ok_or_else(|| anyhow!("Strategy \"{}\" not found", id))
}

pub fn set_active_strategy(id: &str) -> Result<ActiveStrategyResult> {
    let path = strategy_library_path();
    let mut store = StrategyLibraryStore::load(&path)?;
    let result = store.set_active_strategy(id)?;
    store.save(&path)?;
    Ok(result)
}

pub fn remove_strategy(id: &str) -> Result<RemovedStrategyResult> {
    let path = strategy_library_path();
    let mut store = StrategyLibraryStore::load(&path)?;
    let result = store.remove_strategy(id)?;
    store.save(&path)?;
    Ok(result)
}

pub fn get_active_strategy() -> Result<Option<StrategyEntry>> {
    Ok(load_default_store()?.get_active_strategy())
}

pub fn active_strategy_prompt_summary() -> Result<Option<String>> {
    Ok(get_active_strategy()?.map(|strategy| strategy.prompt_summary()))
}

fn default_strategy_entries(now: &str) -> Vec<StrategyEntry> {
    vec![
        StrategyEntry {
            id: "custom_ratio_spot".to_string(),
            name: "Custom Ratio Spot".to_string(),
            author: "meridian".to_string(),
            lp_strategy: "spot".to_string(),
            token_criteria: json!({"notes": "Any token. Ratio expresses directional bias."}),
            entry: json!({"condition": "Directional view on token", "single_side": null, "notes": "75% token = bullish (sell on pump out of range). 75% SOL = bearish/DCA-in (buy on dip). Set bins_below:bins_above proportional to ratio."}),
            range: json!({"type": "custom", "notes": "bins_below:bins_above ratio matches token:SOL ratio. E.g., 75% token → ~52 bins below, ~17 bins above."}),
            exit: json!({"take_profit_pct": 10, "notes": "Close when OOR or TP hit. Re-deploy with updated ratio based on new momentum signals."}),
            best_for: "Expressing directional bias while earning fees both ways".to_string(),
            raw: String::new(),
            added_at: now.to_string(),
            updated_at: now.to_string(),
        },
        StrategyEntry {
            id: "single_sided_reseed".to_string(),
            name: "Single-Sided Bid-Ask + Re-seed".to_string(),
            author: "meridian".to_string(),
            lp_strategy: "bid_ask".to_string(),
            token_criteria: json!({"notes": "Volatile tokens with strong narrative. Must have active volume."}),
            entry: json!({"condition": "Deploy token-only (amount_x only, amount_y=0) bid-ask, bins below active bin only", "single_side": "token", "notes": "As price drops through bins, token sold for SOL. Bid-ask concentrates at bottom edge."}),
            range: json!({"type": "default", "bins_below_pct": 100, "notes": "All bins below active bin. bins_above=0."}),
            exit: json!({"notes": "When OOR downside: close_position(skip_swap=true) → redeploy token-only bid-ask at new lower price. Do NOT swap to SOL. Full close only when token dead or after N re-seeds with declining performance."}),
            best_for: "Riding volatile tokens down without cutting losses. DCA out via LP.".to_string(),
            raw: String::new(),
            added_at: now.to_string(),
            updated_at: now.to_string(),
        },
        StrategyEntry {
            id: "fee_compounding".to_string(),
            name: "Fee Compounding".to_string(),
            author: "meridian".to_string(),
            lp_strategy: "any".to_string(),
            token_criteria: json!({"notes": "Stable volume pools with consistent fee generation."}),
            entry: json!({"condition": "Deploy normally with any shape", "notes": "Strategy is about management, not entry shape."}),
            range: json!({"type": "default", "notes": "Standard range for the pair."}),
            exit: json!({"notes": "When unclaimed fees > $5 AND in range: claim_fees → add_liquidity back into same position. Normal close rules otherwise."}),
            best_for: "Maximizing yield on stable, range-bound pools via compounding".to_string(),
            raw: String::new(),
            added_at: now.to_string(),
            updated_at: now.to_string(),
        },
        StrategyEntry {
            id: "multi_layer".to_string(),
            name: "Multi-Layer".to_string(),
            author: "meridian".to_string(),
            lp_strategy: "mixed".to_string(),
            token_criteria: json!({"notes": "High volume pools. Layer multiple shapes into ONE position via addLiquidityByStrategy to sculpt a composite distribution."}),
            entry: json!({
                "condition": "Create ONE position, then layer additional shapes onto it with add-liquidity. Each layer adds a different strategy/shape to the same position, compositing them.",
                "notes": "Step 1: deploy (creates position with first shape). Step 2+: add-liquidity to same position with different shapes. All layers share the same bin range but different distribution curves stack on top of each other.",
                "example_patterns": {
                    "smooth_edge": "Deploy Bid-Ask (edges) → add-liquidity Spot (fills the middle gap). 2 layers, 1 position.",
                    "full_composite": "Deploy Bid-Ask (edges) → add-liquidity Spot (middle) → add-liquidity Curve (center boost). 3 layers, 1 position.",
                    "edge_heavy": "Deploy Bid-Ask → add-liquidity Bid-Ask again (double edge weight). 2 layers, 1 position."
                }
            }),
            range: json!({"type": "custom", "notes": "All layers share the position's bin range (set at deploy). Choose range wide enough for the widest layer needed."}),
            exit: json!({"notes": "Single position — one close, one claim. The composite shape means fees earned reflect ALL layers combined."}),
            best_for: "Creating custom liquidity distributions by stacking shapes in one position. Single position to manage.".to_string(),
            raw: String::new(),
            added_at: now.to_string(),
            updated_at: now.to_string(),
        },
        StrategyEntry {
            id: "partial_harvest".to_string(),
            name: "Partial Harvest".to_string(),
            author: "meridian".to_string(),
            lp_strategy: "any".to_string(),
            token_criteria: json!({"notes": "High fee pools where taking profit incrementally is preferred."}),
            entry: json!({"condition": "Deploy normally", "notes": "Strategy is about progressive profit-taking, not entry."}),
            range: json!({"type": "default", "notes": "Standard range."}),
            exit: json!({"take_profit_pct": 10, "notes": "When total return >= 10% of deployed capital: withdraw_liquidity(bps=5000) to take 50% off. Remaining 50% keeps running. Repeat at next threshold."}),
            best_for: "Locking in profits without fully exiting winning positions".to_string(),
            raw: String::new(),
            added_at: now.to_string(),
            updated_at: now.to_string(),
        },
    ]
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_underscore = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_underscore = false;
        } else if !last_was_underscore {
            slug.push('_');
            last_was_underscore = true;
        }
    }
    slug.trim_matches('_').to_string()
}

fn default_author() -> String {
    "unknown".to_string()
}

fn default_lp_strategy() -> String {
    "bid_ask".to_string()
}

fn normalize_lp_strategy(value: &str) -> String {
    match value.to_ascii_lowercase().replace('-', "_").as_str() {
        "spot" => "spot".to_string(),
        "curve" => "curve".to_string(),
        "bidask" | "bid_ask" => "bid_ask".to_string(),
        "any" => "any".to_string(),
        "mixed" => "mixed".to_string(),
        other => other.to_string(),
    }
}

fn object_or_empty(value: Value) -> Value {
    if value.is_object() {
        value
    } else {
        json!({})
    }
}

fn object_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)?
        .as_str()
        .filter(|text| !text.trim().is_empty())
}

fn date_prefix(value: &str) -> Option<String> {
    if value.len() >= 10 {
        Some(value[..10].to_string())
    } else {
        None
    }
}

fn first_strategy_id(strategies: &BTreeMap<String, StrategyEntry>) -> String {
    strategies.keys().next().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn unique_test_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "meridian-{label}-{}-{}.json",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    #[test]
    fn default_library_preloads_active_strategy_presets() {
        let path = unique_test_path("strategy-library-defaults");
        let store = StrategyLibraryStore::load(&path).expect("store should load");
        let list = store.list_strategies();
        std::fs::remove_file(&path).ok();

        assert_eq!(store.active.as_deref(), Some("custom_ratio_spot"));
        assert_eq!(list.count, 5);
        assert!(list
            .strategies
            .iter()
            .any(|strategy| strategy.id == "custom_ratio_spot" && strategy.active));
        assert!(list
            .strategies
            .iter()
            .any(|strategy| strategy.id == "single_sided_reseed" && !strategy.active));
        assert!(store.get_strategy("multi_layer").is_some());
    }

    #[test]
    fn add_set_active_and_remove_strategy_persists() {
        let path = unique_test_path("strategy-library-crud");
        let mut store = StrategyLibraryStore::load(&path).expect("store should load");
        let saved = store
            .add_strategy(StrategyInput {
                id: "Panda Strat!".to_string(),
                name: "Panda Strat".to_string(),
                author: Some("top-lper".to_string()),
                lp_strategy: Some("curve".to_string()),
                token_criteria: serde_json::json!({"min_mcap": 150000, "notes": "fresh but liquid"}),
                entry: serde_json::json!({"condition": "after pullback"}),
                range: serde_json::json!({"type": "wide", "bins_below_pct": 85}),
                exit: serde_json::json!({"take_profit_pct": 8}),
                best_for: Some("volatile narrative pools".to_string()),
                raw: Some("tweet text".to_string()),
            })
            .expect("strategy should save");

        assert_eq!(saved.id, "panda_strat");
        assert!(!saved.active);
        store
            .set_active_strategy("panda_strat")
            .expect("active strategy should set");
        store.save(&path).expect("store should save");

        let mut reloaded = StrategyLibraryStore::load(&path).expect("store should reload");
        let active = reloaded
            .get_active_strategy()
            .expect("active strategy exists");
        assert_eq!(active.name, "Panda Strat");
        assert_eq!(active.lp_strategy, "curve");
        assert_eq!(active.token_criteria["min_mcap"], 150000);

        let removed = reloaded
            .remove_strategy("panda_strat")
            .expect("strategy should remove");
        std::fs::remove_file(&path).ok();

        assert_eq!(removed.id, "panda_strat");
        assert_eq!(removed.new_active.as_deref(), Some("custom_ratio_spot"));
    }

    #[test]
    fn active_strategy_summary_matches_screening_prompt_context() {
        let store = StrategyLibraryStore::default();
        let active = store
            .get_active_strategy()
            .expect("default active strategy");
        let summary = active.prompt_summary();

        assert!(summary.contains("STRATEGY CONTEXT: Custom Ratio Spot"));
        assert!(summary.contains("entry:"));
        assert!(summary.contains("exit:"));
        assert!(summary.contains("best for:"));
    }
}
