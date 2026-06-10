use chrono::{DateTime, Utc};

pub fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

pub fn timestamp_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub fn hours_since(iso_str: &str) -> Option<f64> {
    let dt = DateTime::parse_from_rfc3339(iso_str).ok()?;
    let dur = Utc::now().signed_duration_since(dt);
    Some(dur.num_milliseconds() as f64 / 3_600_000.0)
}
