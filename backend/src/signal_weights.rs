use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use crate::config::types::DarwinConfig;
use crate::lessons::PerformanceRecord;

const SIGNAL_NAMES: &[&str] = &[
    "organic_score",
    "fee_tvl_ratio",
    "volume",
    "mcap",
    "holder_count",
    "smart_wallets_present",
    "narrative_quality",
    "study_win_rate",
    "hive_consensus",
    "volatility",
    "entry_mcap",
    "entry_tvl",
    "entry_volume",
];

const HIGHER_IS_BETTER: &[&str] = &[
    "organic_score",
    "fee_tvl_ratio",
    "volume",
    "holder_count",
    "study_win_rate",
    "hive_consensus",
];
const BOOLEAN_SIGNALS: &[&str] = &["smart_wallets_present"];
const CATEGORICAL_SIGNALS: &[&str] = &["narrative_quality"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalWeightChange {
    pub signal: String,
    pub from: f64,
    pub to: f64,
    pub lift: f64,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalWeightsHistoryEntry {
    pub timestamp: String,
    pub changes: Vec<SignalWeightChange>,
    pub window_size: usize,
    pub win_count: usize,
    pub loss_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalWeightsStore {
    pub weights: BTreeMap<String, f64>,
    #[serde(default)]
    pub last_recalc: Option<String>,
    #[serde(default)]
    pub recalc_count: u64,
    #[serde(default)]
    pub history: Vec<SignalWeightsHistoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalWeightsResult {
    pub changes: Vec<SignalWeightChange>,
    pub weights: BTreeMap<String, f64>,
}

impl Default for SignalWeightsStore {
    fn default() -> Self {
        Self {
            weights: default_weights(),
            last_recalc: None,
            recalc_count: 0,
            history: vec![],
        }
    }
}

impl SignalWeightsStore {
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let raw = std::fs::read_to_string(path)?;
            let mut store: Self = serde_json::from_str(&raw)?;
            store.ensure_defaults();
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

    pub fn summary(&self) -> String {
        let mut lines =
            vec!["Signal Weights (Darwinian — learned from past positions):".to_string()];
        let mut signals = SIGNAL_NAMES
            .iter()
            .filter_map(|signal| self.weights.get(*signal).map(|weight| (*signal, *weight)))
            .collect::<Vec<_>>();
        signals.sort_by(|(left_signal, left_weight), (right_signal, right_weight)| {
            right_weight
                .partial_cmp(left_weight)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left_signal.cmp(right_signal))
        });

        for (signal, weight) in signals {
            lines.push(format!(
                "  {:<24} {:.2}  {}  {}",
                signal,
                weight,
                weight_bar(weight),
                interpret_weight(weight)
            ));
        }

        if let Some(last_recalc) = &self.last_recalc {
            lines.push(format!(
                "\nLast recalculated: {} ({} total)",
                last_recalc, self.recalc_count
            ));
        } else {
            lines.push("\nWeights have not been recalculated yet (using defaults).".to_string());
        }

        lines.join("\n")
    }

    fn ensure_defaults(&mut self) {
        for signal in SIGNAL_NAMES {
            self.weights.entry((*signal).to_string()).or_insert(1.0);
        }
    }
}

pub fn recalculate_weights(
    performance: &[PerformanceRecord],
    config: &DarwinConfig,
    store: &mut SignalWeightsStore,
) -> SignalWeightsResult {
    store.ensure_defaults();
    let weights = &mut store.weights;
    let recent = filter_recent(performance, config.window_days);
    let min_samples = config.min_samples as usize;
    if recent.len() < min_samples {
        return SignalWeightsResult {
            changes: vec![],
            weights: weights.clone(),
        };
    }

    let wins = recent
        .iter()
        .copied()
        .filter(|record| record.pnl_sol > 0.0)
        .collect::<Vec<_>>();
    let losses = recent
        .iter()
        .copied()
        .filter(|record| record.pnl_sol <= 0.0)
        .collect::<Vec<_>>();
    if wins.is_empty() || losses.is_empty() {
        return SignalWeightsResult {
            changes: vec![],
            weights: weights.clone(),
        };
    }

    let mut ranked = SIGNAL_NAMES
        .iter()
        .filter_map(|signal| {
            compute_lift(signal, &wins, &losses, min_samples)
                .map(|lift| ((*signal).to_string(), lift))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|(_, left_lift), (_, right_lift)| {
        right_lift
            .partial_cmp(left_lift)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if ranked.is_empty() {
        return SignalWeightsResult {
            changes: vec![],
            weights: weights.clone(),
        };
    }

    let q1_end = ((ranked.len() as f64) * 0.25).ceil() as usize;
    let q3_start = ((ranked.len() as f64) * 0.75).floor() as usize;
    let top_quartile = ranked
        .iter()
        .take(q1_end)
        .map(|(signal, _)| signal.as_str())
        .collect::<HashSet<_>>();
    let bottom_quartile = ranked
        .iter()
        .skip(q3_start)
        .map(|(signal, _)| signal.as_str())
        .collect::<HashSet<_>>();

    let mut changes = Vec::new();
    for (signal, lift) in &ranked {
        let previous = *weights.get(signal).unwrap_or(&1.0);
        let mut next = previous;
        if top_quartile.contains(signal.as_str()) {
            next = (previous * config.boost_factor).min(config.weight_ceiling);
        } else if bottom_quartile.contains(signal.as_str()) {
            next = (previous * config.decay_factor).max(config.weight_floor);
        }
        next = round3(next);
        if (next - previous).abs() > f64::EPSILON {
            let action = if next > previous {
                "boosted"
            } else {
                "decayed"
            }
            .to_string();
            changes.push(SignalWeightChange {
                signal: signal.clone(),
                from: previous,
                to: next,
                lift: round3(*lift),
                action,
            });
            weights.insert(signal.clone(), next);
        }
    }

    let timestamp = chrono::Utc::now().to_rfc3339();
    store.last_recalc = Some(timestamp.clone());
    store.recalc_count += 1;
    if !changes.is_empty() {
        store.history.push(SignalWeightsHistoryEntry {
            timestamp,
            changes: changes.clone(),
            window_size: recent.len(),
            win_count: wins.len(),
            loss_count: losses.len(),
        });
        if store.history.len() > 20 {
            let excess = store.history.len() - 20;
            store.history.drain(..excess);
        }
    }

    SignalWeightsResult {
        changes,
        weights: weights.clone(),
    }
}

fn default_weights() -> BTreeMap<String, f64> {
    SIGNAL_NAMES
        .iter()
        .map(|signal| ((*signal).to_string(), 1.0))
        .collect()
}

fn filter_recent(performance: &[PerformanceRecord], window_days: u64) -> Vec<&PerformanceRecord> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(window_days as i64);
    performance
        .iter()
        .filter(|record| {
            chrono::DateTime::parse_from_rfc3339(&record.timestamp)
                .map(|timestamp| timestamp.with_timezone(&chrono::Utc) >= cutoff)
                .unwrap_or(false)
        })
        .collect()
}

fn compute_lift(
    signal: &str,
    wins: &[&PerformanceRecord],
    losses: &[&PerformanceRecord],
    min_samples: usize,
) -> Option<f64> {
    if BOOLEAN_SIGNALS.contains(&signal) {
        compute_boolean_lift(signal, wins, losses, min_samples)
    } else if CATEGORICAL_SIGNALS.contains(&signal) {
        compute_categorical_lift(signal, wins, losses, min_samples)
    } else {
        compute_numeric_lift(signal, wins, losses, min_samples)
    }
}

fn compute_numeric_lift(
    signal: &str,
    wins: &[&PerformanceRecord],
    losses: &[&PerformanceRecord],
    min_samples: usize,
) -> Option<f64> {
    let win_values = extract_numeric(signal, wins);
    let loss_values = extract_numeric(signal, losses);
    if win_values.len() + loss_values.len() < min_samples
        || win_values.is_empty()
        || loss_values.is_empty()
    {
        return None;
    }

    let min = win_values
        .iter()
        .chain(loss_values.iter())
        .fold(f64::INFINITY, |acc, value| acc.min(*value));
    let max = win_values
        .iter()
        .chain(loss_values.iter())
        .fold(f64::NEG_INFINITY, |acc, value| acc.max(*value));
    let range = max - min;
    if range.abs() < f64::EPSILON {
        return Some(0.0);
    }

    let normalize = |value: f64| (value - min) / range;
    let win_mean = mean(
        &win_values
            .iter()
            .map(|value| normalize(*value))
            .collect::<Vec<_>>(),
    );
    let loss_mean = mean(
        &loss_values
            .iter()
            .map(|value| normalize(*value))
            .collect::<Vec<_>>(),
    );
    let lift = if HIGHER_IS_BETTER.contains(&signal) {
        win_mean - loss_mean
    } else {
        (win_mean - loss_mean).abs()
    };
    Some(lift)
}

fn compute_boolean_lift(
    signal: &str,
    wins: &[&PerformanceRecord],
    losses: &[&PerformanceRecord],
    min_samples: usize,
) -> Option<f64> {
    let mut true_wins = 0usize;
    let mut true_total = 0usize;
    let mut false_wins = 0usize;
    let mut false_total = 0usize;

    for (is_win, record) in wins
        .iter()
        .map(|record| (true, *record))
        .chain(losses.iter().map(|record| (false, *record)))
    {
        let Some(value) = signal_value(record, signal).and_then(|value| value.as_bool()) else {
            continue;
        };
        if value {
            true_total += 1;
            if is_win {
                true_wins += 1;
            }
        } else {
            false_total += 1;
            if is_win {
                false_wins += 1;
            }
        }
    }

    if true_total + false_total < min_samples || true_total == 0 || false_total == 0 {
        return None;
    }

    Some((true_wins as f64 / true_total as f64) - (false_wins as f64 / false_total as f64))
}

fn compute_categorical_lift(
    signal: &str,
    wins: &[&PerformanceRecord],
    losses: &[&PerformanceRecord],
    min_samples: usize,
) -> Option<f64> {
    let mut buckets: HashMap<String, (usize, usize)> = HashMap::new();
    for (is_win, record) in wins
        .iter()
        .map(|record| (true, *record))
        .chain(losses.iter().map(|record| (false, *record)))
    {
        let Some(value) =
            signal_value(record, signal).and_then(|value| value.as_str().map(str::to_string))
        else {
            continue;
        };
        let entry = buckets.entry(value).or_insert((0, 0));
        entry.1 += 1;
        if is_win {
            entry.0 += 1;
        }
    }

    let total_samples = buckets.values().map(|(_, total)| *total).sum::<usize>();
    if total_samples < min_samples {
        return None;
    }
    let rates = buckets
        .values()
        .filter(|(_, total)| *total >= 2)
        .map(|(wins, total)| *wins as f64 / *total as f64)
        .collect::<Vec<_>>();
    if rates.len() < 2 {
        return None;
    }
    let min = rates.iter().fold(f64::INFINITY, |acc, rate| acc.min(*rate));
    let max = rates
        .iter()
        .fold(f64::NEG_INFINITY, |acc, rate| acc.max(*rate));
    Some(max - min)
}

fn extract_numeric(signal: &str, entries: &[&PerformanceRecord]) -> Vec<f64> {
    entries
        .iter()
        .filter_map(|record| signal_value(record, signal).and_then(|value| value.as_f64()))
        .filter(|value| value.is_finite())
        .collect()
}

fn signal_value(record: &PerformanceRecord, signal: &str) -> Option<Value> {
    let snapshot: Value = serde_json::from_str(&record.signal_snapshot).ok()?;
    snapshot.get(signal).cloned()
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn interpret_weight(value: f64) -> &'static str {
    if value >= 1.8 {
        "[STRONG]"
    } else if value >= 1.2 {
        "[above avg]"
    } else if value >= 0.8 {
        "[neutral]"
    } else if value >= 0.5 {
        "[below avg]"
    } else {
        "[weak]"
    }
}

fn weight_bar(value: f64) -> String {
    let filled = (((value - 0.3) / (2.5 - 0.3)) * 10.0).round() as i32;
    let clamped = filled.clamp(0, 10) as usize;
    format!("{}{}", "#".repeat(clamped), ".".repeat(10 - clamped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: &str, pnl_sol: f64, signal_snapshot: serde_json::Value) -> PerformanceRecord {
        PerformanceRecord {
            id: id.to_string(),
            position_id: format!("pos-{id}"),
            pool: format!("pool-{id}"),
            symbol: id.to_ascii_uppercase(),
            pnl_sol,
            fees_earned: 0.01,
            range_efficiency: if pnl_sol > 0.0 { 0.9 } else { 0.35 },
            close_reason: if pnl_sol > 0.0 {
                "take_profit"
            } else {
                "low_yield"
            }
            .to_string(),
            signal_snapshot: signal_snapshot.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn test_config() -> DarwinConfig {
        DarwinConfig {
            enabled: true,
            window_days: 60,
            recalc_every: 5,
            boost_factor: 1.10,
            decay_factor: 0.90,
            weight_floor: 0.50,
            weight_ceiling: 2.00,
            min_samples: 6,
        }
    }

    #[test]
    fn recalculates_signal_weights_from_winning_and_losing_snapshots() {
        let records = vec![
            record(
                "win-1",
                0.12,
                json!({"organic_score": 91.0, "volume": 5000.0}),
            ),
            record(
                "win-2",
                0.08,
                json!({"organic_score": 88.0, "volume": 5000.0}),
            ),
            record(
                "win-3",
                0.05,
                json!({"organic_score": 84.0, "volume": 5000.0}),
            ),
            record(
                "loss-1",
                -0.04,
                json!({"organic_score": 35.0, "volume": 5000.0}),
            ),
            record(
                "loss-2",
                -0.07,
                json!({"organic_score": 30.0, "volume": 5000.0}),
            ),
            record(
                "loss-3",
                -0.03,
                json!({"organic_score": 42.0, "volume": 5000.0}),
            ),
        ];
        let mut store = SignalWeightsStore::default();

        let result = recalculate_weights(&records, &test_config(), &mut store);

        assert!(
            !result.changes.is_empty(),
            "expected at least one Darwin weight change"
        );
        assert!(result
            .changes
            .iter()
            .any(|change| change.action == "boosted"));
        assert!(result
            .changes
            .iter()
            .any(|change| change.action == "decayed"));
        assert!(result.weights["organic_score"] > 1.0);
        assert_eq!(store.recalc_count, 1);
        assert!(store.last_recalc.is_some());
        assert_eq!(store.history.len(), 1);
        assert_eq!(store.history[0].window_size, 6);
        assert_eq!(store.history[0].win_count, 3);
        assert_eq!(store.history[0].loss_count, 3);
    }

    #[test]
    fn skips_recalculation_when_window_has_too_few_samples() {
        let records = vec![
            record("win", 0.04, json!({"organic_score": 90.0})),
            record("loss", -0.02, json!({"organic_score": 20.0})),
        ];
        let mut store = SignalWeightsStore::default();

        let result = recalculate_weights(&records, &test_config(), &mut store);

        assert!(result.changes.is_empty());
        assert_eq!(store.recalc_count, 0);
        assert_eq!(result.weights["organic_score"], 1.0);
    }

    #[test]
    fn saves_loads_and_summarizes_signal_weights() {
        let path = std::env::temp_dir().join(format!(
            "signal-weights-{}-{}.json",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let mut store = SignalWeightsStore::default();
        store.weights.insert("organic_score".to_string(), 1.25);
        store.weights.insert("volume".to_string(), 0.75);
        store.last_recalc = Some("2026-06-09T00:00:00Z".to_string());
        store.recalc_count = 2;

        store.save(&path).expect("weights should save");
        let loaded = SignalWeightsStore::load(&path).expect("weights should load");
        let summary = loaded.summary();
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.weights["organic_score"], 1.25);
        assert!(summary.contains("Signal Weights"));
        assert!(summary.contains("organic_score"));
        assert!(summary.contains("[above avg]"));
        assert!(summary.contains("Last recalculated"));
    }
}
