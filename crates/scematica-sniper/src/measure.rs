//! Measure: what the decision log can actually answer about the bot's own behaviour.
//!
//! This module exists because of a mistake that is easy to make and expensive to act on.
//! Reading `scematica-pool-decisions.jsonl` as a whole, one finds that 1,747 pools were
//! rejected with `inflow_rate=0.000` — a fifth of every evaluation ever made — and
//! concludes the momentum gate is vetoing on a signal that was never measured. That
//! conclusion is correct about the data and wrong about the bot: the veto was found and
//! removed on 2026-08-05, and the log simply still contains the months before it. Split the
//! same file at that date and the rate falls from 28.3% of the window to 0.4%.
//!
//! So the first rule here: **an aggregate over the whole log is a statement about history,
//! not about the bot.** Every report this module produces is windowed, and `--split` exists
//! because comparing two windows is usually the only honest way to ask whether a change
//! worked.
//!
//! ## What it reports, and why each one
//!
//! - **The funnel.** Where pools die, by stage, as a share of the window. A stage that
//!   dominates one window and vanishes from the next is the clearest evidence a fix landed.
//! - **The dead-signal audit.** For every numeric field, how many records carried a
//!   non-zero value. This is the standing version of a rule this repository learned the
//!   expensive way: *before adjusting any threshold, check that the quantity it compares
//!   against is capable of varying.* A gate reading a field that is `0.0` in every record
//!   is not a strict filter, it is an unconditional veto wearing a threshold.
//! - **Coverage**, when it was recorded. See below.
//!
//! ## The distinction this module refuses to blur
//!
//! A field that is `0.0` in every record admits two readings — the quantity was measured
//! and was zero, or nothing ever wrote it — and the log cannot tell them apart. Neither can
//! this module, so it does not guess. It reports **how many records carried a non-zero
//! value**, which is a measurement about the *field*, and leaves the interpretation to a
//! reader who can go and look at the producer. `never varied` is a much weaker and much
//! more honest claim than `always zero`.
//!
//! Coverage — the share of RPC-bound checks that returned real data — is not in the decision
//! record at all. The coherence breaker counts it process-globally and nothing writes it per
//! pool. So for every record written before the sampler existed, coverage is **unmeasured**,
//! and it renders as `—`. It is never rendered as `0`, and PnL is never attributed to a
//! coverage band that was not recorded. That is the same rule as `scema_policy::render::cell`
//! and for the same reason: a column of numbers is the most persuasive thing a program can
//! emit, and "nobody looked" must not appear in it wearing a measurement's clothes.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

/// One decision, as much of it as this module needs.
///
/// Deliberately a separate, permissive struct rather than `PoolDecisionEvent`. This reads
/// months of history written by older builds, and a strict deserialise would drop whole
/// windows because a field was added later. Every field is defaulted; a missing numeric is
/// `None`, which is **not** zero.
#[derive(Debug, Clone, Deserialize)]
pub struct Decision {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub decision: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub mint: String,
    #[serde(flatten)]
    pub rest: BTreeMap<String, serde_json::Value>,
}

impl Decision {
    /// A numeric field, or `None` when it is absent or not a number.
    ///
    /// `None` and `Some(0.0)` are different answers and callers must keep them apart: the
    /// first says this build did not write the field, the second says it wrote a zero.
    pub fn num(&self, key: &str) -> Option<f64> {
        self.rest.get(key).and_then(|v| v.as_f64())
    }

    /// `YYYY-MM-DD`, or empty when the timestamp is unusable.
    pub fn day(&self) -> &str {
        self.timestamp.get(0..10).unwrap_or("")
    }
}

/// One realised trade.
#[derive(Debug, Clone, Deserialize)]
pub struct Trade {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub mint: String,
    #[serde(default)]
    pub pnl: f64,
    #[serde(default)]
    pub status: String,
}

impl Trade {
    pub fn day(&self) -> &str {
        self.timestamp.get(0..10).unwrap_or("")
    }
    /// A sell that actually settled. Buys carry `pnl = 0.0` by construction, so counting
    /// them would dilute every average with zeros that are not outcomes.
    pub fn is_realised(&self) -> bool {
        self.kind.eq_ignore_ascii_case("SELL") || self.kind.eq_ignore_ascii_case("ARB")
    }
}

/// How a JSONL file was read.
///
/// `skipped` is reported rather than swallowed. A log with a torn final line is normal —
/// the writer appends without a rename — but a file where thousands of lines fail to parse
/// is a different situation, and a reader that silently drops them would present a
/// confident report over a fraction of the data.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadStats {
    pub parsed: usize,
    pub skipped: usize,
}

/// Read a JSONL file, tolerating unparseable lines and counting them.
pub fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> (Vec<T>, ReadStats) {
    let mut out = Vec::new();
    let mut stats = ReadStats::default();
    let Ok(text) = std::fs::read_to_string(path) else {
        return (out, stats);
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<T>(line) {
            Ok(v) => {
                out.push(v);
                stats.parsed += 1;
            }
            Err(_) => stats.skipped += 1,
        }
    }
    (out, stats)
}

/// One row of the funnel.
#[derive(Debug, Clone)]
pub struct StageCount {
    pub stage: String,
    pub decision: String,
    pub count: usize,
    pub share: f64,
}

/// Where pools died, most common first.
pub fn funnel(rows: &[Decision]) -> Vec<StageCount> {
    let mut by: BTreeMap<(String, String), usize> = BTreeMap::new();
    for r in rows {
        *by.entry((r.stage.clone(), r.decision.clone())).or_insert(0) += 1;
    }
    let total = rows.len().max(1) as f64;
    let mut out: Vec<StageCount> = by
        .into_iter()
        .map(|((stage, decision), count)| StageCount {
            stage,
            decision,
            count,
            share: count as f64 / total,
        })
        .collect();
    // Descending by count, then by stage so the order is stable for equal counts — a report
    // that reshuffles between runs cannot be diffed.
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.stage.cmp(&b.stage)));
    out
}

/// What one numeric field did across a window.
#[derive(Debug, Clone)]
pub struct SignalAudit {
    pub field: String,
    /// Records where this build wrote the field at all.
    pub present: usize,
    /// Records where it carried something other than zero.
    pub nonzero: usize,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl SignalAudit {
    /// Did this field carry information in this window?
    ///
    /// The question is deliberately not "is it always zero". A field present in every record
    /// and non-zero in none of them **never varied**, which is a fact about the field. What
    /// that means — measured and genuinely zero, or never populated — is a question about the
    /// producer that this data cannot settle.
    pub fn never_varied(&self) -> bool {
        self.present > 0 && self.nonzero == 0
    }

    /// Share of records carrying a non-zero value, or `None` when the field is absent
    /// entirely. `None` is not `0.0`.
    pub fn nonzero_share(&self) -> Option<f64> {
        if self.present == 0 {
            return None;
        }
        Some(self.nonzero as f64 / self.present as f64)
    }
}

/// Audit every numeric field named, in the order given.
pub fn audit(rows: &[Decision], fields: &[&str]) -> Vec<SignalAudit> {
    fields
        .iter()
        .map(|f| {
            let mut present = 0;
            let mut nonzero = 0;
            let mut min: Option<f64> = None;
            let mut max: Option<f64> = None;
            for r in rows {
                let Some(v) = r.num(f) else { continue };
                present += 1;
                if v != 0.0 {
                    nonzero += 1;
                    min = Some(min.map_or(v, |m: f64| m.min(v)));
                    max = Some(max.map_or(v, |m: f64| m.max(v)));
                }
            }
            SignalAudit { field: (*f).to_string(), present, nonzero, min, max }
        })
        .collect()
}

/// The numeric fields worth auditing, in the order a reader wants them.
pub const AUDIT_FIELDS: &[&str] = &[
    "pool_size_sol",
    "pool_age_secs",
    "velocity_sol_per_sec",
    "buy_pressure_ratio",
    "pool_score",
    "pumpfun_score",
    "inflow_rate_sol_per_sec",
    "dex_boost_usd",
    "social_count",
    "effective_min_score",
    "dq_confidence",
];

/// Realised PnL over a window, and how much of it resolved at all.
#[derive(Debug, Clone, Default)]
pub struct Realised {
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub total_pnl: f64,
}

impl Realised {
    /// Mean PnL per realised trade, or `None` when nothing resolved.
    ///
    /// `None` rather than `0.0`, for the same reason `Calibration::mean_abs_error` is
    /// `None` when nothing resolved: an average over an empty set is not zero, it is
    /// undefined, and printing zero invites a comparison against a window that had trades.
    pub fn mean(&self) -> Option<f64> {
        if self.trades == 0 {
            None
        } else {
            Some(self.total_pnl / self.trades as f64)
        }
    }

    pub fn win_rate(&self) -> Option<f64> {
        if self.trades == 0 {
            None
        } else {
            Some(self.wins as f64 / self.trades as f64)
        }
    }
}

/// Realised PnL across a set of trades.
pub fn realised(trades: &[Trade]) -> Realised {
    let mut r = Realised::default();
    for t in trades.iter().filter(|t| t.is_realised()) {
        r.trades += 1;
        r.total_pnl += t.pnl;
        if t.pnl > 0.0 {
            r.wins += 1;
        } else if t.pnl < 0.0 {
            r.losses += 1;
        }
    }
    r
}

/// Split a slice of records into the part before a day and the part from it onward.
///
/// Day granularity, and string comparison rather than date parsing: ISO-8601 sorts
/// lexicographically, which is the whole reason the format is used here, and a parser would
/// add a failure mode for records whose timestamp is malformed.
pub fn split_at<'a, T, F>(rows: &'a [T], day: &str, key: F) -> (Vec<&'a T>, Vec<&'a T>)
where
    F: Fn(&T) -> &str,
{
    let mut before = Vec::new();
    let mut after = Vec::new();
    for r in rows {
        if key(r) < day {
            before.push(r);
        } else {
            after.push(r);
        }
    }
    (before, after)
}


/// One coherence sample, as written by the sniper every 30 s.
#[derive(Debug, Clone, Deserialize)]
pub struct CoherenceSample {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub resolution_rate: f64,
    #[serde(default)]
    pub resolved: u64,
    #[serde(default)]
    pub unresolved: u64,
    /// False while the breaker's sample was too small to judge.
    #[serde(default)]
    pub decisive: bool,
}

impl CoherenceSample {
    pub fn day(&self) -> &str {
        self.timestamp.get(0..10).unwrap_or("")
    }
}

/// What the coherence samples say about a window.
///
/// Every field is `Option` because the honest answer, for every record written before the
/// sampler existed, is that nobody looked. A coverage report that defaulted to `0.0` would
/// claim the pipeline resolved nothing — the exact inverse of "we do not know" — and that
/// claim would then be attached to real PnL.
#[derive(Debug, Clone, Default)]
pub struct CoverageReport {
    pub samples: usize,
    /// Samples the breaker considered large enough to judge.
    pub decisive: usize,
    /// Mean resolution rate across decisive samples only.
    pub mean_resolution: Option<f64>,
    pub worst_resolution: Option<f64>,
}

impl CoverageReport {
    /// Was anything measured at all?
    pub fn measured(&self) -> bool {
        self.decisive > 0
    }
}

/// Summarise coherence samples over a window.
///
/// Indecisive samples are counted but never averaged. Below its minimum sample size the
/// breaker explicitly declines to judge, and folding those into a mean would manufacture a
/// verdict out of the breaker's own refusal to give one.
pub fn coverage(samples: &[CoherenceSample]) -> CoverageReport {
    let mut r = CoverageReport { samples: samples.len(), ..Default::default() };
    let decisive: Vec<&CoherenceSample> = samples.iter().filter(|s| s.decisive).collect();
    r.decisive = decisive.len();
    if decisive.is_empty() {
        return r;
    }
    let sum: f64 = decisive.iter().map(|s| s.resolution_rate).sum();
    r.mean_resolution = Some(sum / decisive.len() as f64);
    r.worst_resolution = decisive
        .iter()
        .map(|s| s.resolution_rate)
        .fold(None, |acc: Option<f64>, v| Some(acc.map_or(v, |a| a.min(v))));
    r
}


/// How long the pipeline took to arrive at a decision, over a window.
///
/// Every field is `Option` because the honest answer for every record written before the
/// span was instrumented is that nobody looked. A latency report defaulting to `0` would
/// claim the bot arrives instantly — the most flattering possible reading of the exact thing
/// the Arrive phase exists to investigate.
#[derive(Debug, Clone, Default)]
pub struct Latency {
    /// Records that carried a span at all.
    pub measured: usize,
    /// Records in the window, measured or not.
    pub total: usize,
    pub median_ms: Option<u64>,
    pub p90_ms: Option<u64>,
    pub worst_ms: Option<u64>,
}

impl Latency {
    pub fn any(&self) -> bool {
        self.measured > 0
    }

    /// Share of the window that carried a span, or `None` when the window is empty.
    pub fn coverage(&self) -> Option<f64> {
        if self.total == 0 {
            return None;
        }
        Some(self.measured as f64 / self.total as f64)
    }
}

/// Summarise detection-to-decision latency over a window.
///
/// Percentiles by nearest rank on the sorted samples — no interpolation, because an
/// interpolated percentile is a number that was never measured, and this report is about
/// distinguishing what was measured from what was not.
pub fn latency(rows: &[Decision]) -> Latency {
    let mut samples: Vec<u64> = rows
        .iter()
        .filter_map(|r| r.num("decide_latency_ms"))
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| v as u64)
        .collect();
    let mut l = Latency { measured: samples.len(), total: rows.len(), ..Default::default() };
    if samples.is_empty() {
        return l;
    }
    samples.sort_unstable();
    let at = |q: f64| -> u64 {
        let idx = ((samples.len() as f64 - 1.0) * q).round() as usize;
        samples[idx.min(samples.len() - 1)]
    };
    l.median_ms = Some(at(0.5));
    l.p90_ms = Some(at(0.9));
    l.worst_ms = samples.last().copied();
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(day: &str, stage: &str, extra: &[(&str, f64)]) -> Decision {
        let mut rest = BTreeMap::new();
        for (k, v) in extra {
            rest.insert(
                (*k).to_string(),
                serde_json::Value::from(*v),
            );
        }
        Decision {
            timestamp: format!("{day}T00:00:00Z"),
            decision: "rejected".into(),
            stage: stage.into(),
            reason: String::new(),
            mint: "m".into(),
            rest,
        }
    }

    #[test]
    fn an_absent_field_is_not_a_zero() {
        // The distinction the whole module is arranged around. A build that never wrote the
        // field and a build that wrote 0.0 are different facts, and `num` keeps them apart.
        let d = dec("2026-01-01", "filters", &[("pool_score", 0.0)]);
        assert_eq!(d.num("pool_score"), Some(0.0));
        assert_eq!(d.num("never_written"), None);
    }

    #[test]
    fn a_field_nobody_wrote_reports_no_share_rather_than_zero_percent() {
        let rows = vec![dec("2026-01-01", "filters", &[("pool_score", 1.0)])];
        let a = audit(&rows, &["absent_field"]);
        assert_eq!(a[0].present, 0);
        assert_eq!(a[0].nonzero_share(), None, "absent must not read as 0%");
        assert!(!a[0].never_varied(), "a field nobody wrote did not 'never vary'");
    }

    #[test]
    fn a_field_written_but_always_zero_is_flagged_as_never_varied() {
        let rows = vec![
            dec("2026-01-01", "filters", &[("velocity", 0.0)]),
            dec("2026-01-02", "filters", &[("velocity", 0.0)]),
        ];
        let a = audit(&rows, &["velocity"]);
        assert_eq!(a[0].present, 2);
        assert_eq!(a[0].nonzero, 0);
        assert!(a[0].never_varied());
        assert_eq!(a[0].nonzero_share(), Some(0.0));
        // And it reports no range, because there is no non-zero value to bound.
        assert_eq!(a[0].min, None);
    }

    #[test]
    fn the_funnel_is_stable_for_equal_counts() {
        // A report that reshuffles between runs on tied counts cannot be diffed, which is
        // the main thing anybody does with two of these.
        let rows = vec![dec("2026-01-01", "b", &[]), dec("2026-01-01", "a", &[])];
        let f = funnel(&rows);
        assert_eq!(f[0].stage, "a");
        assert_eq!(f[1].stage, "b");
        assert!((f[0].share - 0.5).abs() < 1e-12);
    }

    #[test]
    fn splitting_separates_the_windows_at_the_day_boundary() {
        // The whole reason this module exists: an aggregate over the entire log is a claim
        // about history, and reads as a claim about the bot.
        let rows = vec![
            dec("2026-07-09", "momentum_gate", &[]),
            dec("2026-08-05", "risk", &[]),
            dec("2026-08-16", "risk", &[]),
        ];
        let (before, after) = split_at(&rows, "2026-08-05", |d| d.day());
        assert_eq!(before.len(), 1);
        assert_eq!(after.len(), 2);
        assert_eq!(before[0].stage, "momentum_gate");
    }

    #[test]
    fn an_empty_window_has_no_mean_rather_than_a_mean_of_zero() {
        let r = realised(&[]);
        assert_eq!(r.trades, 0);
        assert_eq!(r.mean(), None, "an average over nothing is undefined, not zero");
        assert_eq!(r.win_rate(), None);
    }

    #[test]
    fn only_settled_sells_count_as_outcomes() {
        // Buys carry pnl = 0.0 by construction. Counting them would dilute every average
        // with zeros that are not outcomes.
        let t = |kind: &str, pnl: f64| Trade {
            timestamp: "2026-01-01T00:00:00Z".into(),
            kind: kind.into(),
            mint: "m".into(),
            pnl,
            status: "ok".into(),
        };
        let r = realised(&[t("BUY", 0.0), t("SELL", 1.0), t("SELL", -0.5)]);
        assert_eq!(r.trades, 2);
        assert_eq!(r.wins, 1);
        assert_eq!(r.losses, 1);
        assert!((r.total_pnl - 0.5).abs() < 1e-12);
    }

    #[test]
    fn unparseable_lines_are_counted_not_swallowed() {
        let dir = std::env::temp_dir().join("scematica-measure-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("decisions.jsonl");
        std::fs::write(
            &p,
            "{\"stage\":\"filters\",\"timestamp\":\"2026-01-01T00:00:00Z\"}\nnot json\n\n",
        )
        .unwrap();
        let (rows, stats) = read_jsonl::<Decision>(&p);
        assert_eq!(rows.len(), 1);
        assert_eq!(stats.parsed, 1);
        assert_eq!(stats.skipped, 1, "a torn line is normal; thousands are not");
    }

    #[test]
    fn a_missing_file_reads_as_empty_rather_than_panicking() {
        let (rows, stats) = read_jsonl::<Decision>(Path::new("does-not-exist.jsonl"));
        assert!(rows.is_empty());
        assert_eq!(stats.parsed, 0);
        assert_eq!(stats.skipped, 0);
    }

    #[test]
    fn coverage_over_no_samples_is_unmeasured_not_zero() {
        // The failure this whole report is arranged to avoid: claiming the pipeline
        // resolved nothing, when in fact nobody looked.
        let r = coverage(&[]);
        assert_eq!(r.samples, 0);
        assert!(!r.measured());
        assert_eq!(r.mean_resolution, None, "unmeasured coverage must not read as 0.0");
    }

    #[test]
    fn indecisive_samples_are_counted_but_never_averaged() {
        // Below its minimum sample size the breaker declines to judge. Averaging those in
        // would manufacture a verdict out of its refusal to give one.
        let s = |rate: f64, decisive: bool| CoherenceSample {
            timestamp: "2026-08-20T00:00:00Z".into(),
            resolution_rate: rate,
            resolved: 1,
            unresolved: 0,
            decisive,
        };
        let r = coverage(&[s(0.0, false), s(0.0, false)]);
        assert_eq!(r.samples, 2);
        assert_eq!(r.decisive, 0);
        assert!(!r.measured());
        assert_eq!(r.mean_resolution, None);

        let r = coverage(&[s(0.9, true), s(0.5, true), s(0.0, false)]);
        assert_eq!(r.samples, 3);
        assert_eq!(r.decisive, 2);
        assert!((r.mean_resolution.unwrap() - 0.7).abs() < 1e-12);
        assert!((r.worst_resolution.unwrap() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn a_measured_zero_resolution_is_reported_as_a_number() {
        // And the other side of it: a decisive sample that really did resolve nothing is a
        // measurement, and must not be hidden behind the same dash as "unmeasured".
        let s = CoherenceSample {
            timestamp: "2026-08-20T00:00:00Z".into(),
            resolution_rate: 0.0,
            resolved: 0,
            unresolved: 40,
            decisive: true,
        };
        let r = coverage(&[s]);
        assert!(r.measured());
        assert_eq!(r.mean_resolution, Some(0.0));
    }

    #[test]
    fn latency_over_records_that_never_carried_it_is_unmeasured_not_zero() {
        // Every record written before the span was instrumented. Reporting 0 would claim
        // the bot arrives instantly, which is the most flattering possible reading of the
        // thing under investigation.
        let rows = vec![dec("2026-01-01", "filters", &[("pool_score", 1.0)])];
        let l = latency(&rows);
        assert_eq!(l.measured, 0);
        assert_eq!(l.total, 1);
        assert!(!l.any());
        assert_eq!(l.median_ms, None);
        assert_eq!(l.coverage(), Some(0.0), "0 of 1 measured is a real ratio");
    }

    #[test]
    fn latency_percentiles_are_values_that_were_actually_measured() {
        // Nearest rank, no interpolation: an interpolated percentile is a number nobody
        // observed, and this whole report is about telling those apart.
        let rows: Vec<Decision> = [10.0, 20.0, 30.0, 40.0, 1000.0]
            .iter()
            .map(|v| dec("2026-01-01", "filters", &[("decide_latency_ms", *v)]))
            .collect();
        let l = latency(&rows);
        assert_eq!(l.measured, 5);
        assert_eq!(l.median_ms, Some(30));
        assert_eq!(l.worst_ms, Some(1000));
        assert_eq!(l.coverage(), Some(1.0));
        for v in [l.median_ms, l.p90_ms, l.worst_ms] {
            assert!([10, 20, 30, 40, 1000].contains(&v.unwrap()), "{v:?} was never measured");
        }
    }

    #[test]
    fn a_partly_instrumented_window_reports_its_own_coverage() {
        // The common case during rollout: some records carry the span, most do not. The
        // median is over what was measured, and the coverage says how much that was.
        let mut rows = vec![dec("2026-01-01", "filters", &[("decide_latency_ms", 50.0)])];
        rows.push(dec("2026-01-01", "filters", &[("pool_score", 1.0)]));
        rows.push(dec("2026-01-01", "filters", &[("pool_score", 1.0)]));
        let l = latency(&rows);
        assert_eq!(l.measured, 1);
        assert_eq!(l.total, 3);
        assert_eq!(l.median_ms, Some(50));
        assert!((l.coverage().unwrap() - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn an_empty_window_has_no_coverage_rather_than_zero_coverage() {
        let l = latency(&[]);
        assert_eq!(l.coverage(), None, "0 of 0 is undefined, not 0%");
    }
}
