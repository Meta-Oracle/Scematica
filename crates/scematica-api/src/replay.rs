//! Counterfactual replay over the decision log.
//!
//! Answers "what if the thresholds had been different?" against what the pipeline
//! actually measured, rather than against a simulation of it. Every pool the sniper
//! evaluated is written to `scematica-pool-decisions.jsonl` with the values that decided
//! it — `pool_score`, `pool_size_sol`, `pool_age_secs`, `buy_pressure_ratio` — so a
//! threshold change can be re-applied to real measurements with no RPC and no guessing.
//!
//! **This is deliberately not built on `scematica_sniper::Backtester`.** That replays
//! `BacktestPool` records through `static_filter_check`, which returns `false` outright
//! whenever `min_pool_size > 0` or any RPC-bound filter is enabled, and never looks at
//! `pool_score` at all. Under any realistic config it answers "nothing would pass" — a
//! confident number that means nothing. The decision log has no such problem: the
//! measurement already happened.
//!
//! # The asymmetry that makes this honest
//!
//! Outcomes exist for pools that were **taken** (their trades are in
//! `scematica-trades.jsonl`) and do not exist for pools that were **rejected** — nobody
//! bought them, so nothing recorded what they would have done. That is not a gap to be
//! filled with an estimate; it is the shape of the evidence:
//!
//! * **Tightening** a threshold excludes pools you did take, so the PnL delta is
//!   *exact* — real trades, real realised SOL.
//! * **Loosening** admits pools you rejected, so the PnL delta is *unknowable*. This
//!   module reports how many, and their measured distribution against the winners you
//!   did take, and refuses to put a number on their return.
//!
//! Inventing an expected value for the second case is the single most tempting thing to
//! do here and would make every answer built on it worthless — it is exactly the
//! "simulated PnL rendering as live results" failure the rest of this project is
//! organised against.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Proposed thresholds. Every field is optional; `None` means "leave as it was".
#[derive(Debug, Default, Clone, Deserialize)]
pub struct ReplayQuery {
    pub min_pool_score: Option<f64>,
    pub min_pool_size_sol: Option<f64>,
    pub max_pool_age_secs: Option<f64>,
    pub min_buy_pressure_ratio: Option<f64>,
    /// Most recent N decisions to consider. Clamped by the caller.
    pub limit: Option<usize>,
}

impl ReplayQuery {
    /// True when no threshold was supplied — the caller asked nothing.
    pub fn is_empty(&self) -> bool {
        self.min_pool_score.is_none()
            && self.min_pool_size_sol.is_none()
            && self.max_pool_age_secs.is_none()
            && self.min_buy_pressure_ratio.is_none()
    }
}

/// One pool as the pipeline measured it.
#[derive(Debug, Clone)]
pub struct DecisionRow {
    pub mint: String,
    pub accepted: bool,
    pub pool_score: f64,
    pub pool_size_sol: f64,
    pub pool_age_secs: f64,
    pub buy_pressure_ratio: f64,
    pub reason: String,
    /// Pipeline stage that decided it. Structured, unlike `reason`.
    pub stage: String,
}

/// A pool whose fate changes under the proposed thresholds.
#[derive(Debug, Clone, Serialize)]
pub struct ChangedPool {
    pub mint: String,
    pub pool_score: f64,
    pub pool_size_sol: f64,
    pub pool_age_secs: f64,
    /// Realised SOL, present only for pools that were actually traded.
    pub realised_pnl_sol: Option<f64>,
    pub original_reason: String,
}

#[derive(Debug, Default, Serialize)]
pub struct ReplayOutcome {
    pub decisions_considered: usize,
    pub actually_accepted: usize,
    pub would_accept: usize,

    /// Rejected before, accepted now. **Outcome unknowable** — never priced.
    pub newly_admitted: usize,
    /// Would clear the proposed thresholds but were rejected for an unrelated reason,
    /// which the change does not remove. Reported so the admitted count is not read as
    /// the total addressable set.
    pub still_blocked_elsewhere: usize,
    /// Accepted before, rejected now. Outcome known: these were really traded.
    pub newly_excluded: usize,

    /// Exact realised SOL on the pools a tightening would have removed. Negative here
    /// means the tightening would have *saved* money.
    pub excluded_realised_pnl_sol: f64,
    pub excluded_with_known_outcome: usize,
    pub excluded_winners: usize,
    pub excluded_losers: usize,

    /// Measured profile of the unknowable set, so it is characterised rather than
    /// silently dropped.
    pub admitted_avg_pool_score: f64,
    pub admitted_avg_size_sol: f64,
    pub admitted_avg_age_secs: f64,
    /// The same averages over pools that were taken *and* made money — the only
    /// comparison available for judging whether the admitted set looks like them.
    pub winner_avg_pool_score: f64,
    pub winner_avg_size_sol: f64,
    pub winner_avg_age_secs: f64,

    pub sample_excluded: Vec<ChangedPool>,
    pub sample_admitted: Vec<ChangedPool>,

    /// Caveats that must travel with the numbers.
    pub notes: Vec<String>,
}

/// Examples returned per direction. Enough to be concrete, few enough to stay cheap.
const SAMPLE_LIMIT: usize = 8;

fn num(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// Parse decision-log rows into the fields a threshold replay needs.
pub fn parse_decisions(rows: &[Value]) -> Vec<DecisionRow> {
    rows.iter()
        .filter_map(|v| {
            let mint = text(v, "mint");
            if mint.is_empty() {
                return None;
            }
            Some(DecisionRow {
                accepted: text(v, "decision") == "accepted",
                mint,
                pool_score: num(v, "pool_score"),
                pool_size_sol: num(v, "pool_size_sol"),
                pool_age_secs: num(v, "pool_age_secs"),
                buy_pressure_ratio: num(v, "buy_pressure_ratio"),
                reason: text(v, "reason"),
                stage: text(v, "stage"),
            })
        })
        .collect()
}

/// Realised SOL per mint, summed over sell rows.
///
/// Buys carry `pnl = 0` by construction (the position is still open), so summing every
/// row is the same as summing the sells — and stays correct if that ever changes.
pub fn realised_pnl_by_mint(trades: &[Value]) -> std::collections::HashMap<String, f64> {
    let mut out = std::collections::HashMap::new();
    for t in trades {
        let mint = text(t, "mint");
        if mint.is_empty() {
            continue;
        }
        *out.entry(mint).or_insert(0.0) += num(t, "pnl");
    }
    out
}

/// Pipeline **stages** each threshold governs.
///
/// Without this the replay is wrong in a way that reads as insight. Varying
/// `min_pool_score` applies only that threshold, so a pool the pipeline stopped at
/// `momentum_gate` or `dq_advice` "passes" — and the answer becomes "loosening the score
/// admits 1234 pools" when nine of them were about the score. The blocking stage has not
/// gone away because a different knob moved.
///
/// Keyed on `stage`, not `reason`. Reasons are free text written at each site and are
/// wildly heterogeneous in the real log — `filter_rejected`, `wrong_quote_mint`,
/// `inflow_rate=0.000;live_momentum=false`, `Fibonacci score 0.39 below threshold 0.50`,
/// `score=26.6<min=65.0`. Substring-matching that is guesswork; `stage` is a small
/// closed set the pipeline already assigns. Measured against 4000 live decisions:
/// `filters`, `momentum_gate`, `quote_mint`, `fibonacci_gate`, `dq_advice`,
/// `operator_mode`, `time_gate`, `risk`, `pool_scorer`, `buy_pool_size`.
fn governed_stages(q: &ReplayQuery) -> Vec<&'static str> {
    let mut s = Vec::new();
    if q.min_pool_score.is_some() {
        s.push("pool_scorer");
    }
    if q.min_pool_size_sol.is_some() {
        s.push("buy_pool_size");
    }
    // No dedicated stage writes an age or buy-pressure rejection today. Governing
    // nothing is the honest answer — inventing a match would resurrect exactly the
    // phantom-admission bug this function exists to kill.
    let _ = (q.max_pool_age_secs, q.min_buy_pressure_ratio);
    s
}

/// Whether a rejected pool was stopped at a stage the varied thresholds control.
fn stage_is_governed(stage: &str, governed: &[&str]) -> bool {
    let s = stage.to_lowercase();
    governed.iter().any(|g| s == *g)
}

fn passes(row: &DecisionRow, q: &ReplayQuery) -> bool {
    if let Some(min) = q.min_pool_score {
        if row.pool_score < min {
            return false;
        }
    }
    if let Some(min) = q.min_pool_size_sol {
        if row.pool_size_sol < min {
            return false;
        }
    }
    if let Some(max) = q.max_pool_age_secs {
        if row.pool_age_secs > max {
            return false;
        }
    }
    if let Some(min) = q.min_buy_pressure_ratio {
        if row.buy_pressure_ratio < min {
            return false;
        }
    }
    true
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

/// Replay `query` over measured decisions, joined to realised trade outcomes.
pub fn replay(
    decisions: &[DecisionRow],
    pnl: &std::collections::HashMap<String, f64>,
    query: &ReplayQuery,
) -> ReplayOutcome {
    let mut out = ReplayOutcome {
        decisions_considered: decisions.len(),
        ..Default::default()
    };

    let mut admitted_scores = Vec::new();
    let mut admitted_sizes = Vec::new();
    let mut admitted_ages = Vec::new();
    let mut winner_scores = Vec::new();
    let mut winner_sizes = Vec::new();
    let mut winner_ages = Vec::new();

    let governed = governed_stages(query);

    for row in decisions {
        // A rejected pool only becomes admissible if the stage that blocked it is one the
        // varied thresholds control. Otherwise its real blocker still stands.
        let clears = passes(row, query);
        let unblocked = row.accepted || stage_is_governed(&row.stage, &governed);
        let would = clears && unblocked;
        if !row.accepted && clears && !unblocked {
            out.still_blocked_elsewhere += 1;
        }
        if row.accepted {
            out.actually_accepted += 1;
        }
        if would {
            out.would_accept += 1;
        }

        let realised = pnl.get(&row.mint).copied();

        // Profile of what actually worked, for comparison against the admitted set.
        if row.accepted && realised.is_some_and(|p| p > 0.0) {
            winner_scores.push(row.pool_score);
            winner_sizes.push(row.pool_size_sol);
            winner_ages.push(row.pool_age_secs);
        }

        match (row.accepted, would) {
            // Tightening: outcome is on record.
            (true, false) => {
                out.newly_excluded += 1;
                if let Some(p) = realised {
                    out.excluded_with_known_outcome += 1;
                    out.excluded_realised_pnl_sol += p;
                    if p > 0.0 {
                        out.excluded_winners += 1;
                    } else if p < 0.0 {
                        out.excluded_losers += 1;
                    }
                }
                if out.sample_excluded.len() < SAMPLE_LIMIT {
                    out.sample_excluded.push(ChangedPool {
                        mint: row.mint.clone(),
                        pool_score: row.pool_score,
                        pool_size_sol: row.pool_size_sol,
                        pool_age_secs: row.pool_age_secs,
                        realised_pnl_sol: realised,
                        original_reason: row.reason.clone(),
                    });
                }
            }
            // Loosening: nobody bought it, so nothing knows what it would have done.
            (false, true) => {
                out.newly_admitted += 1;
                admitted_scores.push(row.pool_score);
                admitted_sizes.push(row.pool_size_sol);
                admitted_ages.push(row.pool_age_secs);
                if out.sample_admitted.len() < SAMPLE_LIMIT {
                    out.sample_admitted.push(ChangedPool {
                        mint: row.mint.clone(),
                        pool_score: row.pool_score,
                        pool_size_sol: row.pool_size_sol,
                        pool_age_secs: row.pool_age_secs,
                        realised_pnl_sol: None,
                        original_reason: row.reason.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    out.admitted_avg_pool_score = mean(&admitted_scores);
    out.admitted_avg_size_sol = mean(&admitted_sizes);
    out.admitted_avg_age_secs = mean(&admitted_ages);
    out.winner_avg_pool_score = mean(&winner_scores);
    out.winner_avg_size_sol = mean(&winner_sizes);
    out.winner_avg_age_secs = mean(&winner_ages);

    if out.newly_excluded > 0 {
        out.notes.push(format!(
            "Tightening removes {} pools that were actually traded; their realised PnL \
             ({:+.4} SOL) is exact, not estimated.",
            out.newly_excluded, out.excluded_realised_pnl_sol
        ));
    }
    if out.newly_admitted > 0 {
        out.notes.push(format!(
            "Loosening admits {} pools that were rejected. Nobody bought them, so there \
             is NO record of what they would have returned and none is estimated here. \
             Compare their measured profile against the winner profile instead.",
            out.newly_admitted
        ));
    }
    if out.newly_excluded > 0 && out.excluded_with_known_outcome < out.newly_excluded {
        out.notes.push(format!(
            "{} of the excluded pools have no matching trade — accepted by the filters \
             but never filled, or still open — and are absent from the PnL figure.",
            out.newly_excluded - out.excluded_with_known_outcome
        ));
    }
    if out.still_blocked_elsewhere > 0 {
        out.notes.push(format!(
            "{} further pools clear the proposed thresholds but were rejected for an \
             unrelated reason (DQ* veto, LP checks, deployer reputation). Changing these \
             thresholds does not admit them.",
            out.still_blocked_elsewhere
        ));
    }
    if out.newly_excluded == 0 && out.newly_admitted == 0 {
        out.notes
            .push("No pool changes fate under these thresholds.".to_string());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rows() -> Vec<Value> {
        vec![
            // Taken and profitable.
            json!({"mint":"WIN1","decision":"accepted","pool_score":80.0,"pool_size_sol":30.0,
                   "pool_age_secs":5.0,"buy_pressure_ratio":0.02,"reason":"ok"}),
            // Taken and lost.
            json!({"mint":"LOSE1","decision":"accepted","pool_score":66.0,"pool_size_sol":12.0,
                   "pool_age_secs":9.0,"buy_pressure_ratio":0.01,"reason":"ok"}),
            // Rejected on score.
            json!({"mint":"SKIP1","decision":"rejected","pool_score":58.0,"pool_size_sol":20.0,
                   "pool_age_secs":7.0,"buy_pressure_ratio":0.03,"reason":"score=58.0<min=65.0",
                   "stage":"pool_scorer"}),
        ]
    }

    fn pnl() -> std::collections::HashMap<String, f64> {
        realised_pnl_by_mint(&[
            json!({"mint":"WIN1","pnl":0.0}),
            json!({"mint":"WIN1","pnl":0.5}),
            json!({"mint":"LOSE1","pnl":-0.2}),
        ])
    }

    #[test]
    fn parses_only_rows_with_a_mint() {
        let mut r = rows();
        r.push(json!({"decision":"accepted"}));
        assert_eq!(parse_decisions(&r).len(), 3);
    }

    #[test]
    fn buys_do_not_double_count_against_sells() {
        // Buy rows carry pnl 0, so the sum is the realised total, not twice it.
        assert_eq!(pnl().get("WIN1").copied(), Some(0.5));
    }

    #[test]
    fn tightening_prices_the_pools_it_removes() {
        let d = parse_decisions(&rows());
        let q = ReplayQuery { min_pool_score: Some(70.0), ..Default::default() };
        let out = replay(&d, &pnl(), &q);

        // LOSE1 (66) drops out; WIN1 (80) survives; SKIP1 was never in.
        assert_eq!(out.newly_excluded, 1);
        assert_eq!(out.newly_admitted, 0);
        assert!((out.excluded_realised_pnl_sol + 0.2).abs() < 1e-9);
        assert_eq!(out.excluded_losers, 1);
    }

    #[test]
    fn loosening_counts_but_never_prices() {
        let d = parse_decisions(&rows());
        let q = ReplayQuery { min_pool_score: Some(55.0), ..Default::default() };
        let out = replay(&d, &pnl(), &q);

        assert_eq!(out.newly_admitted, 1);
        // The entire point: no PnL is attributed to a pool nobody bought.
        assert_eq!(out.excluded_realised_pnl_sol, 0.0);
        assert!(out.sample_admitted.iter().all(|p| p.realised_pnl_sol.is_none()));
        assert!(out.notes.iter().any(|n| n.contains("NO record")));
    }

    #[test]
    fn winner_profile_comes_only_from_profitable_taken_pools() {
        let d = parse_decisions(&rows());
        let out = replay(&d, &pnl(), &ReplayQuery { min_pool_score: Some(55.0), ..Default::default() });
        // WIN1 only — LOSE1 was taken but lost, SKIP1 was never taken.
        assert_eq!(out.winner_avg_pool_score, 80.0);
    }

    #[test]
    fn a_pool_blocked_for_another_reason_is_not_admitted() {
        // The bug this caught on real data: loosening the score "admitted" a thousand
        // pools that were actually stopped by the DQ* veto, which the score does not
        // control.
        let mut r = rows();
        r.push(json!({"mint":"VETOED","decision":"rejected","pool_score":90.0,
                      "pool_size_sol":25.0,"pool_age_secs":6.0,"buy_pressure_ratio":0.05,
                      "reason":"dq_star_veto","stage":"dq_advice"}));
        let d = parse_decisions(&r);
        let out = replay(&d, &pnl(), &ReplayQuery { min_pool_score: Some(55.0), ..Default::default() });

        // SKIP1 was rejected on score and is admitted; VETOED was not and is still out.
        assert_eq!(out.newly_admitted, 1);
        assert!(out.sample_admitted.iter().all(|p| p.mint != "VETOED"));
        assert_eq!(out.still_blocked_elsewhere, 1);
        assert!(out.notes.iter().any(|n| n.contains("unrelated reason")));
    }

    #[test]
    fn an_unchanged_threshold_changes_nothing() {
        let d = parse_decisions(&rows());
        let out = replay(&d, &pnl(), &ReplayQuery::default());
        assert_eq!(out.newly_excluded, 0);
        // An empty query varies nothing, so it governs no rejection reason and cannot
        // admit anything. A no-op query must be a no-op.
        assert_eq!(out.newly_admitted, 0);
        assert!(ReplayQuery::default().is_empty());
    }

    #[test]
    fn accepted_pools_with_no_trade_are_excluded_from_pnl_not_hidden() {
        let mut r = rows();
        r.push(json!({"mint":"NOFILL","decision":"accepted","pool_score":61.0,
                      "pool_size_sol":8.0,"pool_age_secs":4.0,"buy_pressure_ratio":0.01,
                      "reason":"ok"}));
        let d = parse_decisions(&r);
        let out = replay(&d, &pnl(), &ReplayQuery { min_pool_score: Some(70.0), ..Default::default() });

        assert_eq!(out.newly_excluded, 2);
        assert_eq!(out.excluded_with_known_outcome, 1);
        assert!(out.notes.iter().any(|n| n.contains("no matching trade")));
    }
}
