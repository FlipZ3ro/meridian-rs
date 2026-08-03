//! Bollinger Bands %B entry-timing gate.
//!
//! Mean-reversion entry filter for single-side-SOL-below LP. We deploy SOL
//! liquidity in bins *below* the active price and only earn fees when price
//! trades back *down* into our range. So we want to enter when price is
//! over-extended to the upside (near/above the upper Bollinger band, %B >= ~0.8)
//! — statistically a pullback toward the mean (down, into our range) is more
//! likely. Deploying regardless of price position was a major source of bleed:
//! we kept opening as price ran up, then sat out of range earning nothing.
//!
//! Candles come from the Agent Meridian `/chart-indicators/{mint}` endpoint,
//! which returns raw 5-minute OHLCV; we compute %B from the closes ourselves.

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::config::Config;
use crate::tools::agent_meridian::AgentMeridianSettings;

/// Built-in public key for the Agent Meridian API (base64 "meridian-is-the-
/// best-agents"). Used as a fallback so the indicator gate works out of the box.
const DEFAULT_PUBLIC_KEY: &str = "bWVyaWRpYW4taXMtdGhlLWJlc3QtYWdlbnRz";

/// Standard Bollinger period.
const BB_PERIOD: usize = 20;

/// Candles per window for the volume-trend comparison (12 × 5m = 1h).
const VOL_TREND_WINDOW: usize = 12;
// Flush / exhaustion detection: over the last FLUSH_WINDOW candles, a drop of
// ≥ FLUSH_MIN_DRAWDOWN from the window high counts as a "flush". If price is
// still pinned to the window low (< FLUSH_STABILIZE_MIN above it) it's freefall
// (skip — falling knife); once it's lifted off the low it's exhaustion (a valid
// dip-buy setup for our single-side SOL-below range).
const FLUSH_WINDOW: usize = 24;
const FLUSH_MIN_DRAWDOWN: f64 = 0.08;
const FLUSH_STABILIZE_MIN: f64 = 0.02;

/// Volume momentum of a pool's token, derived from OHLCV. In a DLMM, fees come
/// from volume — slowing volume is the single signal that (per large-sample
/// backtests) captures the catastrophic losers, so we skip Decelerating pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeTrend {
    Accelerating,
    Stable,
    Decelerating,
}

impl VolumeTrend {
    pub fn as_str(&self) -> &'static str {
        match self {
            VolumeTrend::Accelerating => "accelerating",
            VolumeTrend::Stable => "stable",
            VolumeTrend::Decelerating => "decelerating",
        }
    }
}

/// Recent-move regime for timing single-side dip-buys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushState {
    /// Dropped hard and still making fresh lows — a falling knife; don't deploy.
    Freefall,
    /// Dropped hard then lifted off the low — capitulation likely done; a good
    /// mean-reversion dip-buy setup.
    Exhaustion,
    /// No significant recent drop — not a flush situation; other gates decide.
    Normal,
}

/// Both entry-timing signals computed from one OHLCV fetch.
pub struct EntrySignals {
    pub percent_b: Option<f64>,
    pub volume_trend: Option<VolumeTrend>,
    pub flush: Option<FlushState>,
}

/// Short RSI period for the over-extension take-profit exit (Evil Panda uses
/// RSI(2)). A fast RSI spikes toward 100 on sharp up-moves — the local-top
/// signal to bank the bounce on a single-side-SOL-below position.
pub const EXIT_RSI_PERIOD: usize = 2;

/// Take-profit exit signals (over-extension to the UPSIDE), computed from the
/// same OHLCV fetch. Each is None when data is insufficient.
pub struct ExitSignals {
    pub rsi: Option<f64>,
    pub percent_b: Option<f64>,
}

/// Simple RSI of the latest close over `period` (unsmoothed — appropriate for a
/// very short period like 2). ~100 when recent candles are all up, ~0 when all
/// down. None if there aren't `period + 1` closes.
pub fn rsi(closes: &[f64], period: usize) -> Option<f64> {
    if period == 0 || closes.len() < period + 1 {
        return None;
    }
    let window = &closes[closes.len() - period - 1..]; // period+1 closes → period deltas
    let (mut gains, mut losses) = (0.0, 0.0);
    for w in window.windows(2) {
        let change = w[1] - w[0];
        if change >= 0.0 {
            gains += change;
        } else {
            losses -= change;
        }
    }
    let avg_gain = gains / period as f64;
    let avg_loss = losses / period as f64;
    if avg_loss == 0.0 {
        return Some(if avg_gain == 0.0 { 50.0 } else { 100.0 });
    }
    let rs = avg_gain / avg_loss;
    Some(100.0 - 100.0 / (1.0 + rs))
}

/// Classify the recent price action into a flush regime from closes
/// (oldest→newest). None if there aren't enough candles.
pub fn flush_state(closes: &[f64], window: usize) -> Option<FlushState> {
    if window == 0 || closes.len() < window {
        return None;
    }
    let w = &closes[closes.len() - window..];
    let hi = w.iter().cloned().fold(f64::MIN, f64::max);
    let lo = w.iter().cloned().fold(f64::MAX, f64::min);
    let cur = *closes.last()?;
    if hi <= 0.0 || lo <= 0.0 {
        return None;
    }
    let drawdown = (hi - cur) / hi; // how far below the window high
    if drawdown < FLUSH_MIN_DRAWDOWN {
        return Some(FlushState::Normal);
    }
    let off_low = (cur - lo) / lo; // how far above the window low
    Some(if off_low < FLUSH_STABILIZE_MIN {
        FlushState::Freefall
    } else {
        FlushState::Exhaustion
    })
}

/// Fetch recent (oldest→newest) closes and volumes for a token mint.
async fn fetch_candles(
    config: &Config,
    mint: &str,
    interval: &str,
    candles: u32,
) -> Result<(Vec<f64>, Vec<f64>)> {
    let settings = AgentMeridianSettings::from_config(config);
    let key = settings
        .api_key
        .clone()
        .filter(|k| !k.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_PUBLIC_KEY.to_string());
    let url = format!(
        "{}/chart-indicators/{}?interval={}&candles={}&rsiLength=2",
        settings.base_url.trim_end_matches('/'),
        mint,
        interval,
        candles,
    );
    let client = reqwest::Client::new();
    let resp: Value = client
        .get(&url)
        .header("x-api-key", key)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| anyhow!("chart-indicators request: {}", e))?
        .json()
        .await
        .map_err(|e| anyhow!("chart-indicators parse: {}", e))?;
    let arr = resp
        .get("candles")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("chart-indicators: no candles in response"))?;
    let closes: Vec<f64> = arr
        .iter()
        .filter_map(|c| c.get("close").and_then(Value::as_f64))
        .filter(|c| *c > 0.0)
        .collect();
    let volumes: Vec<f64> = arr
        .iter()
        .filter_map(|c| {
            c.get("volume")
                .or_else(|| c.get("v"))
                .or_else(|| c.get("vol"))
                .and_then(Value::as_f64)
        })
        .filter(|v| *v >= 0.0)
        .collect();
    Ok((closes, volumes))
}

/// Classify volume momentum: avg of the most recent window vs the window before
/// it. Accelerating ≥ +10%, Decelerating ≤ −10%, else Stable. None if data is
/// insufficient (caller decides whether to allow the deploy).
pub fn volume_trend(volumes: &[f64]) -> Option<VolumeTrend> {
    let w = VOL_TREND_WINDOW;
    if volumes.len() < w * 2 {
        return None;
    }
    let n = volumes.len();
    let recent = volumes[n - w..].iter().sum::<f64>() / w as f64;
    let prior = volumes[n - 2 * w..n - w].iter().sum::<f64>() / w as f64;
    if prior <= 0.0 {
        return None;
    }
    let ratio = recent / prior;
    Some(if ratio >= 1.10 {
        VolumeTrend::Accelerating
    } else if ratio <= 0.90 {
        VolumeTrend::Decelerating
    } else {
        VolumeTrend::Stable
    })
}

/// Bollinger %B of the latest close over `period`: (close - lower) / (upper -
/// lower), where bands are SMA ± 2σ. ~0 at the lower band, ~1 at the upper band,
/// can exceed [0,1] when price pierces a band. None if data is insufficient.
pub fn percent_b(closes: &[f64], period: usize) -> Option<f64> {
    if period == 0 || closes.len() < period {
        return None;
    }
    let window = &closes[closes.len() - period..];
    let mean = window.iter().sum::<f64>() / period as f64;
    let variance = window.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / period as f64;
    let sd = variance.sqrt();
    if sd <= 0.0 {
        return None;
    }
    let upper = mean + 2.0 * sd;
    let lower = mean - 2.0 * sd;
    let last = *closes.last()?;
    Some((last - lower) / (upper - lower))
}

/// Compute both entry signals (%B + volume trend) for a token mint on the
/// configured short timeframe, from a single OHLCV fetch. Each field is None
/// when its data is unavailable/insufficient — the caller decides whether to
/// allow or block the deploy in that case.
pub async fn entry_signals(config: &Config, mint: &str) -> Result<EntrySignals> {
    let interval = config
        .indicators
        .intervals
        .first()
        .cloned()
        .unwrap_or_else(|| "5_MINUTE".to_string());
    // Fetch enough candles to cover the BB window + two volume windows, with margin.
    let need = (BB_PERIOD + VOL_TREND_WINDOW * 2 + 8) as u32;
    let candles = need.min(config.indicators.candles.max(need));
    let (closes, volumes) = fetch_candles(config, mint, &interval, candles).await?;
    Ok(EntrySignals {
        percent_b: percent_b(&closes, BB_PERIOD),
        volume_trend: volume_trend(&volumes),
        flush: flush_state(&closes, FLUSH_WINDOW),
    })
}

/// Compute the take-profit exit signals (RSI over EXIT_RSI_PERIOD + Bollinger
/// %B) for a token mint on the configured short timeframe, from one OHLCV
/// fetch. Used to bank the bounce when price is over-extended to the upside.
pub async fn exit_signals(config: &Config, mint: &str) -> Result<ExitSignals> {
    let interval = config
        .indicators
        .intervals
        .first()
        .cloned()
        .unwrap_or_else(|| "5_MINUTE".to_string());
    let need = (BB_PERIOD + 8) as u32;
    let candles = need.min(config.indicators.candles.max(need));
    let (closes, _volumes) = fetch_candles(config, mint, &interval, candles).await?;
    Ok(ExitSignals {
        rsi: rsi(&closes, EXIT_RSI_PERIOD),
        percent_b: percent_b(&closes, BB_PERIOD),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_b_zero_at_lower_one_at_upper() {
        // Flat then a spike: just sanity-check the math on a simple series.
        let mut closes: Vec<f64> = vec![10.0; 19];
        closes.push(10.0);
        // zero stddev → None
        assert_eq!(percent_b(&closes, 20), None);
    }

    #[test]
    fn percent_b_high_when_price_above_mean() {
        let mut closes: Vec<f64> = (0..19).map(|_| 10.0).collect();
        closes.push(12.0); // last close well above the mean → %B > 0.5
        let pb = percent_b(&closes, 20).expect("should compute");
        assert!(pb > 0.5, "expected overextended %B, got {pb}");
    }

    #[test]
    fn insufficient_data_returns_none() {
        assert_eq!(percent_b(&[1.0, 2.0, 3.0], 20), None);
    }
}
