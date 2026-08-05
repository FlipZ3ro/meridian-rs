use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndicatorSignalSummary {
    pub close: Option<f64>,
    pub previous_close: Option<f64>,
    pub rsi: Option<f64>,
    pub lower_band: Option<f64>,
    pub middle_band: Option<f64>,
    pub upper_band: Option<f64>,
    pub supertrend_value: Option<f64>,
    pub supertrend_direction: String,
    pub supertrend_break_up: bool,
    pub supertrend_break_down: bool,
    pub fib50: Option<f64>,
    pub fib618: Option<f64>,
    pub fib786: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndicatorEvaluation {
    pub confirmed: bool,
    pub reason: String,
    pub signal: IndicatorSignalSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndicatorIntervalResult {
    pub interval: String,
    pub ok: bool,
    pub confirmed: Option<bool>,
    pub reason: String,
    pub signal: Option<IndicatorSignalSummary>,
    pub latest: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndicatorConfirmation {
    pub enabled: bool,
    pub confirmed: bool,
    pub skipped: bool,
    pub preset: Option<String>,
    pub side: String,
    pub require_all_intervals: bool,
    pub reason: String,
    pub intervals: Vec<IndicatorIntervalResult>,
}

pub fn normalize_intervals(intervals: &[String]) -> Vec<String> {
    let source = if intervals.is_empty() {
        vec!["5_MINUTE".to_string()]
    } else {
        intervals.to_vec()
    };
    source
        .into_iter()
        .map(|value| value.trim().to_ascii_uppercase())
        .filter(|value| value == "5_MINUTE" || value == "15_MINUTE")
        .collect()
}

fn safe_num(value: Option<&Value>) -> Option<f64> {
    value.and_then(|value| match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    })
}

fn nested<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(*key)?;
    }
    Some(cursor)
}

pub fn build_signal_summary(payload: &Value) -> IndicatorSignalSummary {
    let latest = payload.get("latest").unwrap_or(&Value::Null);
    IndicatorSignalSummary {
        close: safe_num(nested(latest, &["candle", "close"])),
        previous_close: safe_num(nested(latest, &["previousCandle", "close"])),
        rsi: safe_num(nested(latest, &["rsi", "value"])),
        lower_band: safe_num(nested(latest, &["bollinger", "lower"])),
        middle_band: safe_num(nested(latest, &["bollinger", "middle"])),
        upper_band: safe_num(nested(latest, &["bollinger", "upper"])),
        supertrend_value: safe_num(nested(latest, &["supertrend", "value"])),
        supertrend_direction: nested(latest, &["supertrend", "direction"])
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        supertrend_break_up: nested(latest, &["states", "supertrendBreakUp"])
            .and_then(Value::as_bool)
            .unwrap_or(false),
        supertrend_break_down: nested(latest, &["states", "supertrendBreakDown"])
            .and_then(Value::as_bool)
            .unwrap_or(false),
        fib50: safe_num(nested(latest, &["fibonacci", "levels", "0.500"])),
        fib618: safe_num(nested(latest, &["fibonacci", "levels", "0.618"])),
        fib786: safe_num(nested(latest, &["fibonacci", "levels", "0.786"])),
    }
}

pub fn evaluate_preset(
    side: &str,
    preset: &str,
    payload: &Value,
    config: &crate::config::types::IndicatorsConfig,
) -> IndicatorEvaluation {
    let summary = build_signal_summary(payload);
    let side = side.trim().to_ascii_lowercase();
    let preset = preset.trim();
    let oversold = config.rsi_oversold;
    let overbought = config.rsi_overbought;
    let close = summary.close;
    let previous_close = summary.previous_close;
    let lower_band = summary.lower_band;
    let upper_band = summary.upper_band;
    let rsi = summary.rsi;
    let is_bullish = summary.supertrend_direction == "bullish";
    let is_bearish = summary.supertrend_direction == "bearish";
    let crossed_up = |level: Option<f64>| {
        level.is_some_and(|level| {
            close.is_some_and(|close| {
                previous_close.is_some_and(|previous| previous < level && close >= level)
            })
        })
    };
    let crossed_down = |level: Option<f64>| {
        level.is_some_and(|level| {
            close.is_some_and(|close| {
                previous_close.is_some_and(|previous| previous > level && close <= level)
            })
        })
    };

    let (confirmed, reason) = match (preset, side.as_str()) {
        ("supertrend_break", "entry") => {
            let confirmed = summary.supertrend_break_up
                || (is_bullish
                    && close.is_some()
                    && summary.supertrend_value.is_some()
                    && close >= summary.supertrend_value);
            let reason = if summary.supertrend_break_up {
                "Supertrend flipped bullish".to_string()
            } else {
                "Price is above bullish Supertrend".to_string()
            };
            (confirmed, reason)
        }
        ("supertrend_break", _) => {
            let confirmed = summary.supertrend_break_down
                || (is_bearish
                    && close.is_some()
                    && summary.supertrend_value.is_some()
                    && close <= summary.supertrend_value);
            let reason = if summary.supertrend_break_down {
                "Supertrend flipped bearish".to_string()
            } else {
                "Price is below bearish Supertrend".to_string()
            };
            (confirmed, reason)
        }
        ("rsi_reversal", "entry") => (
            rsi.is_some_and(|rsi| rsi <= oversold),
            format!(
                "RSI {} <= oversold {}",
                display_num(rsi),
                display_num(Some(oversold))
            ),
        ),
        ("rsi_reversal", _) => (
            rsi.is_some_and(|rsi| rsi >= overbought),
            format!(
                "RSI {} >= overbought {}",
                display_num(rsi),
                display_num(Some(overbought))
            ),
        ),
        ("bollinger_reversion", "entry") => (
            close.is_some() && lower_band.is_some() && close <= lower_band,
            format!(
                "Close {} <= lower band {}",
                display_num(close),
                display_num(lower_band)
            ),
        ),
        ("bollinger_reversion", _) => (
            close.is_some() && upper_band.is_some() && close >= upper_band,
            format!(
                "Close {} >= upper band {}",
                display_num(close),
                display_num(upper_band)
            ),
        ),
        ("rsi_plus_supertrend", "entry") => (
            rsi.is_some_and(|rsi| rsi <= oversold) && (summary.supertrend_break_up || is_bullish),
            "RSI oversold with bullish Supertrend context".to_string(),
        ),
        ("rsi_plus_supertrend", _) => (
            rsi.is_some_and(|rsi| rsi >= overbought)
                && (summary.supertrend_break_down || is_bearish),
            "RSI overbought with bearish Supertrend context".to_string(),
        ),
        ("supertrend_or_rsi", "entry") => (
            summary.supertrend_break_up
                || (is_bullish
                    && close.is_some()
                    && summary.supertrend_value.is_some()
                    && close >= summary.supertrend_value)
                || rsi.is_some_and(|rsi| rsi <= oversold),
            "Supertrend bullish confirmation or RSI oversold".to_string(),
        ),
        ("supertrend_or_rsi", _) => (
            summary.supertrend_break_down
                || (is_bearish
                    && close.is_some()
                    && summary.supertrend_value.is_some()
                    && close <= summary.supertrend_value)
                || rsi.is_some_and(|rsi| rsi >= overbought),
            "Supertrend bearish confirmation or RSI overbought".to_string(),
        ),
        ("bb_plus_rsi", "entry") => (
            close.is_some()
                && lower_band.is_some()
                && close <= lower_band
                && rsi.is_some_and(|rsi| rsi <= oversold),
            "Close at/below lower band with RSI oversold".to_string(),
        ),
        ("bb_plus_rsi", _) => (
            close.is_some()
                && upper_band.is_some()
                && close >= upper_band
                && rsi.is_some_and(|rsi| rsi >= overbought),
            "Close at/above upper band with RSI overbought".to_string(),
        ),
        ("fibo_reclaim", "entry") => (
            crossed_up(summary.fib618) || crossed_up(summary.fib50) || crossed_up(summary.fib786),
            "Price reclaimed a key Fibonacci level".to_string(),
        ),
        ("fibo_reclaim", _) => (
            crossed_up(summary.fib618) || crossed_up(summary.fib50),
            "Price reclaimed a key Fibonacci level upward".to_string(),
        ),
        ("fibo_reject", "entry") => (
            crossed_down(summary.fib618) || crossed_down(summary.fib50),
            "Price rejected from a key Fibonacci level".to_string(),
        ),
        ("fibo_reject", _) => (
            crossed_down(summary.fib618)
                || crossed_down(summary.fib50)
                || crossed_down(summary.fib786),
            "Price rejected below a key Fibonacci level".to_string(),
        ),
        _ => (false, format!("Unknown preset {preset}")),
    };

    IndicatorEvaluation {
        confirmed,
        reason,
        signal: summary,
    }
}

fn display_num(value: Option<f64>) -> String {
    match value {
        Some(value) if value.fract() == 0.0 => format!("{}", value as i64),
        Some(value) => format!("{}", value),
        None => "n/a".to_string(),
    }
}

pub fn confirmation_from_interval_results(
    side: &str,
    preset: &str,
    intervals: Vec<IndicatorIntervalResult>,
    config: &crate::config::types::IndicatorsConfig,
) -> IndicatorConfirmation {
    let successful: Vec<&IndicatorIntervalResult> =
        intervals.iter().filter(|entry| entry.ok).collect();
    if successful.is_empty() {
        return IndicatorConfirmation {
            enabled: true,
            confirmed: true,
            skipped: true,
            preset: Some(preset.to_string()),
            side: side.to_string(),
            require_all_intervals: config.require_all_intervals,
            reason: "Indicator API unavailable; falling back to existing logic".to_string(),
            intervals,
        };
    }

    let confirmed = if config.require_all_intervals {
        successful.iter().all(|entry| entry.confirmed == Some(true))
    } else {
        successful.iter().any(|entry| entry.confirmed == Some(true))
    };
    let reason = if confirmed {
        let confirmed_intervals = successful
            .iter()
            .filter(|entry| entry.confirmed == Some(true))
            .map(|entry| entry.interval.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{preset} confirmed on {confirmed_intervals}")
    } else {
        let checked = successful
            .iter()
            .map(|entry| entry.interval.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{preset} not confirmed on {checked}")
    };

    IndicatorConfirmation {
        enabled: true,
        confirmed,
        skipped: false,
        preset: Some(preset.to_string()),
        side: side.to_string(),
        require_all_intervals: config.require_all_intervals,
        reason,
        intervals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::IndicatorsConfig;
    use serde_json::json;

    #[test]
    fn evaluates_entry_and_exit_presets_from_chart_payload() {
        let config = IndicatorsConfig::default();
        let entry_payload = json!({
            "latest": {
                "candle": { "close": 1.20 },
                "previousCandle": { "close": 0.95 },
                "rsi": { "value": 24.0 },
                "bollinger": { "lower": 1.0, "middle": 1.1, "upper": 1.3 },
                "supertrend": { "value": 1.0, "direction": "bullish" },
                "states": { "supertrendBreakUp": true },
                "fibonacci": { "levels": { "0.500": 1.1, "0.618": 1.15, "0.786": 1.3 } }
            }
        });
        let entry = evaluate_preset("entry", "supertrend_break", &entry_payload, &config);
        assert!(entry.confirmed);
        assert_eq!(entry.reason, "Supertrend flipped bullish");

        let exit_payload = json!({
            "latest": {
                "candle": { "close": 2.0 },
                "previousCandle": { "close": 1.8 },
                "rsi": { "value": 85.0 },
                "bollinger": { "lower": 1.2, "middle": 1.5, "upper": 1.9 },
                "supertrend": { "value": 1.9, "direction": "bearish" },
                "states": { "supertrendBreakDown": false },
                "fibonacci": { "levels": { "0.500": 1.7, "0.618": 1.8, "0.786": 1.95 } }
            }
        });
        let exit = evaluate_preset("exit", "rsi_reversal", &exit_payload, &config);
        assert!(exit.confirmed);
        assert_eq!(exit.reason, "RSI 85 >= overbought 80");
    }

    #[test]
    fn confirmation_respects_any_vs_all_interval_policy() {
        let mut config = IndicatorsConfig::default();
        let intervals = vec![
            IndicatorIntervalResult {
                interval: "5_MINUTE".to_string(),
                ok: true,
                confirmed: Some(false),
                reason: "no".to_string(),
                signal: None,
                latest: None,
            },
            IndicatorIntervalResult {
                interval: "15_MINUTE".to_string(),
                ok: true,
                confirmed: Some(true),
                reason: "yes".to_string(),
                signal: None,
                latest: None,
            },
        ];

        config.require_all_intervals = false;
        let any = confirmation_from_interval_results(
            "entry",
            "supertrend_break",
            intervals.clone(),
            &config,
        );
        assert!(any.confirmed);
        assert_eq!(any.reason, "supertrend_break confirmed on 15_MINUTE");

        config.require_all_intervals = true;
        let all =
            confirmation_from_interval_results("entry", "supertrend_break", intervals, &config);
        assert!(!all.confirmed);
        assert_eq!(
            all.reason,
            "supertrend_break not confirmed on 5_MINUTE, 15_MINUTE"
        );
    }
}
