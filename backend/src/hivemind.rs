//! HiveMind shared-learning sync (Agent Meridian).
//!
//! Port of the original Node.js `hivemind.js`. Registers the agent, pulls
//! shared lessons/presets into a local cache, and pushes closed-position
//! performance + lesson events. All network failures are non-blocking — the
//! agent logs a warning and keeps running. Private keys and wallet balances are
//! never sent.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::meridian_data_path;
use crate::config::types::HiveMindConfig;
use crate::utils::logger::module;

const HEARTBEAT_INTERVAL_SECS: u64 = 15 * 60;
const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ─── Paths ──────────────────────────────────────────────────────

fn cache_path() -> PathBuf {
    meridian_data_path("hivemind-cache.json")
}

fn agent_id_path() -> PathBuf {
    meridian_data_path("hivemind-agent.json")
}

// ─── Cache model ────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HiveCache {
    #[serde(default)]
    pub shared_lessons: Vec<SharedLesson>,
    #[serde(default)]
    pub presets: Vec<Value>,
    #[serde(default)]
    pub pulled_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedLesson {
    pub id: String,
    pub rule: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub source_type: String,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub created_at: String,
}

fn read_cache() -> HiveCache {
    std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn write_cache(cache: &HiveCache) {
    if let Ok(content) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::write(cache_path(), content);
    }
}

// ─── Helpers ────────────────────────────────────────────────────

/// Strip control chars / angle brackets and clamp length (mirrors JS sanitize).
fn sanitize_text(text: &str, max_len: usize) -> Option<String> {
    let cleaned: String = text
        .chars()
        .map(|c| match c {
            '\r' | '\n' | '\t' => ' ',
            '<' | '>' | '`' => '\u{0}',
            other => other,
        })
        .filter(|c| *c != '\u{0}')
        .collect();
    // Collapse runs of whitespace.
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed: String = collapsed.chars().take(max_len).collect();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn number_or_null(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn url_non_empty(config: &HiveMindConfig) -> Option<String> {
    config
        .url
        .as_deref()
        .and_then(|s| sanitize_text(s, 500))
        .filter(|s| !s.is_empty())
}

fn api_key(config: &HiveMindConfig) -> Option<String> {
    config
        .api_key
        .as_deref()
        .and_then(|s| sanitize_text(s, 300))
        .filter(|s| !s.is_empty())
}

/// HiveMind is active only when both a base URL and API key are configured.
pub fn is_enabled(config: &HiveMindConfig) -> bool {
    url_non_empty(config).is_some() && api_key(config).is_some()
}

fn pull_mode(config: &HiveMindConfig) -> String {
    match sanitize_text(&config.pull_mode, 20).as_deref() {
        Some("manual") => "manual".to_string(),
        _ => "auto".to_string(),
    }
}

// ─── Agent identity ─────────────────────────────────────────────

/// Resolve the persistent agent id, generating + persisting one if absent.
/// Precedence: config value → `hivemind-agent.json` → freshly generated.
pub fn ensure_agent_id(config: &HiveMindConfig) -> String {
    if let Some(id) = config.agent_id.as_deref().filter(|s| !s.trim().is_empty()) {
        return id.to_string();
    }
    if let Some(id) = std::fs::read_to_string(agent_id_path())
        .ok()
        .and_then(|c| serde_json::from_str::<Value>(&c).ok())
        .and_then(|v| v.get("agentId").and_then(Value::as_str).map(str::to_string))
        .filter(|s| !s.is_empty())
    {
        return id;
    }

    let id = generate_agent_id();
    let _ = std::fs::write(agent_id_path(), json!({ "agentId": id }).to_string());
    module::info("hivemind", &format!("Generated agentId {}", id));
    id
}

fn generate_agent_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64;
    // 24 hex chars, mirroring the JS `agt_<12 random bytes>` shape.
    format!(
        "agt_{:012x}{:012x}",
        ts & 0xffff_ffff_ffff,
        seq & 0xffff_ffff_ffff
    )
}

// ─── HTTP ───────────────────────────────────────────────────────

fn build_url(base: &str, pathname: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), pathname)
}

async fn request_json(
    config: &HiveMindConfig,
    pathname: &str,
    method: reqwest::Method,
    body: Option<Value>,
    query: &[(&str, String)],
) -> anyhow::Result<Value> {
    let base = url_non_empty(config).ok_or_else(|| anyhow::anyhow!("HiveMind not enabled"))?;
    let key = api_key(config).ok_or_else(|| anyhow::anyhow!("HiveMind API key missing"))?;

    let client = reqwest::Client::new();
    let mut req = client
        .request(method, build_url(&base, pathname))
        .header("accept", "application/json")
        .header("x-api-key", key);
    if !query.is_empty() {
        req = req.query(query);
    }
    if let Some(ref payload) = body {
        req = req.header("content-type", "application/json").json(payload);
    }

    let resp = req.send().await?;
    let status = resp.status();
    let payload: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = payload
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("HiveMind {}", status));
        anyhow::bail!(msg);
    }
    Ok(payload)
}

// ─── Shared lessons (pull side, prompt injection) ───────────────

fn normalize_shared_lesson(value: &Value) -> Option<SharedLesson> {
    let rule = sanitize_text(value.get("rule").and_then(Value::as_str).unwrap_or(""), 400)?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| value.get("lessonId").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| format!("shared_{}", chrono::Utc::now().timestamp_millis()));
    let tags = value
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().and_then(|s| sanitize_text(s, 48)))
                .collect()
        })
        .unwrap_or_default();
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .and_then(|s| sanitize_text(s, 20));
    let outcome = value
        .get("outcome")
        .and_then(Value::as_str)
        .and_then(|s| sanitize_text(s, 20))
        .unwrap_or_else(|| "shared".to_string());
    let source_type = value
        .get("sourceType")
        .and_then(Value::as_str)
        .or_else(|| value.get("source").and_then(Value::as_str))
        .and_then(|s| sanitize_text(s, 24))
        .unwrap_or_else(|| "shared".to_string());
    let created_at = value
        .get("created_at")
        .and_then(Value::as_str)
        .or_else(|| value.get("createdAt").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(now_rfc3339);

    Some(SharedLesson {
        id,
        rule,
        tags,
        role,
        outcome,
        source_type,
        score: number_or_null(value.get("score")),
        created_at,
    })
}

/// Build the `── HIVEMIND ──` prompt block from cached shared lessons, filtered
/// by agent role and ranked by score. Returns `None` when nothing applies.
pub fn shared_lessons_for_prompt(role: &str, max_lessons: usize) -> Option<String> {
    let role_upper = role.to_ascii_uppercase();
    let mut shared: Vec<SharedLesson> = read_cache()
        .shared_lessons
        .into_iter()
        .filter(|lesson| match lesson.role.as_deref() {
            None => true,
            Some(r) => r.eq_ignore_ascii_case(&role_upper) || role_upper == "GENERAL",
        })
        .collect();
    shared.sort_by(|a, b| {
        b.score
            .unwrap_or(0.0)
            .partial_cmp(&a.score.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    shared.truncate(max_lessons);

    if shared.is_empty() {
        return None;
    }
    let lines: Vec<String> = shared
        .iter()
        .map(|lesson| {
            let score = lesson
                .score
                .map(|s| format!(" score={}", s))
                .unwrap_or_default();
            format!("[HIVEMIND{}] {}", score, lesson.rule)
        })
        .collect();
    Some(format!("── HIVEMIND ──\n{}", lines.join("\n")))
}

// ─── Register / pull ────────────────────────────────────────────

pub async fn register_agent(config: &HiveMindConfig, reason: &str) -> Option<Value> {
    if !is_enabled(config) {
        return None;
    }
    let body = json!({
        "agentId": ensure_agent_id(config),
        "version": AGENT_VERSION,
        "timestamp": now_rfc3339(),
        "reason": reason,
        "capabilities": {
            "telegram": std::env::var("TELEGRAM_BOT_TOKEN").map(|v| !v.is_empty()).unwrap_or(false),
            "lpagent": std::env::var("LPAGENT_API_KEY").map(|v| !v.is_empty()).unwrap_or(false),
            "dryRun": std::env::var("DRY_RUN").as_deref() == Ok("true"),
        },
    });
    match request_json(
        config,
        "/api/hivemind/agents/register",
        reqwest::Method::POST,
        Some(body),
        &[],
    )
    .await
    {
        Ok(payload) => Some(payload),
        Err(e) => {
            module::warn("hivemind", &format!("Agent register failed: {}", e));
            None
        }
    }
}

pub async fn pull_lessons(config: &HiveMindConfig, limit: u32) -> Option<Vec<SharedLesson>> {
    if !is_enabled(config) {
        return None;
    }
    let query = [
        ("agentId", ensure_agent_id(config)),
        ("limit", limit.to_string()),
    ];
    match request_json(
        config,
        "/api/hivemind/lessons/pull",
        reqwest::Method::GET,
        None,
        &query,
    )
    .await
    {
        Ok(payload) => {
            let lessons: Vec<SharedLesson> = payload
                .get("lessons")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(normalize_shared_lesson).collect())
                .unwrap_or_default();
            let mut cache = read_cache();
            cache.shared_lessons = lessons.clone();
            cache.pulled_at = Some(now_rfc3339());
            write_cache(&cache);
            Some(lessons)
        }
        Err(e) => {
            module::warn("hivemind", &format!("Lesson pull failed: {}", e));
            None
        }
    }
}

pub async fn pull_presets(config: &HiveMindConfig) -> Option<Vec<Value>> {
    if !is_enabled(config) {
        return None;
    }
    let query = [("agentId", ensure_agent_id(config))];
    match request_json(
        config,
        "/api/hivemind/presets/pull",
        reqwest::Method::GET,
        None,
        &query,
    )
    .await
    {
        Ok(payload) => {
            let presets = payload
                .get("presets")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut cache = read_cache();
            cache.presets = presets.clone();
            cache.pulled_at = Some(now_rfc3339());
            write_cache(&cache);
            Some(presets)
        }
        Err(e) => {
            module::warn("hivemind", &format!("Preset pull failed: {}", e));
            None
        }
    }
}

// ─── Push side ──────────────────────────────────────────────────

/// A closed-position performance event to broadcast to the HiveMind.
#[derive(Debug, Clone, Default)]
pub struct PerformanceEvent {
    pub event_id: Option<String>,
    pub pool: Option<String>,
    pub pool_name: Option<String>,
    pub base_mint: Option<String>,
    pub strategy: Option<String>,
    pub close_reason: Option<String>,
    pub pnl_usd: f64,
    pub pnl_pct: f64,
    pub fees_earned_usd: f64,
    pub fees_earned_sol: f64,
    pub minutes_held: f64,
}

fn count_in_adjusted_win_rate(close_reason: Option<&str>) -> bool {
    let text = close_reason.unwrap_or("").to_ascii_lowercase();
    !(text.contains("out of range")
        || text.contains("pumped far above range")
        || text.contains("oor"))
}

pub async fn push_performance_event(
    config: &HiveMindConfig,
    ev: &PerformanceEvent,
) -> Option<Value> {
    if !is_enabled(config) {
        return None;
    }
    let agent_id = ensure_agent_id(config);
    let event_id = ev.event_id.clone().unwrap_or_else(|| {
        format!(
            "close:{}:{}:{}",
            agent_id,
            ev.pool.as_deref().unwrap_or("unknown"),
            chrono::Utc::now().timestamp_millis()
        )
    });
    let body = json!({
        "eventId": event_id,
        "agentId": agent_id,
        "version": AGENT_VERSION,
        "timestamp": now_rfc3339(),
        "event": {
            "pool": ev.pool.as_deref().and_then(|s| sanitize_text(s, 64)),
            "poolName": ev.pool_name.as_deref().and_then(|s| sanitize_text(s, 80)),
            "baseMint": ev.base_mint.as_deref().and_then(|s| sanitize_text(s, 64)),
            "strategy": ev.strategy.as_deref().and_then(|s| sanitize_text(s, 32)),
            "closeReason": ev.close_reason.as_deref().and_then(|s| sanitize_text(s, 200)).unwrap_or_else(|| "unknown".to_string()),
            "pnlUsd": ev.pnl_usd,
            "pnlPct": ev.pnl_pct,
            "feesUsd": ev.fees_earned_usd,
            "feesSol": ev.fees_earned_sol,
            "minutesHeld": ev.minutes_held,
            "countInAdjustedWinRate": count_in_adjusted_win_rate(ev.close_reason.as_deref()),
        },
    });
    match request_json(
        config,
        "/api/hivemind/performance/push",
        reqwest::Method::POST,
        Some(body),
        &[],
    )
    .await
    {
        Ok(payload) => Some(payload),
        Err(e) => {
            module::warn("hivemind", &format!("Performance push failed: {}", e));
            None
        }
    }
}

// ─── Lifecycle ──────────────────────────────────────────────────

/// Register on startup and (in auto mode) pull shared lessons + presets.
pub async fn bootstrap(config: &HiveMindConfig) {
    if !is_enabled(config) {
        return;
    }
    ensure_agent_id(config);
    let auto = pull_mode(config) == "auto";
    let _ = register_agent(config, "startup").await;
    if auto {
        let _ = pull_lessons(config, 12).await;
        let _ = pull_presets(config).await;
    }
    module::info(
        "hivemind",
        &format!(
            "HiveMind enabled (agentId {}, pullMode {})",
            ensure_agent_id(config),
            pull_mode(config)
        ),
    );
}

/// Run one heartbeat tick: register + (auto) pull. Used by the background loop.
pub async fn heartbeat_tick(config: &HiveMindConfig) {
    if !is_enabled(config) {
        return;
    }
    let _ = register_agent(config, "heartbeat").await;
    if pull_mode(config) == "auto" {
        let _ = pull_lessons(config, 12).await;
        let _ = pull_presets(config).await;
    }
}

/// Interval (seconds) for the background heartbeat loop.
pub fn heartbeat_interval_secs() -> u64 {
    HEARTBEAT_INTERVAL_SECS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_brackets_and_collapses_whitespace() {
        assert_eq!(
            sanitize_text("  hello\n<world>  foo ", 100).as_deref(),
            Some("hello world foo")
        );
        assert_eq!(sanitize_text("   ", 100), None);
        assert_eq!(sanitize_text("abcdef", 3).as_deref(), Some("abc"));
    }

    #[test]
    fn enabled_requires_url_and_key() {
        let mut cfg = HiveMindConfig::default();
        assert!(!is_enabled(&cfg));
        cfg.url = Some("https://api.agentmeridian.xyz".to_string());
        assert!(!is_enabled(&cfg));
        cfg.api_key = Some("k".to_string());
        assert!(is_enabled(&cfg));
    }

    #[test]
    fn pull_mode_normalizes() {
        let mut cfg = HiveMindConfig::default();
        assert_eq!(pull_mode(&cfg), "auto");
        cfg.pull_mode = "manual".to_string();
        assert_eq!(pull_mode(&cfg), "manual");
        cfg.pull_mode = "weird".to_string();
        assert_eq!(pull_mode(&cfg), "auto");
    }

    #[test]
    fn normalize_shared_lesson_requires_rule() {
        assert!(normalize_shared_lesson(&json!({})).is_none());
        let lesson = normalize_shared_lesson(&json!({
            "rule": "avoid low organic",
            "role": "screener",
            "score": 3.5,
            "tags": ["screening"]
        }))
        .expect("valid lesson");
        assert_eq!(lesson.rule, "avoid low organic");
        assert_eq!(lesson.score, Some(3.5));
        assert_eq!(lesson.role.as_deref(), Some("screener"));
    }

    #[test]
    fn adjusted_win_rate_excludes_oor() {
        assert!(count_in_adjusted_win_rate(Some("low yield")));
        assert!(!count_in_adjusted_win_rate(Some("out of range")));
        assert!(!count_in_adjusted_win_rate(Some("OOR exit")));
    }

    #[test]
    fn agent_ids_are_unique_and_prefixed() {
        let a = generate_agent_id();
        let b = generate_agent_id();
        assert!(a.starts_with("agt_"));
        assert_ne!(a, b);
    }
}
