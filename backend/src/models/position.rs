use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedPosition {
    pub id: String,
    pub pool_address: String,
    pub pool_name: Option<String>,
    pub base_mint: String,
    pub base_symbol: Option<String>,
    pub lower_bin: i32,
    pub upper_bin: i32,
    pub amount_sol: f64,
    pub status: PositionStatus,
    pub created_at: String,
    pub entry_mcap: Option<f64>,
    pub entry_tvl: Option<f64>,
    pub entry_volume: Option<f64>,
    pub entry_holders: Option<u64>,
    pub total_fees_claimed: f64,
    pub instruction: Option<String>,
    pub note: Option<String>,
    pub out_of_range_since: Option<String>,
    pub pnl_sol: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PositionStatus {
    Active,
    OutOfRange,
    Closed,
    Claimed,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PositionState {
    pub positions: HashMap<String, TrackedPosition>,
}

impl PositionState {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let p = std::path::Path::new(path);
        if p.exists() {
            let content = std::fs::read_to_string(path)?;
            Ok(serde_json::from_str(&content).unwrap_or_default())
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn add(&mut self, pos: TrackedPosition) {
        self.positions.insert(pos.id.clone(), pos);
    }

    pub fn remove(&mut self, id: &str) {
        self.positions.remove(id);
    }

    pub fn get_active(&self) -> Vec<&TrackedPosition> {
        self.positions
            .values()
            .filter(|p| {
                p.status == PositionStatus::Active || p.status == PositionStatus::OutOfRange
            })
            .collect()
    }

    pub fn count_active(&self) -> usize {
        self.positions
            .values()
            .filter(|p| {
                p.status == PositionStatus::Active || p.status == PositionStatus::OutOfRange
            })
            .count()
    }

    pub fn mark_oor(&mut self, id: &str) {
        if let Some(p) = self.positions.get_mut(id) {
            if p.status == PositionStatus::Active {
                p.status = PositionStatus::OutOfRange;
                p.out_of_range_since = Some(chrono::Utc::now().to_rfc3339());
            }
        }
    }

    pub fn mark_in_range(&mut self, id: &str) {
        if let Some(p) = self.positions.get_mut(id) {
            if p.status == PositionStatus::OutOfRange {
                p.status = PositionStatus::Active;
                p.out_of_range_since = None;
            }
        }
    }

    pub fn record_claim(&mut self, id: &str, fees: f64) {
        if let Some(p) = self.positions.get_mut(id) {
            p.total_fees_claimed += fees;
        }
    }

    pub fn record_close(&mut self, id: &str, pnl: f64) {
        if let Some(p) = self.positions.get_mut(id) {
            p.status = PositionStatus::Closed;
            p.pnl_sol = Some(pnl);
        }
    }
}
