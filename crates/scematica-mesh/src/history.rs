//! Measures derived from the append-only trade log and the pool radar.
//!
//! These turn four of §20's six risk components from "needs a subsystem nobody built" into
//! real measurements, because the data was already on disk — it had simply never been read
//! as risk.
//!
//! ## Measurement versus mapping, and why the distinction is on screen
//!
//! Each function below returns a value in `[0,1]` suitable for the risk field, and getting
//! there takes two steps that are not equally trustworthy:
//!
//! 1. **The measurement.** Max drawdown, return dispersion, median depth, Herfindahl
//!    concentration. These are arithmetic over observed numbers and are exactly as good as
//!    the log.
//! 2. **The mapping to `[0,1]`.** Turning "σ of 38% per trade" into "risk 0.76" requires a
//!    reference point, and that reference is a *judgement*, not an observation.
//!
//! Every mapped term therefore carries its anchor in the term note, so a reader can
//! disagree with the mapping without having to distrust the measurement. Concentration is
//! the one component that needs no anchor at all — Herfindahl is already scale-free — and
//! it is the one to trust most.

use serde_json::Value;

/// Closed trades, most recent last.
#[derive(Clone, Debug, Default)]
pub struct TradeHistory {
    /// Realised PnL in SOL per closing trade, chronological.
    pub realised: Vec<f64>,
    /// Realised return percentage per closing trade, chronological.
    pub returns_pct: Vec<f64>,
}

/// How many recent closes the risk measures consider.
///
/// Bounded because these are *current* risk figures: a drawdown suffered three months and
/// two strategy rewrites ago is history, not a statement about the position the bot would
/// take now. Long enough that a handful of bad fills cannot dominate.
pub const RECENT_TRADES: usize = 200;

/// Return dispersion treated as full volatility risk.
///
/// A 50% standard deviation in per-trade return is extreme even for this market; at that
/// point position size is doing more than edge is. **This is an anchor, not a
/// measurement** — see the module docs.
pub const VOL_REFERENCE_PCT: f64 = 50.0;

/// Pool depth at or above which liquidity stops being the binding constraint, in SOL.
///
/// Also an anchor. Chosen against the sniper's own `max_pool_size` regime rather than
/// pulled from the air: below roughly this depth, exit slippage rather than direction
/// determines the outcome for the sizes this bot trades.
pub const DEPTH_REFERENCE_SOL: f64 = 50.0;

impl TradeHistory {
    /// Parse the append-only trade log.
    ///
    /// Malformed lines are skipped rather than failing the read: the file is appended to by
    /// a live process and the last line can legitimately be half-written when observed.
    /// Skipping one line costs a data point; refusing the whole file costs every measure.
    pub fn from_jsonl(text: &str) -> Self {
        let mut realised = Vec::new();
        let mut returns_pct = Vec::new();
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            // Only closes carry realised PnL. A BUY has no outcome yet, and counting its
            // zero would halve every dispersion figure.
            if v.get("kind").and_then(|k| k.as_str()) != Some("SELL") {
                continue;
            }
            if let Some(p) = v.get("pnl").and_then(|x| x.as_f64()) {
                realised.push(p);
            }
            if let Some(p) = v.get("pnl_pct").and_then(|x| x.as_f64()) {
                returns_pct.push(p);
            }
        }
        let n = realised.len().saturating_sub(RECENT_TRADES);
        let m = returns_pct.len().saturating_sub(RECENT_TRADES);
        TradeHistory { realised: realised[n..].to_vec(), returns_pct: returns_pct[m..].to_vec() }
    }

    pub fn closes(&self) -> usize {
        self.realised.len()
    }

    /// §20 `R_drawdown` — deepest peak-to-trough fall of the cumulative PnL curve, as a
    /// fraction of the peak.
    ///
    /// `None` when the equity curve never rose: a strategy that has only ever lost has no
    /// peak to fall from, and reporting drawdown `0` for it would be the most misleading
    /// number on the page.
    pub fn max_drawdown(&self) -> Option<f64> {
        if self.realised.is_empty() {
            return None;
        }
        let mut equity = 0.0;
        let mut peak = f64::NEG_INFINITY;
        let mut worst = 0.0;
        let mut saw_positive_peak = false;
        for p in &self.realised {
            equity += p;
            if equity > peak {
                peak = equity;
            }
            if peak > 0.0 {
                saw_positive_peak = true;
                let dd = (peak - equity) / peak;
                if dd > worst {
                    worst = dd;
                }
            }
        }
        if !saw_positive_peak {
            return None;
        }
        Some(worst.clamp(0.0, 1.0))
    }

    /// §20 `R_volatility` — dispersion of realised returns against [`VOL_REFERENCE_PCT`].
    pub fn volatility_risk(&self) -> Option<(f64, f64)> {
        if self.returns_pct.len() < 8 {
            // Dispersion over a handful of points is dominated by sampling noise; the same
            // reasoning as `MIN_SAMPLES` in the agent's own equations module.
            return None;
        }
        let n = self.returns_pct.len() as f64;
        let mean = self.returns_pct.iter().sum::<f64>() / n;
        let var = self.returns_pct.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        let sigma = var.sqrt();
        Some((sigma, (sigma / VOL_REFERENCE_PCT).clamp(0.0, 1.0)))
    }
}

/// §20 `R_liquidity` — from the depth of the pools actually reaching the bot.
///
/// Uses the **median** rather than the mean: pool depth is heavy-tailed, and one 800-SOL
/// pool in a field of 2-SOL ones would otherwise report the field as deep.
pub fn liquidity_risk(radar: &Value) -> Option<(f64, f64)> {
    let arr = radar.as_array()?;
    let mut sizes: Vec<f64> = arr
        .iter()
        .filter_map(|p| p.get("size_sol").and_then(|s| s.as_f64()))
        .filter(|s| *s > 0.0)
        .collect();
    if sizes.len() < 4 {
        return None;
    }
    sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sizes[sizes.len() / 2];
    let risk = (1.0 - median / DEPTH_REFERENCE_SOL).clamp(0.0, 1.0);
    Some((median, risk))
}

/// §20 `R_concentration` — Herfindahl index over open position sizes.
///
/// The one risk component here needing no anchor: `H = Σwᵢ²` is already scale-free and
/// already lands in `(0,1]`. One position is `1.0` (everything in one place); ten equal
/// positions are `0.1`. No reference constant, so nothing to disagree with.
///
/// `None` for an empty book — no positions is not concentration `0`, it is no exposure at
/// all, and the two must not report alike.
pub fn concentration_risk(positions: &Value) -> Option<(usize, f64)> {
    let arr = positions.as_array()?;
    let sizes: Vec<f64> = arr
        .iter()
        .filter_map(|p| {
            p.get("amount_sol")
                .or_else(|| p.get("amount"))
                .or_else(|| p.get("size_sol"))
                .and_then(|v| v.as_f64())
        })
        .filter(|v| *v > 0.0)
        .collect();
    if sizes.is_empty() {
        return None;
    }
    let total: f64 = sizes.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let h: f64 = sizes.iter().map(|s| (s / total).powi(2)).sum();
    Some((sizes.len(), h.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn log(rows: &[(&str, f64, f64)]) -> String {
        rows.iter()
            .map(|(kind, pnl, pct)| json!({"kind": kind, "pnl": pnl, "pnl_pct": pct}).to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Only closes carry an outcome. Counting a BUY's absent PnL as zero would halve every
    /// dispersion figure computed here.
    #[test]
    fn buys_are_not_outcomes() {
        let h = TradeHistory::from_jsonl(&log(&[("BUY", 0.0, 0.0), ("SELL", 1.0, 10.0), ("BUY", 0.0, 0.0)]));
        assert_eq!(h.closes(), 1);
    }

    /// A half-written final line is normal against a live appender and must cost one data
    /// point, not the whole file.
    #[test]
    fn a_torn_final_line_is_skipped_not_fatal() {
        let mut t = log(&[("SELL", 1.0, 10.0), ("SELL", 2.0, 20.0)]);
        t.push_str("\n{\"kind\":\"SELL\",\"pnl\":3.0,");
        let h = TradeHistory::from_jsonl(&t);
        assert_eq!(h.closes(), 2);
    }

    #[test]
    fn drawdown_is_peak_to_trough_over_the_equity_curve() {
        // +10 → peak 10, then −4 → equity 6, drawdown 0.4.
        let h = TradeHistory::from_jsonl(&log(&[("SELL", 10.0, 1.0), ("SELL", -4.0, -1.0)]));
        assert_eq!(h.max_drawdown(), Some(0.4));
    }

    /// A curve that never rose has no peak to fall from, and reporting drawdown 0 for a
    /// strategy that has only ever lost would be the most misleading number on the page.
    #[test]
    fn a_curve_that_never_rose_has_no_drawdown_to_report() {
        let h = TradeHistory::from_jsonl(&log(&[("SELL", -1.0, -5.0), ("SELL", -2.0, -5.0)]));
        assert_eq!(h.max_drawdown(), None);
    }

    #[test]
    fn volatility_needs_enough_samples_to_mean_anything() {
        let few = TradeHistory::from_jsonl(&log(&[("SELL", 1.0, 10.0), ("SELL", 1.0, -10.0)]));
        assert_eq!(few.volatility_risk(), None, "dispersion over 2 points is noise");

        let rows: Vec<(&str, f64, f64)> = (0..12)
            .map(|i| ("SELL", 0.0, if i % 2 == 0 { 50.0 } else { -50.0 }))
            .collect();
        let many = TradeHistory::from_jsonl(&log(&rows));
        let (sigma, risk) = many.volatility_risk().unwrap();
        assert!((sigma - 50.0).abs() < 1e-6);
        assert_eq!(risk, 1.0, "σ at the reference is full volatility risk");
    }

    /// Depth is heavy-tailed, so one deep pool must not make a shallow field look deep.
    #[test]
    fn liquidity_uses_the_median_not_the_mean() {
        let radar = json!([
            {"size_sol": 1.0}, {"size_sol": 2.0}, {"size_sol": 2.0},
            {"size_sol": 3.0}, {"size_sol": 800.0}
        ]);
        let (median, risk) = liquidity_risk(&radar).unwrap();
        assert_eq!(median, 2.0, "the mean would be 161");
        assert!(risk > 0.9, "a 2-SOL median field is nearly all liquidity risk");
    }

    #[test]
    fn a_thin_radar_is_unmeasured() {
        assert!(liquidity_risk(&json!([{"size_sol": 1.0}])).is_none());
        assert!(liquidity_risk(&json!([])).is_none());
    }

    /// Herfindahl needs no anchor: one position is total concentration, ten equal ones are
    /// a tenth of it.
    #[test]
    fn concentration_is_scale_free() {
        let one = concentration_risk(&json!([{"amount_sol": 5.0}])).unwrap();
        assert_eq!(one, (1, 1.0));

        let ten = json!((0..10).map(|_| json!({"amount_sol": 1.0})).collect::<Vec<_>>());
        let (n, h) = concentration_risk(&ten).unwrap();
        assert_eq!(n, 10);
        assert!((h - 0.1).abs() < 1e-9);

        // Scale-free: doubling every position changes nothing.
        let doubled = json!((0..10).map(|_| json!({"amount_sol": 2.0})).collect::<Vec<_>>());
        assert!((concentration_risk(&doubled).unwrap().1 - h).abs() < 1e-9);
    }

    /// An empty book is no exposure, not concentration zero.
    #[test]
    fn an_empty_book_is_unmeasured_not_diversified() {
        assert!(concentration_risk(&json!([])).is_none());
    }

    /// Only the recent window counts — a drawdown from before two strategy rewrites is
    /// history, not a statement about the position the bot would take now.
    #[test]
    fn only_the_recent_window_is_considered() {
        let rows: Vec<(&str, f64, f64)> = (0..RECENT_TRADES + 50).map(|_| ("SELL", 1.0, 1.0)).collect();
        let h = TradeHistory::from_jsonl(&log(&rows));
        assert_eq!(h.closes(), RECENT_TRADES);
    }
}
