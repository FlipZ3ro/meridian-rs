use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

use crate::config::Config;
use crate::tools::agent_meridian::AgentMeridianSettings;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StudyTopLpersResult {
    pub pool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_name: Option<String>,
    pub message: String,
    pub patterns: Value,
    pub lpers: Vec<Value>,
}

pub async fn study_top_lpers(
    pool_address: &str,
    limit: usize,
    config: &Config,
) -> Result<StudyTopLpersResult> {
    if pool_address.trim().is_empty() {
        return Err(anyhow!("pool_address required"));
    }

    if config.dry_run {
        return Ok(build_study_from_payloads(
            pool_address,
            limit,
            &json!({"topLpers": [], "historicalOwners": []}),
            &json!({}),
        ));
    }

    let settings = AgentMeridianSettings::from_config(config);
    let encoded_pool = urlencoding::encode(pool_address);
    let pool_data = fetch_agent_meridian_json(&settings, &format!("/top-lp/{encoded_pool}"))
        .await
        .with_context(|| format!("failed to fetch top LPers for pool {pool_address}"))?;
    let signal_data =
        fetch_agent_meridian_json(&settings, &format!("/study-top-lp/{encoded_pool}"))
            .await
            .with_context(|| format!("failed to fetch top LPer study for pool {pool_address}"))?;

    Ok(build_study_from_payloads(
        pool_address,
        limit,
        &pool_data,
        &signal_data,
    ))
}

async fn fetch_agent_meridian_json(settings: &AgentMeridianSettings, path: &str) -> Result<Value> {
    let url = format!("{}{}", settings.base_url.trim_end_matches('/'), path);
    let client = reqwest::Client::new();
    let mut request = client.get(&url);
    if let Some(key) = settings.api_key.as_deref() {
        request = request.header("x-api-key", key);
    }
    let response = request.send().await?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let payload: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw": text}));
    if !status.is_success() {
        return Err(anyhow!(
            "Agent Meridian {} returned {}: {}",
            path,
            status,
            payload
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("request failed")
        ));
    }
    Ok(payload)
}

pub fn build_study_from_payloads(
    pool_address: &str,
    limit: usize,
    pool_data: &Value,
    signal_data: &Value,
) -> StudyTopLpersResult {
    let top_lpers = value_array(pool_data, "topLpers");
    let historical_owners = value_array(pool_data, "historicalOwners");
    let take = limit.max(1);
    let ranked: Vec<Value> = top_lpers.iter().take(take).cloned().collect();

    if ranked.is_empty() {
        return StudyTopLpersResult {
            pool: pool_address.to_string(),
            pool_name: None,
            message: "No LPAgent top LPer data found for this pool yet.".to_string(),
            patterns: json!({}),
            lpers: vec![],
        };
    }

    let historical_map: BTreeMap<String, Value> = historical_owners
        .iter()
        .filter_map(|owner| string_field(owner, "owner").map(|id| (id, owner.clone())))
        .collect();

    let pool_name = pool_name(pool_data);
    let lpers = ranked
        .iter()
        .map(|owner| build_lper(pool_address, owner, &historical_map, pool_name.as_deref()))
        .collect::<Vec<_>>();
    let patterns = build_patterns(&ranked, &historical_owners, signal_data, pool_data);

    StudyTopLpersResult {
        pool: pool_address.to_string(),
        pool_name,
        message: "LPAgent-backed top LP study from Agent Meridian 30m cached owner aggregates plus owner historical positions."
            .to_string(),
        patterns,
        lpers,
    }
}

fn build_lper(
    pool_address: &str,
    owner: &Value,
    historical_map: &BTreeMap<String, Value>,
    pool_name: Option<&str>,
) -> Value {
    let owner_id = string_field(owner, "owner").unwrap_or_default();
    let history = historical_map.get(&owner_id);
    let preferred_strategy = history
        .and_then(|h| string_field(h, "preferredStrategy"))
        .unwrap_or_else(|| "unknown".to_string());
    let preferred_range = history
        .and_then(|h| string_field(h, "preferredRangeStyle"))
        .unwrap_or_else(|| "unknown".to_string());
    let signal_tags = [
        history
            .and_then(|h| string_field(h, "preferredStrategy"))
            .map(|strategy| format!("strategy:{strategy}")),
        history
            .and_then(|h| string_field(h, "preferredRangeStyle"))
            .map(|range| format!("range:{range}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    let positions = history
        .and_then(|h| h.get("topPositions"))
        .and_then(Value::as_array)
        .map(|positions| {
            positions
                .iter()
                .map(|position| {
                    json!({
                        "pool": pool_address,
                        "pair": pool_name.unwrap_or("Unknown pool"),
                        "hold_hours": round(num_field(position, "ageHours"), 2),
                        "pnl_usd": round(num_field(position, "pnlUsd"), 2),
                        "pnl_pct": fmt_pct(num_field(position, "pnlPct")),
                        "fee_usd": round(num_field(position, "feeUsd"), 2),
                        "in_range_pct": position.get("inRange").and_then(Value::as_bool).map(|in_range| if in_range { 100 } else { 0 }),
                        "strategy": string_field(position, "strategy"),
                        "closed_reason": string_field(position, "rangeStyle"),
                        "balance_usd": round(num_field(position, "inputValue"), 2),
                        "fee_per_tvl_24h_pct": round(num_field(position, "feePercent"), 2),
                        "range_width_pct": position.get("widthBins").and_then(Value::as_i64),
                        "distance_to_active_pct": Value::Null,
                        "lower_bin_id": position.get("lowerBinId").and_then(Value::as_i64),
                        "upper_bin_id": position.get("upperBinId").and_then(Value::as_i64),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    json!({
        "owner": owner_id,
        "owner_short": string_field(owner, "ownerShort").unwrap_or_else(|| owner_short(&string_field(owner, "owner").unwrap_or_default())),
        "signal_tags": signal_tags,
        "summary": {
            "total_positions": owner.get("totalLp").and_then(Value::as_u64).or_else(|| history.and_then(|h| h.get("topPositions")).and_then(Value::as_array).map(|items| items.len() as u64)).unwrap_or(0),
            "avg_hold_hours": round(num_field(owner, "avgAgeHours").or_else(|| history.and_then(|h| num_field(h, "avgHoldHours"))), 2),
            "avg_open_pnl_pct": round(num_field(owner, "pnlPerInflowPct").or_else(|| history.and_then(|h| num_field(h, "avgPnlPct"))), 2),
            "avg_fee_per_tvl_24h_pct": round(num_field(owner, "feePercent").or_else(|| history.and_then(|h| num_field(h, "avgFeePercent"))), 2),
            "total_pnl_usd": round(num_field(owner, "totalPnlUsd"), 2),
            "total_balance_usd": round(num_field(owner, "totalInflowUsd"), 2),
            "avg_range_width_pct": Value::Null,
            "avg_distance_to_active_pct": Value::Null,
            "win_rate": round(num_field(owner, "winRatePct").map(|value| value / 100.0), 2),
            "roi": round(num_field(owner, "roiPct").map(|value| value / 100.0), 4),
            "fee_pct_of_capital": round(num_field(owner, "feePercent"), 2),
            "preferred_strategy": preferred_strategy,
            "preferred_range_style": preferred_range,
        },
        "positions": positions,
    })
}

fn build_patterns(
    ranked: &[Value],
    historical_owners: &[Value],
    signal_data: &Value,
    pool_data: &Value,
) -> Value {
    json!({
        "top_lper_count": ranked.len(),
        "study_mode": "lpagent_top_lpers",
        "pool_name": pool_name(pool_data).unwrap_or_else(|| "TOKEN-SOL".to_string()),
        "active_position_count": signal_data.get("activePositionCount").and_then(Value::as_u64).unwrap_or(ranked.len() as u64),
        "owner_count": signal_data.get("ownerCount").and_then(Value::as_u64).unwrap_or(ranked.len() as u64),
        "avg_hold_hours": round(avg(ranked.iter().filter_map(|item| num_field(item, "avgAgeHours"))), 2),
        "avg_open_pnl_pct": round(avg(ranked.iter().filter_map(|item| num_field(item, "pnlPerInflowPct"))), 2),
        "avg_fee_percent": round(avg(ranked.iter().filter_map(|item| num_field(item, "feePercent"))), 2),
        "avg_roi_pct": round(avg(ranked.iter().filter_map(|item| num_field(item, "roiPct"))), 2),
        "best_open_pnl_pct": ranked.first().map(|item| format!("{}%", round(num_field(item, "pnlPerInflowPct"), 2))),
        "scalper_count": ranked.iter().filter(|item| num_field(item, "avgAgeHours").unwrap_or(0.0) < 1.0).count(),
        "holder_count": ranked.iter().filter(|item| num_field(item, "avgAgeHours").unwrap_or(0.0) >= 4.0).count(),
        "preferred_strategies": count_values(historical_owners.iter().filter_map(|item| string_field(item, "preferredStrategy"))),
        "preferred_range_styles": count_values(historical_owners.iter().filter_map(|item| string_field(item, "preferredRangeStyle"))),
        "top_historical_owners": signal_data.get("topHistoricalOwners").and_then(Value::as_array).map(|items| items.iter().take(3).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "suggested_style": signal_data.get("suggestedStyle").cloned().unwrap_or(Value::Null),
    })
}

fn pool_name(pool_data: &Value) -> Option<String> {
    let overview = pool_data.get("overview")?;
    string_field(overview, "name").or_else(|| {
        let x = string_field(overview, "tokenXSymbol").unwrap_or_else(|| "TOKEN".to_string());
        let y = string_field(overview, "tokenYSymbol").unwrap_or_else(|| "SOL".to_string());
        Some(format!("{x}-{y}"))
    })
}

fn value_array(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn num_field(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(|value| match value {
            Value::Number(number) => number.as_f64(),
            Value::String(text) => text.parse::<f64>().ok(),
            _ => None,
        })
        .filter(|value| value.is_finite())
}

fn avg(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut count = 0usize;
    let mut sum = 0.0;
    for value in values {
        count += 1;
        sum += value;
    }
    (count > 0).then_some(sum / count as f64)
}

fn round(value: Option<f64>, digits: u32) -> f64 {
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return 0.0;
    };
    let factor = 10_f64.powi(digits as i32);
    (value * factor).round() / factor
}

fn fmt_pct(value: Option<f64>) -> String {
    let value = round(value, 2);
    if value >= 0.0 {
        format!("+{value}%")
    } else {
        format!("{value}%")
    }
}

fn count_values(values: impl Iterator<Item = String>) -> Value {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    let mut sorted = counts.into_iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    Value::Object(
        sorted
            .into_iter()
            .map(|(key, count)| (key, Value::Number(count.into())))
            .collect::<Map<_, _>>(),
    )
}

fn owner_short(owner: &str) -> String {
    let prefix: String = owner.chars().take(8).collect();
    if prefix.is_empty() {
        "unknown".to_string()
    } else {
        format!("{prefix}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_top_lper_patterns_from_agent_meridian_payloads() {
        let pool_data = serde_json::json!({
            "overview": {"name": "PANDA/SOL", "tokenXSymbol": "PANDA", "tokenYSymbol": "SOL"},
            "topLpers": [
                {
                    "owner": "Owner1111111111111111111111111111111111111",
                    "ownerShort": "Owner111...",
                    "totalLp": 3,
                    "avgAgeHours": 0.75,
                    "pnlPerInflowPct": 12.345,
                    "feePercent": 1.234,
                    "roiPct": 9.87,
                    "winRatePct": 80,
                    "totalPnlUsd": 42.42,
                    "totalInflowUsd": 420.0
                },
                {
                    "owner": "Owner2222222222222222222222222222222222222",
                    "totalLp": 2,
                    "avgAgeHours": 5.5,
                    "pnlPerInflowPct": -1.0,
                    "feePercent": 0.5,
                    "roiPct": -0.25,
                    "winRatePct": 25,
                    "totalPnlUsd": -5,
                    "totalInflowUsd": 200.0
                }
            ],
            "historicalOwners": [
                {
                    "owner": "Owner1111111111111111111111111111111111111",
                    "preferredStrategy": "bid_ask",
                    "preferredRangeStyle": "narrow",
                    "topPositions": [
                        {
                            "ageHours": 0.5,
                            "pnlUsd": 10.0,
                            "pnlPct": 3.21,
                            "feeUsd": 1.5,
                            "inRange": true,
                            "strategy": "bid_ask",
                            "rangeStyle": "narrow",
                            "inputValue": 100.0,
                            "feePercent": 1.5,
                            "widthBins": 20,
                            "lowerBinId": 100,
                            "upperBinId": 120
                        }
                    ]
                },
                {
                    "owner": "Owner2222222222222222222222222222222222222",
                    "preferredStrategy": "spot",
                    "preferredRangeStyle": "wide"
                }
            ]
        });
        let signal_data = serde_json::json!({
            "activePositionCount": 8,
            "ownerCount": 6,
            "suggestedStyle": "narrow_bid_ask",
            "topHistoricalOwners": ["Owner1111111111111111111111111111111111111"]
        });

        let result = build_study_from_payloads("Pool111", 2, &pool_data, &signal_data);

        assert_eq!(result.pool, "Pool111");
        assert_eq!(result.pool_name.as_deref(), Some("PANDA/SOL"));
        assert_eq!(result.patterns["top_lper_count"], 2);
        assert_eq!(result.patterns["avg_hold_hours"], 3.13);
        assert_eq!(result.patterns["scalper_count"], 1);
        assert_eq!(result.patterns["holder_count"], 1);
        assert_eq!(result.patterns["preferred_strategies"]["bid_ask"], 1);
        assert_eq!(result.patterns["suggested_style"], "narrow_bid_ask");
        assert_eq!(result.lpers.len(), 2);
        assert_eq!(result.lpers[0]["signal_tags"][0], "strategy:bid_ask");
        assert_eq!(result.lpers[0]["summary"]["win_rate"], 0.8);
        assert_eq!(result.lpers[0]["positions"][0]["pnl_pct"], "+3.21%");
    }

    #[test]
    fn no_top_lpers_returns_js_compatible_empty_result() {
        let result = build_study_from_payloads(
            "PoolNoData",
            4,
            &serde_json::json!({"topLpers": [], "historicalOwners": []}),
            &serde_json::json!({}),
        );

        assert_eq!(result.pool, "PoolNoData");
        assert!(result.message.contains("No LPAgent top LPer data"));
        assert!(result.patterns.as_object().unwrap().is_empty());
        assert!(result.lpers.is_empty());
    }
}
