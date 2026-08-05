/// Round to N decimal places
pub fn round_to(value: f64, decimals: u32) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    (value * factor).round() / factor
}

/// Percentage change
pub fn pct_change(from: f64, to: f64) -> f64 {
    if from == 0.0 {
        return 0.0;
    }
    ((to - from) / from) * 100.0
}

/// Safe division
pub fn safe_div(a: f64, b: f64) -> f64 {
    if b == 0.0 {
        0.0
    } else {
        a / b
    }
}
