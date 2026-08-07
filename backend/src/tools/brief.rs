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

/// Score a position on its price leg alone.
///
/// This used to add all_time_fees_usd on top, which double-counted: pnl already
/// contains the fees. Meteora's own numbers for a GOBLIN-SOL close settle it —
/// deposit $36.91, withdraw $33.20, fees earned $1.42, and the reported PnL of
/// -$2.30 is exactly -3.71 + 1.42. Adding the fee term again is why a session
/// scored at +$40 left the wallet up about $2.
///
/// The fee figure itself is sound (the bot read $1.3533 against Meteora's
/// $1.42). An earlier reading of this discrepancy blamed the number for being
/// unreliable, comparing it against claimedSol — but claimedSol is only the SOL
/// leg, and most fee income arrives as base token: $1.02 of that $1.42 was
/// GOBLIN. So it is still worth printing next to a pool, just never summed into
/// a result.
fn net_of(p: &TrackedPosition) -> f64 {
    p.pnl_usd.unwrap_or(0.0)
}

/// Minutes between two RFC3339 stamps, if both parse.
fn minutes_between(from: &str, to: &str) -> Option<i64> {
    let a = DateTime::parse_from_rfc3339(from).ok()?;
    let b = DateTime::parse_from_rfc3339(to).ok()?;
    Some((b - a).num_minutes().max(0))
}

/// How far below entry the position's range reached, in percent. A 48-bin range
/// at bin_step 100 covers ~38%, a 16-bin range ~15%. Printing it makes a range
/// that outruns the stop-loss visible instead of merely implied.
fn range_downside_pct(p: &TrackedPosition) -> Option<f64> {
    let step = p.bin_step? as f64 / 10_000.0;
    let bins = (p.upper_bin - p.lower_bin).max(0) as f64;
    if bins == 0.0 {
        return None;
    }
    Some((1.0 - (1.0 - step).powf(bins)) * 100.0)
}

/// Out-of-range breakdown. Direction matters more than the count: drifting out
/// above spot leaves the capital idle as SOL, dropping out below means the SOL
/// already converted into a token that kept falling. State carries no direction
/// flag, so it is inferred from the outcome — a position that earned nothing and
/// ended flat drifted up and away, one that ended red converted on the way down.
fn oor_section(closed: &[&TrackedPosition], open: &[&TrackedPosition]) -> String {
    let oor: Vec<&TrackedPosition> = closed
        .iter()
        .copied()
        .filter(|p| Bucket::of(p.close_reason.as_deref().unwrap_or("")) == Bucket::OutOfRange)
        .collect();
    let open_oor = open
        .iter()
        .filter(|p| p.status == PositionStatus::OutOfRange)
        .count();
    if oor.is_empty() && open_oor == 0 {
        return String::new();
    }

    let mut out = format!("\n\n↔️ *ANALISA OUT-OF-RANGE* · {} tutup", oor.len());
    if open_oor > 0 {
        out.push_str(&format!(" · {open_oor} masih terbuka"));
    }

    let idle = oor
        .iter()
        .filter(|p| p.all_time_fees_usd.unwrap_or(0.0) <= 0.0)
        .count();
    let bagged = oor.iter().filter(|p| p.pnl_pct.unwrap_or(0.0) < -1.0).count();
    if idle > 0 {
        out.push_str(&format!(
            "\n🟡 {idle} nganggur (fee nol) — harga naik menjauh, modal diam jadi SOL"
        ));
    }
    if bagged > 0 {
        out.push_str(&format!(
            "\n🔴 {bagged} nyangkut — SOL terlanjur jadi token yang terus turun"
        ));
    }

    // Time from deploy to leaving the range. The shorter it is, the worse the
    // range width fits the token's volatility.
    let mut spans: Vec<i64> = oor
        .iter()
        .filter_map(|p| {
            p.out_of_range_since
                .as_deref()
                .and_then(|t| minutes_between(&p.created_at, t))
        })
        .collect();
    spans.sort_unstable();
    if !spans.is_empty() {
        out.push_str(&format!(
            "\n⏱️ keluar range setelah {} (median dari {} posisi)",
            fmt_dur(spans[spans.len() / 2]),
            spans.len()
        ));
    }

    let widths: Vec<f64> = oor.iter().filter_map(|p| range_downside_pct(p)).collect();
    if !widths.is_empty() {
        let avg = widths.iter().sum::<f64>() / widths.len() as f64;
        out.push_str(&format!("\n📐 range rata-rata −{avg:.0}% dari harga masuk"));
    }
    out
}

/// Impermanent loss, read off the realised price leg. For a single-side SOL
/// position the price leg *is* the IL: SOL converts into the token as the price
/// falls, so a red PnL means it converted into something that kept dropping.
/// Fees are the only thing paying for that risk, which makes the ratio between
/// the two the number that decides whether the strategy earns its keep.
fn il_section(closed: &[&TrackedPosition]) -> String {
    if closed.is_empty() {
        return String::new();
    }
    let gains: f64 = closed.iter().filter_map(|p| p.pnl_usd).filter(|v| *v > 0.0).sum();
    let losses: f64 = closed.iter().filter_map(|p| p.pnl_usd).filter(|v| *v < 0.0).sum();
    let fees: f64 = closed
        .iter()
        .map(|p| p.all_time_fees_usd.unwrap_or(0.0))
        .sum();

    let mut out = String::from("\n\n🩸 *ANALISA IL*");
    out.push_str(&format!(
        "\nharga  +{:.2} / {:.2} = {:+.2} USD",
        gains,
        losses,
        gains + losses
    ));
    out.push_str(&format!("\nfee    {fees:.2} USD"));

    if losses < 0.0 {
        let cover = fees / losses.abs();
        out.push_str(&format!(
            "\ntutup  fee menutup {:.0}% dari rugi harga",
            cover * 100.0
        ));
        out.push_str(if cover >= 1.5 {
            "\n✅ fee jauh di atas IL — ini yang dicari"
        } else if cover >= 1.0 {
            "\n🟡 fee cuma sedikit di atas IL — marjin tipis"
        } else {
            "\n🔴 IL melebihi fee — posisi kelamaan ditahan atau range kelebaran"
        });
    }
    out.push_str("\n_fee dari API Meteora — cek saldo wallet buat angka pastinya_");
    out
}

pub fn render(state_path: &str, wallet_sol: Option<f64>, baseline_sol: Option<f64>) -> String {
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

    // Lead with the wallet. Everything else here is derived from state the bot
    // wrote about itself; this is the chain's answer, and when the two disagree
    // the chain is right.
    let truth = match (wallet_sol, baseline_sol) {
        (Some(now), Some(base)) => {
            format!("
💼 *Wallet:* {now:.4} SOL vs modal {base:.4} = *{:+.4} SOL*", now - base)
        }
        (Some(now), None) => format!("
💼 *Wallet:* {now:.4} SOL"),
        _ => String::new(),
    };
    let mut out = format!(
        "📋 *BRIEFING HARIAN* — _{} WIB_{}\n\n\
         💰 *PnL harga 24j:* {:+.2} USD ({}W/{}L) · fee {:.2} USD (sudah di PnL)\n\
         📗 *Terbuka:* {} · unreal {:+.2} · fee {:.2}\n\
         🏆 *Lifetime:* {} trade · {}% menang · PnL harga {:+.2} USD",
        wib.format("%Y-%m-%d %H:%M"),
        truth,
        pnl_24h,
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

    out.push_str(&drift_section(&closed_24h));
    out.push_str(&oor_section(&closed_24h, &open));
    out.push_str(&il_section(&closed_24h));
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

/// How far the reading an exit fired on sat from where the position settled.
///
/// Exit rules act on the live open-position quote; the withdrawal lands
/// somewhere else. A Tanisha-SOL stop-loss triggered at -7.04% and settled at
/// -3.35%, cutting a position that never reached the -6% it was cut for. One
/// case proves nothing — a memecoin can genuinely move three points in the
/// seconds around a close — so this prints the running gap. A drift that stays
/// one-sided says the trigger reading is biased and the thresholds are being
/// applied to a number that is not the outcome; a drift that scatters around
/// zero says the exits are landing on real prices and the noise is the market.
fn drift_section(closed: &[&TrackedPosition]) -> String {
    let pairs: Vec<(f64, f64)> = closed
        .iter()
        .filter_map(|p| Some((p.pnl_pct?, p.settled_pnl_pct?)))
        .collect();
    if pairs.is_empty() {
        return String::new();
    }
    let drifts: Vec<f64> = pairs.iter().map(|(t, s)| s - t).collect();
    let avg = drifts.iter().sum::<f64>() / drifts.len() as f64;
    let optimistic = drifts.iter().filter(|d| **d > 0.1).count();
    let pessimistic = drifts.iter().filter(|d| **d < -0.1).count();

    let mut out = format!(
        "

🎯 *AKURASI TRIGGER* · {} sampel
rata-rata drift {:+.2}pp · {} settle lebih baik · {} lebih buruk",
        pairs.len(),
        avg,
        optimistic,
        pessimistic
    );
    for (t, s) in pairs.iter().take(5) {
        out.push_str(&format!("
trigger {t:+.2}% → settle {s:+.2}% ({:+.2}pp)", s - t));
    }
    if pairs.len() >= 5 && avg.abs() >= 1.0 {
        out.push_str(if avg > 0.0 {
            "
⚠️ trigger konsisten lebih pesimis — exit kemungkinan kecepetan"
        } else {
            "
⚠️ trigger konsisten lebih optimis — exit kemungkinan kelewatan"
        });
    }
    out
}
