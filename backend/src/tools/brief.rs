//! Daily brief: what the bot did in the last 24h, grouped by why each position
//! closed, followed by a read of what that pattern means.
//!
//! Grouping by close reason is the point. A list of trades says little; "12
//! take-profits averaging +9%, but 3 of 4 losses were out-of-range within 30
//! minutes" says the ranges are too tight, which is actionable.

use crate::state::positions::{PositionState, PositionStatus, TrackedPosition};
use chrono::{DateTime, Duration, Utc};

/// Close reasons collapsed into the handful of outcomes worth reasoning about.
/// The raw strings carry rule details ("Trailing TP: peak 7.45% -> …") that are
/// noise when counting.
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum Bucket {
    TakeProfit,
    StopLoss,
    OutOfRange,
    LowYield,
    Other,
}

impl Bucket {
    fn of(reason: &str) -> Self {
        let r = reason.to_ascii_lowercase();
        if r.contains("stop loss") || r.contains("safety exit") {
            Bucket::StopLoss
        } else if r.contains("trailing") || r.contains("take-profit") || r.contains("take profit") {
            Bucket::TakeProfit
        } else if r.contains("oor") || r.contains("out of range") || r.contains("out-of-range") {
            Bucket::OutOfRange
        } else if r.contains("low yield") {
            Bucket::LowYield
        } else {
            Bucket::Other
        }
    }
    fn label(self) -> &'static str {
        match self {
            Bucket::TakeProfit => "🎯 TAKE-PROFIT",
            Bucket::StopLoss => "🛑 STOP-LOSS",
            Bucket::OutOfRange => "↔️ OUT-OF-RANGE",
            Bucket::LowYield => "💀 FEE-MATI",
            Bucket::Other => "📄 LAINNYA",
        }
    }
}

fn held_minutes(p: &TrackedPosition) -> i64 {
    let start = DateTime::parse_from_rfc3339(&p.created_at).ok();
    let end = p
        .closed_at
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
    match (start, end) {
        (Some(a), Some(b)) => (b - a).num_minutes().max(0),
        _ => 0,
    }
}

fn fmt_dur(mins: i64) -> String {
    if mins >= 60 {
        format!("{:.1}j", mins as f64 / 60.0)
    } else {
        format!("{mins}m")
    }
}

fn name_of(p: &TrackedPosition) -> String {
    p.pool_name
        .clone()
        .or_else(|| p.base_symbol.clone())
        .unwrap_or_else(|| "?".into())
}

/// Net is PnL plus fees: fees routinely turn a red percentage into a profitable
/// trade, so scoring on PnL alone misreads the day.
fn net_of(p: &TrackedPosition) -> f64 {
    p.pnl_usd.unwrap_or(0.0) + p.all_time_fees_usd.unwrap_or(0.0)
}

pub fn render(state_path: &str) -> String {
    let state = match PositionState::load(state_path) {
        Ok(s) => s,
        Err(e) => return format!("⚠️ tidak bisa baca state: {e}"),
    };

    let cutoff = Utc::now() - Duration::hours(24);
    let all: Vec<&TrackedPosition> = state.positions.values().collect();
    let open: Vec<&TrackedPosition> = all
        .iter()
        .copied()
        .filter(|p| p.status != PositionStatus::Closed)
        .collect();
    let closed_24h: Vec<&TrackedPosition> = all
        .iter()
        .copied()
        .filter(|p| p.status == PositionStatus::Closed)
        .filter(|p| {
            p.closed_at
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.with_timezone(&Utc) >= cutoff)
                .unwrap_or(false)
        })
        .collect();

    let pnl_24h: f64 = closed_24h.iter().map(|p| p.pnl_usd.unwrap_or(0.0)).sum();
    let fees_24h: f64 = closed_24h
        .iter()
        .map(|p| p.all_time_fees_usd.unwrap_or(0.0))
        .sum();
    let wins = closed_24h.iter().filter(|p| net_of(p) > 0.0).count();
    let losses = closed_24h.len().saturating_sub(wins);
    let open_unreal: f64 = open.iter().map(|p| p.pnl_usd.unwrap_or(0.0)).sum();
    let open_fees: f64 = open
        .iter()
        .map(|p| p.all_time_fees_usd.unwrap_or(0.0))
        .sum();

    let lifetime_closed = all
        .iter()
        .filter(|p| p.status == PositionStatus::Closed)
        .count();
    let lifetime_net: f64 = all
        .iter()
        .filter(|p| p.status == PositionStatus::Closed)
        .map(|p| net_of(p))
        .sum();
    let lifetime_wins = all
        .iter()
        .filter(|p| p.status == PositionStatus::Closed)
        .filter(|p| net_of(p) > 0.0)
        .count();

    let wib = Utc::now() + Duration::hours(7);
    let mut out = format!(
        "📋 *BRIEFING HARIAN* — _{} WIB_\n\n\
         💰 *PnL 24j:* {:+.2} USD ({}W/{}L) · fee ~{:.2} USD\n\
         📗 *Terbuka:* {} · unreal {:+.2} · fee {:.2}\n\
         🏆 *Lifetime:* {} trade · {}% menang · net {:+.2} USD",
        wib.format("%Y-%m-%d %H:%M"),
        pnl_24h + fees_24h,
        wins,
        losses,
        fees_24h,
        open.len(),
        open_unreal,
        open_fees,
        lifetime_closed,
        if lifetime_closed > 0 {
            lifetime_wins * 100 / lifetime_closed
        } else {
            0
        },
        lifetime_net,
    );

    // ── Closes grouped by why ────────────────────────────────────
    out.push_str(&format!("\n\n📕 *DITUTUP 24 JAM* · {} pos", closed_24h.len()));
    for bucket in [
        Bucket::TakeProfit,
        Bucket::StopLoss,
        Bucket::OutOfRange,
        Bucket::LowYield,
        Bucket::Other,
    ] {
        let group: Vec<&&TrackedPosition> = closed_24h
            .iter()
            .filter(|p| Bucket::of(p.close_reason.as_deref().unwrap_or("")) == bucket)
            .collect();
        if group.is_empty() {
            continue;
        }
        let sum: f64 = group.iter().map(|p| net_of(p)).sum();
        out.push_str(&format!(
            "\n\n{} · {} · net {:+.2}",
            bucket.label(),
            group.len(),
            sum
        ));
        for p in group.iter().take(6) {
            out.push_str(&format!(
                "\n{} {:+.1}% · {:+.2} · {}",
                name_of(p),
                p.pnl_pct.unwrap_or(0.0),
                net_of(p),
                fmt_dur(held_minutes(p)),
            ));
        }
        if group.len() > 6 {
            out.push_str(&format!("\n…+{} lagi", group.len() - 6));
        }
    }

    // ── Open ─────────────────────────────────────────────────────
    if !open.is_empty() {
        out.push_str("\n\n📗 *POSISI TERBUKA*");
        for p in &open {
            let net = net_of(p);
            out.push_str(&format!(
                "\n{} {} {:+.2} · fee {:.2}",
                if net >= 0.0 { "🟢" } else { "🔴" },
                name_of(p),
                net,
                p.all_time_fees_usd.unwrap_or(0.0),
            ));
        }
    }

    out.push_str(&analysis(&closed_24h, &open));
    out
}

/// Read the day's shape and say what to change. Heuristics, not an LLM: each
/// line below is a claim the numbers support, so it can be trusted or argued
/// with. Silence is deliberate — no finding is better than a made-up one.
fn analysis(closed: &[&TrackedPosition], open: &[&TrackedPosition]) -> String {
    if closed.is_empty() {
        return "\n\n🧠 *ANALISA*\nBelum ada posisi ditutup 24 jam terakhir.".to_string();
    }
    let mut out = String::from("\n\n🧠 *ANALISA*");

    let tp: Vec<_> = closed
        .iter()
        .filter(|p| Bucket::of(p.close_reason.as_deref().unwrap_or("")) == Bucket::TakeProfit)
        .collect();
    let oor: Vec<_> = closed
        .iter()
        .filter(|p| Bucket::of(p.close_reason.as_deref().unwrap_or("")) == Bucket::OutOfRange)
        .collect();
    let sl: Vec<_> = closed
        .iter()
        .filter(|p| Bucket::of(p.close_reason.as_deref().unwrap_or("")) == Bucket::StopLoss)
        .collect();

    if !tp.is_empty() {
        let avg: f64 = tp.iter().map(|p| p.pnl_pct.unwrap_or(0.0)).sum::<f64>() / tp.len() as f64;
        let avg_min: i64 = tp.iter().map(|p| held_minutes(p)).sum::<i64>() / tp.len() as i64;
        out.push_str(&format!(
            "\n\n💚 *CUAN* — {} take-profit, rata-rata {:+.1}% dalam {}. \
             Entry-nya kena pas volume masih jalan.",
            tp.len(),
            avg,
            fmt_dur(avg_min)
        ));
    }

    // The most actionable failure: positions that fell out of range fast.
    // A quick OOR is not bad entry timing, it is a range too narrow for the
    // token's volatility.
    let fast_oor = oor.iter().filter(|p| held_minutes(p) <= 30).count();
    if fast_oor > 0 {
        out.push_str(&format!(
            "\n\n🔴 *LOSS* — {} dari {} posisi keluar range dalam ≤30 menit. \
             Itu bukan salah waktu masuk: range-nya kesempitan buat volatilitas token ini.",
            fast_oor,
            closed.len()
        ));
    }
    if !sl.is_empty() {
        let net_sl: f64 = sl.iter().map(|p| net_of(p)).sum();
        let positive = sl.iter().filter(|p| net_of(p) > 0.0).count();
        out.push_str(&format!(
            "\n\n🛑 *STOP-LOSS* — {} kena, net {:+.2} USD ({} di antaranya tetap plus \
             karena fee sudah menutup ruginya).",
            sl.len(),
            net_sl,
            positive
        ));
    }

    // Fixes only when the data points at one.
    let mut fixes: Vec<String> = Vec::new();
    if fast_oor >= 2 {
        fixes.push(
            "lebarkan `maxBinsBelow` atau naikkan `outOfRangeWaitMinutes` — posisi yang cepat OOR \
             butuh ruang harga balik sebelum ditutup"
                .into(),
        );
    }
    let idle = open
        .iter()
        .filter(|p| p.all_time_fees_usd.unwrap_or(0.0) == 0.0)
        .count();
    if idle >= 2 {
        fixes.push(format!(
            "{idle} posisi terbuka belum menghasilkan fee sama sekali — naikkan \
             `minFeeActiveTvlRatio` supaya pool sepi tidak lolos screening"
        ));
    }
    if !tp.is_empty() && !oor.is_empty() && oor.len() > tp.len() * 2 {
        fixes.push(
            "OOR jauh melebihi take-profit — pertimbangkan turunkan `trailingTriggerPct` \
             supaya profit terkunci sebelum harga kabur dari range"
                .into(),
        );
    }
    if !fixes.is_empty() {
        out.push_str("\n\n🔧 *FIX*");
        for f in fixes {
            out.push_str(&format!("\n• {f}"));
        }
    }
    out
}
