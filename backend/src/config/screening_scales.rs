/// Timeframe-scaled screening defaults matching the original Node.js Meridian
/// `screening-scales.js` helper. Pool discovery volume and fee/active-TVL are
/// window-dependent, so setup/default values must be scaled to the selected
/// API timeframe.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreeningTimeframeDefaults {
    pub timeframe: String,
    pub min_fee_active_tvl_ratio: f64,
    pub min_volume: f64,
}

const DEFAULT_TIMEFRAME: &str = "4h";
const TIMEFRAME_SCREENING_SCALES: &[(&str, f64, f64)] = &[
    ("5m", 0.02, 500.0),
    ("30m", 0.15, 1_000.0),
    ("1h", 0.2, 10_000.0),
    ("2h", 0.4, 20_000.0),
    ("4h", 0.4, 2_000.0),
    ("12h", 1.5, 60_000.0),
    ("24h", 2.0, 10_000.0),
];

pub fn normalize_timeframe(timeframe: &str) -> String {
    let normalized = timeframe.trim().to_ascii_lowercase();
    if TIMEFRAME_SCREENING_SCALES
        .iter()
        .any(|(tf, _, _)| *tf == normalized)
    {
        normalized
    } else {
        DEFAULT_TIMEFRAME.to_string()
    }
}

pub fn screening_defaults_for_timeframe(timeframe: &str) -> ScreeningTimeframeDefaults {
    let normalized = normalize_timeframe(timeframe);
    let (_, min_fee_active_tvl_ratio, min_volume) = TIMEFRAME_SCREENING_SCALES
        .iter()
        .find(|(tf, _, _)| *tf == normalized)
        .copied()
        .expect("normalized timeframe has defaults");
    ScreeningTimeframeDefaults {
        timeframe: normalized,
        min_fee_active_tvl_ratio,
        min_volume,
    }
}

pub fn scale_screening_to_timeframe(timeframe: &str) -> (f64, f64) {
    let defaults = screening_defaults_for_timeframe(timeframe);
    (defaults.min_fee_active_tvl_ratio, defaults.min_volume)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_timeframe_and_returns_js_scaled_defaults() {
        assert_eq!(normalize_timeframe("5M"), "5m");
        assert_eq!(normalize_timeframe("unknown"), "4h");

        let five = screening_defaults_for_timeframe("5m");
        assert_eq!(five.timeframe, "5m");
        assert_eq!(five.min_fee_active_tvl_ratio, 0.02);
        assert_eq!(five.min_volume, 500.0);

        let one_hour = screening_defaults_for_timeframe("1h");
        assert_eq!(one_hour.min_fee_active_tvl_ratio, 0.2);
        assert_eq!(one_hour.min_volume, 10_000.0);

        let defaulted = screening_defaults_for_timeframe("bad-timeframe");
        assert_eq!(defaulted.timeframe, "4h");
        assert_eq!(defaulted.min_fee_active_tvl_ratio, 0.4);
        assert_eq!(defaulted.min_volume, 2_000.0);
    }
}
