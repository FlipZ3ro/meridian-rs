use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// ─── Lesson ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub content: String,
    pub pinned: bool,
    pub created_at: String,
    #[serde(default)]
    pub role: Option<String>,       // "manager", "screener", "general"
    #[serde(default)]
    pub tags: Vec<String>,          // ["close", "deploy", "skip", "performance"]
    #[serde(default)]
    pub confidence: f64,            // 0.0-1.0, higher = more trustworthy
    #[serde(default)]
    pub source: String,             // "manual", "auto", "evolved"
}

// ─── Performance Record ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceRecord {
    pub id: String,
    pub position_id: String,
    pub pool: String,
    pub symbol: String,
    pub pnl_sol: f64,
    pub fees_earned: f64,
    pub range_efficiency: f64,  // 0-1, how well range captured price action
    pub close_reason: String,   // "stop_loss", "take_profit", "oor", "low_yield", "trailing"
    pub signal_snapshot: String, // JSON of screening signals that led to deploy
    pub timestamp: String,
}

// ─── Config for Evolution ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionConfig {
    pub evolve_every_n_closes: usize,
    pub min_organic_min: f64,
    pub min_organic_max: f64,
    pub min_fee_active_tvl_ratio_min: f64,
    pub min_fee_active_tvl_ratio_max: f64,
    pub boost_good: f64,   // multiplier for good signals (e.g. 1.05)
    pub decay_bad: f64,    // multiplier for bad signals (e.g. 0.95)
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            evolve_every_n_closes: 5,
            min_organic_min: 30.0,
            min_organic_max: 80.0,
            min_fee_active_tvl_ratio_min: 0.005,
            min_fee_active_tvl_ratio_max: 0.05,
            boost_good: 1.05,
            decay_bad: 0.95,
        }
    }
}

// ─── Lesson Store ───────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LessonStore {
    pub lessons: Vec<Lesson>,
    #[serde(default)]
    pub performance: Vec<PerformanceRecord>,
    #[serde(default)]
    pub close_count: usize,
}

impl LessonStore {
    pub fn load(path: &str) -> Result<Self> {
        if Path::new(path).exists() {
            let content = fs::read_to_string(path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, path: &str) -> Result<()> {
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    // ── Basic CRUD ─────────────────────────────────────────────

    pub fn add(&mut self, content: &str) {
        self.lessons.push(Lesson {
            id: format!("lesson_{}", chrono::Utc::now().timestamp_millis()),
            content: content.to_string(),
            pinned: false,
            created_at: chrono::Utc::now().to_rfc3339(),
            role: None,
            tags: vec![],
            confidence: 0.5,
            source: "manual".to_string(),
        });
    }

    pub fn add_with_meta(&mut self, content: &str, role: &str, tags: Vec<String>, confidence: f64, source: &str) {
        self.lessons.push(Lesson {
            id: format!("lesson_{}", chrono::Utc::now().timestamp_millis()),
            content: content.to_string(),
            pinned: false,
            created_at: chrono::Utc::now().to_rfc3339(),
            role: Some(role.to_string()),
            tags,
            confidence,
            source: source.to_string(),
        });
    }

    pub fn pin(&mut self, id: &str) -> bool {
        if let Some(l) = self.lessons.iter_mut().find(|l| l.id == id) {
            l.pinned = true;
            true
        } else {
            false
        }
    }

    pub fn unpin(&mut self, id: &str) -> bool {
        if let Some(l) = self.lessons.iter_mut().find(|l| l.id == id) {
            l.pinned = false;
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.lessons.retain(|l| l.pinned);
    }

    // ── Performance Recording ──────────────────────────────────

    pub fn record_performance(
        &mut self,
        position_id: &str,
        pool: &str,
        symbol: &str,
        pnl_sol: f64,
        fees_earned: f64,
        range_efficiency: f64,
        close_reason: &str,
        signal_snapshot: &str,
    ) {
        let record = PerformanceRecord {
            id: format!("perf_{}", chrono::Utc::now().timestamp_millis()),
            position_id: position_id.to_string(),
            pool: pool.to_string(),
            symbol: symbol.to_string(),
            pnl_sol,
            fees_earned,
            range_efficiency,
            close_reason: close_reason.to_string(),
            signal_snapshot: signal_snapshot.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        self.performance.push(record);

        // Keep rolling window of 200 records
        if self.performance.len() > 200 {
            let excess = self.performance.len() - 200;
            self.performance.drain(..excess);
        }

        self.close_count += 1;

        // Auto-learn from this close
        let lesson_content = if pnl_sol > 0.0 {
            format!(" profitable close: {} SOL profit on {} ({})", format_f64(pnl_sol), symbol, close_reason)
        } else {
            format!(" loss: {} SOL on {} ({})", format_f64(pnl_sol), symbol, close_reason)
        };

        self.add_with_meta(
            &lesson_content,
            "manager",
            vec!["close".to_string(), "performance".to_string(), close_reason.to_string()],
            if pnl_sol > 0.0 { 0.7 } else { 0.6 },
            "auto",
        );
    }

    // ── Darwinian Evolution ────────────────────────────────────

    /// Check if evolution is due and evolve thresholds.
    /// Returns (new_min_organic, new_min_fee_active_tvl_ratio, evolved_lesson).
    pub fn evolve_thresholds(
        &mut self,
        current_min_organic: f64,
        current_min_fee_active_tvl_ratio: f64,
        config: &EvolutionConfig,
    ) -> Option<(f64, f64, String)> {
        if self.close_count == 0 || self.close_count % config.evolve_every_n_closes != 0 {
            return None;
        }

        // Split performance into quartiles
        let total = self.performance.len();
        if total < 4 {
            return None;
        }

        let mut sorted = self.performance.clone();
        sorted.sort_by(|a, b| a.pnl_sol.partial_cmp(&b.pnl_sol).unwrap_or(std::cmp::Ordering::Equal));

        let q1_end = total / 4;
        let q4_start = (3 * total) / 4;

        let q1_avg = sorted[..q1_end].iter().map(|r| r.pnl_sol).sum::<f64>() / q1_end as f64;
        let q4_avg = sorted[q4_start..].iter().map(|r| r.pnl_sol).sum::<f64>() / (total - q4_start) as f64;

        let mut new_min_organic = current_min_organic;
        let mut new_min_fee = current_min_fee_active_tvl_ratio;
        let mut changes = Vec::new();

        // If bottom quartile is losing significantly, tighten thresholds
        if q1_avg < -0.01 {
            // Increase min_organic to filter out low-quality pools
            new_min_organic = (current_min_organic * config.boost_good)
                .max(config.min_organic_min)
                .min(config.min_organic_max);
            if (new_min_organic - current_min_organic).abs() > 0.1 {
                changes.push(format!("min_organic: {:.1} -> {:.1}", current_min_organic, new_min_organic));
            }
        }

        // If top quartile is doing well, we can be slightly more aggressive
        if q4_avg > 0.05 {
            new_min_fee = (current_min_fee_active_tvl_ratio * config.decay_bad)
                .max(config.min_fee_active_tvl_ratio_min)
                .min(config.min_fee_active_tvl_ratio_max);
            if (new_min_fee - current_min_fee_active_tvl_ratio).abs() > 0.0001 {
                changes.push(format!("min_fee_tvl: {:.4} -> {:.4}", current_min_fee_active_tvl_ratio, new_min_fee));
            }
        }

        if changes.is_empty() {
            return None;
        }

        let lesson = format!(
            "[SELF-TUNED] Evolved after {} closes. Q1 avg: {} SOL, Q4 avg: {} SOL. Changes: {}",
            self.close_count,
            format_f64(q1_avg),
            format_f64(q4_avg),
            changes.join(", ")
        );

        self.add_with_meta(
            &lesson,
            "manager",
            vec!["evolved".to_string(), "thresholds".to_string()],
            0.8,
            "evolved",
        );

        Some((new_min_organic, new_min_fee, lesson))
    }

    // ── 3-Tier Injection for Prompts ───────────────────────────

    pub fn get_for_prompt(&self, role: Option<&str>) -> String {
        if self.lessons.is_empty() {
            return String::new();
        }

        let mut output = Vec::new();

        // Tier 1: Pinned lessons (highest priority)
        let pinned: Vec<&Lesson> = self.lessons.iter().filter(|l| l.pinned).collect();
        if !pinned.is_empty() {
            output.push("PINNED LESSONS:".to_string());
            for l in pinned.iter().take(5) {
                output.push(format!("  * {}", l.content));
            }
        }

        // Tier 2: Role-specific lessons
        if let Some(role) = role {
            let role_lessons: Vec<&Lesson> = self.lessons.iter()
                .filter(|l| l.role.as_deref() == Some(role) && !l.pinned)
                .collect();
            if !role_lessons.is_empty() {
                output.push(format!("{} LESSONS:", role.to_uppercase()));
                for l in role_lessons.iter().take(3) {
                    output.push(format!("  - {}", l.content));
                }
            }
        }

        // Tier 3: Recent lessons (last 3)
        let recent: Vec<&Lesson> = self.lessons.iter()
            .rev()
            .filter(|l| !l.pinned)
            .take(3)
            .collect();
        if !recent.is_empty() {
            output.push("RECENT:".to_string());
            for l in recent {
                output.push(format!("  - {}", l.content));
            }
        }

        if output.is_empty() {
            String::new()
        } else {
            output.join("\n")
        }
    }

    // ── Stats ──────────────────────────────────────────────────

    pub fn get_performance_summary(&self) -> String {
        if self.performance.is_empty() {
            return "No performance data yet.".to_string();
        }

        let total_pnl: f64 = self.performance.iter().map(|r| r.pnl_sol).sum();
        let total_fees: f64 = self.performance.iter().map(|r| r.fees_earned).sum();
        let wins = self.performance.iter().filter(|r| r.pnl_sol > 0.0).count();
        let losses = self.performance.iter().filter(|r| r.pnl_sol <= 0.0).count();
        let avg_range_eff: f64 = self.performance.iter().map(|r| r.range_efficiency).sum::<f64>()
            / self.performance.len() as f64;

        format!(
            "Performance: {} closes | {}W/{}L | PnL: {} SOL | Fees: {} SOL | Avg range eff: {:.1}%",
            self.performance.len(),
            wins,
            losses,
            format_f64(total_pnl),
            format_f64(total_fees),
            avg_range_eff * 100.0,
        )
    }
}

fn format_f64(v: f64) -> String {
    if v.abs() >= 1.0 {
        format!("{:.2}", v)
    } else {
        format!("{:.4}", v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_performance() {
        let mut store = LessonStore::default();
        store.record_performance(
            "pos-1", "pool-1", "TEST", 0.05, 0.01, 0.8, "take_profit", "{}",
        );
        assert_eq!(store.performance.len(), 1);
        assert_eq!(store.close_count, 1);
        assert!(!store.lessons.is_empty()); // auto-lesson created
    }

    #[test]
    fn test_evolve_no_data() {
        let mut store = LessonStore::default();
        let config = EvolutionConfig::default();
        assert!(store.evolve_thresholds(50.0, 0.01, &config).is_none());
    }

    #[test]
    fn test_evolve_after_n_closes() {
        let mut store = LessonStore::default();
        let config = EvolutionConfig { evolve_every_n_closes: 4, ..Default::default() };

        // Add 4 losing records
        for _ in 0..4 {
            store.record_performance("pos", "pool", "TEST", -0.05, 0.001, 0.5, "stop_loss", "{}");
        }
        assert_eq!(store.close_count, 4);

        let result = store.evolve_thresholds(50.0, 0.01, &config);
        assert!(result.is_some());
        let (new_org, _new_fee, _lesson) = result.unwrap();
        assert!(new_org >= 50.0); // Should have tightened (increased)
    }

    #[test]
    fn test_3tier_injection() {
        let mut store = LessonStore::default();
        store.add("pinned lesson");
        store.pin(&store.lessons[0].id.clone());
        store.add_with_meta("manager lesson", "manager", vec![], 0.5, "manual");
        store.add_with_meta("recent lesson", "screener", vec![], 0.5, "manual");

        let prompt = store.get_for_prompt(Some("manager"));
        assert!(prompt.contains("PINNED"));
        assert!(prompt.contains("manager lesson"));
        assert!(prompt.contains("recent lesson"));
    }
}
