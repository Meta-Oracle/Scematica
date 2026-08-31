//! Was the agent right? — asked of its own recorded advice.
//!
//! The sniper writes `dq_action` and `dq_confidence` onto every pool decision the agent
//! advised on. That is a record of what the net believed, made before the outcome existed,
//! which is the only kind of claim worth scoring. This module scores it.
//!
//! ## The asymmetry, which is the whole design
//!
//! It is the same one `scematica_sniper::calibration` applies to Scylar's claims and
//! `scema_policy` applies to declined branches, and it is not a limitation to be papered
//! over:
//!
//! - Advice on a pool the bot **bought** resolves. The trade settled, the PnL is recorded,
//!   and the agent's lean can be compared against it.
//! - Advice that **vetoed** a pool does not resolve, and never will. Nobody bought it, so
//!   no outcome exists — and the counterfactual "it would have lost money" is exactly the
//!   claim under test. Imputing one would mean the agent grading its own homework, and the
//!   grade would then improve every time it became more conservative.
//!
//! So unresolved advice is **counted, never scored**, and [`Calibration::mean_abs_error`]
//! is `None` rather than `0.0` when nothing resolved. A zero there would read as perfect
//! calibration achieved by refusing to act, which is the most flattering possible reading
//! of a policy that has stopped trading.
//!
//! ## Why the action histogram is here and not somewhere it could be ignored
//!
//! The first thing this module was pointed at found that the agent had advised on 399 pools
//! and emitted **`SELL_PARTIAL` on all 399** — one value, no variance, across three months.
//! That is not a calibration result; it is the reason a calibration result is unobtainable,
//! and the two must not be reported as if they were the same thing.
//!
//! It is also the identical failure this repository has now hit three times, one layer up
//! each time: a gate reading a filter input that never varies, a Ψ term pinned at zero, and
//! now a policy whose argmax never moves. In each case the number *looked* like a decision
//! and was a constant. So [`Verdict`] names the cause rather than returning a score, and
//! [`Calibration::action_never_varied`] is a first-class answer — the same shape as omni's
//! five abstention reasons, for the same reason: "cannot be scored" has several distinct
//! causes and each one asks something different of the operator.
//!
//! ## A score a constant policy can still earn
//!
//! The first real run of this module produced `mean abs error 0.0000` on the sixteen pieces
//! of advice that did resolve — and that number is worthless, because the advice was the same
//! label every time. A policy that always says "bearish" is scored perfectly in any window
//! where every trade lost money. It has learned the base rate, and the error term cannot tell
//! that apart from skill.
//!
//! So [`Calibration::score_is_base_rate`] exists and the report says so on the same line as
//! the number. Hiding the score would be the other error — it *is* the measured quantity —
//! but printing it unqualified next to a constant policy is how a degenerate agent gets
//! congratulated.
//!
//! Nothing here reads a file or a clock. The sniper's `measure` binary feeds it, so this
//! crate stays free of the log format and can be tested with three lines of input.

use std::collections::BTreeMap;

use crate::action::TradeAction;

/// One piece of advice the agent gave, as recorded at the time.
#[derive(Debug, Clone)]
pub struct Advice {
    /// The action label as written to the log. Kept as a string rather than parsed into a
    /// [`TradeAction`]: the log holds months of output from builds whose action set may not
    /// match this one, and silently mapping an unrecognised label onto a known variant would
    /// invent agreement. See [`Advice::action`] for the parse that is allowed to fail.
    pub action: String,
    /// The magnitude the sniper recorded alongside it. `None` when the build did not write
    /// one — which is not the same as a confidence of zero.
    pub confidence: Option<f64>,
    /// Whether the advice actually changed what happened, as opposed to being recorded
    /// during warm-up while the agent was not being enforced.
    ///
    /// `None` when the source could not say. The sniper logs the *advice* identically in
    /// both cases and only the branch it took afterwards differs, so for most records this
    /// is genuinely unknown — and an unknown that defaulted to `true` would credit the agent
    /// for calls that never touched a position.
    pub enforced: Option<bool>,
    /// Realised PnL of the trade this advice preceded, if one settled.
    ///
    /// `None` is the normal case for a veto and carries no information about the outcome —
    /// there is no outcome. It must never be read as a flat result.
    pub realised_pnl: Option<f64>,
}

impl Advice {
    /// The advice as a known action, or `None` if this build does not have that variant.
    pub fn action(&self) -> Option<TradeAction> {
        match self.action.as_str() {
            "HOLD" => Some(TradeAction::Hold),
            "BUY" => Some(TradeAction::BuyStandard),
            "BUY_AGG" => Some(TradeAction::BuyAggressive),
            "SELL_PARTIAL" => Some(TradeAction::SellPartial),
            "SELL_ALL" => Some(TradeAction::SellAll),
            _ => None,
        }
    }

    /// Which way this advice leaned. `None` for `HOLD` and for an unrecognised label.
    fn lean(&self) -> Option<bool> {
        let a = self.action()?;
        if a.is_buy() {
            Some(true)
        } else if a.is_sell() {
            Some(false)
        } else {
            None
        }
    }
}

/// Why a calibration could not be produced, or that it could.
///
/// Each variant is a different instruction to the operator, which is the point of naming
/// them rather than returning an `Option<f64>` and letting the caller guess.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The log contains no advice at all. Either the agent never reached
    /// `ready_to_advise`, or this window predates the instrumentation.
    NoAdvice,
    /// Advice exists and is a constant. Not a calibration result — the reason there cannot
    /// be one. Check `last_q_values` before touching any threshold: value dispersion is not
    /// action dispersion, and an argmax can be pinned while the Q-values move freely.
    ActionNeverVaried { action: String, n: usize },
    /// Advice varies, but every piece of it vetoed, so nothing resolved. The agent has
    /// stopped the bot from generating the outcomes its own calibration would need.
    AllUnresolved { n: usize },
    /// Enough resolved to say something.
    Scored {
        resolved: usize,
        mean_abs_error: f64,
    },
}

impl Verdict {
    /// One line, for a terminal.
    pub fn headline(&self) -> String {
        match self {
            Self::NoAdvice => "no advice recorded in this window".into(),
            Self::ActionNeverVaried { action, n } => {
                format!("advice never varied: {action} on all {n} — this is why there is no score")
            }
            Self::AllUnresolved { n } => {
                format!("{n} piece(s) of advice, none resolved — every one vetoed a buy")
            }
            Self::Scored {
                resolved,
                mean_abs_error,
            } => {
                format!("{resolved} resolved, mean absolute error {mean_abs_error:.4}")
            }
        }
    }
}

/// The agent's record against its own past advice.
#[derive(Debug, Clone, Default)]
pub struct Calibration {
    /// Every piece of advice seen, by label. The distribution *is* a finding.
    pub actions: BTreeMap<String, usize>,
    /// Advice that preceded a settled trade.
    pub resolved: usize,
    /// Advice with no outcome, because nothing was bought. Counted, never scored.
    pub unresolved: usize,
    /// Advice known to have been recorded while the agent was not being enforced. Real
    /// advice, no consequence.
    pub warm_up: usize,
    /// Advice whose enforcement the source could not determine. Reported rather than folded
    /// into either bucket.
    pub enforcement_unknown: usize,
    /// Sum of |error| over the resolved subset, where error is the signed disagreement
    /// between the lean and the realised sign.
    error_sum: f64,
    /// Resolved advice whose lean matched the realised sign.
    pub correct: usize,
    /// Resolved advice that carried no lean (`HOLD`, or a label this build does not know).
    pub no_lean: usize,
    /// Confidence values seen on resolved advice, for the correlation below.
    conf: Vec<(f64, f64)>,
}

impl Calibration {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold in one recorded piece of advice.
    pub fn observe(&mut self, a: &Advice) {
        *self.actions.entry(a.action.clone()).or_insert(0) += 1;
        match a.enforced {
            Some(false) => self.warm_up += 1,
            None => self.enforcement_unknown += 1,
            Some(true) => {}
        }

        let Some(pnl) = a.realised_pnl else {
            self.unresolved += 1;
            return;
        };
        self.resolved += 1;

        let Some(bullish) = a.lean() else {
            // A `HOLD` makes no directional claim, so there is nothing to be right or wrong
            // about. Scoring it as a miss would penalise the one honest answer available to
            // a policy that does not know.
            self.no_lean += 1;
            return;
        };

        // The claim is directional, so the error is directional: 0 when the lean matched the
        // sign of the outcome, 1 when it did not. Deliberately not scaled by |pnl| — a
        // magnitude-weighted error rewards being right about large moves, and the agent's
        // advice is about *whether* to enter, not about how far it will run.
        let realised_up = pnl > 0.0;
        if realised_up == bullish {
            self.correct += 1;
        } else {
            self.error_sum += 1.0;
        }

        if let Some(c) = a.confidence {
            self.conf
                .push((c, if realised_up == bullish { 1.0 } else { 0.0 }));
        }
    }

    /// Total advice seen.
    pub fn advised(&self) -> usize {
        self.actions.values().sum()
    }

    /// Mean absolute error over the **resolved, directional** subset.
    ///
    /// `None` when nothing resolved directionally. Not `0.0`: a zero there says the agent
    /// was never wrong, and "never had the chance to be" is a different sentence.
    pub fn mean_abs_error(&self) -> Option<f64> {
        let n = self.resolved.checked_sub(self.no_lean)?;
        if n == 0 {
            return None;
        }
        Some(self.error_sum / n as f64)
    }

    /// True when a score exists but cannot distinguish skill from the base rate.
    ///
    /// A constant policy is right exactly as often as its one answer happens to be right,
    /// which in a losing window is always. The number is real and it is not a measurement of
    /// the agent.
    pub fn score_is_base_rate(&self) -> bool {
        self.action_never_varied() && self.mean_abs_error().is_some()
    }

    /// True when every piece of advice carried the same label.
    ///
    /// The standing form of the finding in the module note. One value across a window is a
    /// constant wearing a decision's clothes, and no threshold change can help it.
    pub fn action_never_varied(&self) -> bool {
        self.actions.len() == 1 && self.advised() > 1
    }

    /// Does higher confidence actually mean more often right?
    ///
    /// `None` unless there are enough resolved, directional, confidence-carrying samples for
    /// the question to mean anything, and unless the confidence itself varied — a constant
    /// input has no correlation with anything, and the formula would divide by zero.
    pub fn confidence_discriminates(&self) -> Option<f64> {
        const MIN: usize = 20;
        if self.conf.len() < MIN {
            return None;
        }
        let n = self.conf.len() as f64;
        let mx = self.conf.iter().map(|(x, _)| x).sum::<f64>() / n;
        let my = self.conf.iter().map(|(_, y)| y).sum::<f64>() / n;
        let mut num = 0.0;
        let mut dx = 0.0;
        let mut dy = 0.0;
        for (x, y) in &self.conf {
            num += (x - mx) * (y - my);
            dx += (x - mx) * (x - mx);
            dy += (y - my) * (y - my);
        }
        if dx <= f64::EPSILON || dy <= f64::EPSILON {
            return None;
        }
        Some(num / (dx.sqrt() * dy.sqrt()))
    }

    /// What can honestly be said.
    pub fn verdict(&self) -> Verdict {
        if self.advised() == 0 {
            return Verdict::NoAdvice;
        }
        // Order matters. A degenerate policy is *also* all-unresolved, and reporting the
        // second hides the first — which is the actionable one.
        if self.action_never_varied() {
            let (action, n) = self
                .actions
                .iter()
                .next()
                .map(|(k, v)| (k.clone(), *v))
                .unwrap();
            return Verdict::ActionNeverVaried { action, n };
        }
        match self.mean_abs_error() {
            Some(e) => Verdict::Scored {
                resolved: self.resolved,
                mean_abs_error: e,
            },
            None => Verdict::AllUnresolved { n: self.advised() },
        }
    }

    /// A short report. Every unmeasured quantity prints `—`, never a zero.
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("DQ* advice: {} recorded\n", self.advised()));
        for (label, n) in &self.actions {
            let share = 100.0 * *n as f64 / self.advised().max(1) as f64;
            s.push_str(&format!("  {label:<14} {n:>6}  {share:>5.1}%\n"));
        }
        let warm = if self.enforcement_unknown == self.advised() {
            "—".to_string()
        } else {
            format!("{}", self.warm_up)
        };
        s.push_str(&format!(
            "  resolved {}   unresolved {}   warm-up {}   enforcement unknown {}\n",
            self.resolved, self.unresolved, warm, self.enforcement_unknown
        ));
        let mae = match self.mean_abs_error() {
            Some(e) if self.score_is_base_rate() => {
                format!("{e:.4} (the base rate — advice never varied, so this is not skill)")
            }
            Some(e) => format!("{e:.4}"),
            None => "—".into(),
        };
        let disc = match self.confidence_discriminates() {
            Some(c) => format!("{c:+.3}"),
            None => "—".into(),
        };
        s.push_str(&format!("  mean abs error {mae}\n"));
        s.push_str(&format!("  confidence↔correctness {disc}\n"));
        s.push_str(&format!("  {}\n", self.verdict().headline()));
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advice(action: &str, pnl: Option<f64>) -> Advice {
        Advice {
            action: action.into(),
            confidence: Some(1.0),
            enforced: Some(true),
            realised_pnl: pnl,
        }
    }

    #[test]
    fn nothing_resolved_is_none_and_never_zero() {
        // A zero here reads as perfect calibration achieved by refusing to act, which is the
        // most flattering possible reading of a policy that has stopped trading.
        let mut c = Calibration::new();
        for _ in 0..10 {
            c.observe(&advice("SELL_PARTIAL", None));
        }
        assert_eq!(c.mean_abs_error(), None);
        assert_eq!(c.unresolved, 10);
        assert_eq!(c.resolved, 0);
    }

    #[test]
    fn a_constant_policy_is_named_as_such_rather_than_scored() {
        // The finding that prompted this module: 399 pieces of advice, one value. That is
        // not a calibration result, it is the reason there cannot be one.
        let mut c = Calibration::new();
        for _ in 0..399 {
            c.observe(&advice("SELL_PARTIAL", None));
        }
        assert!(c.action_never_varied());
        match c.verdict() {
            Verdict::ActionNeverVaried { action, n } => {
                assert_eq!(action, "SELL_PARTIAL");
                assert_eq!(n, 399);
            }
            v => panic!("expected ActionNeverVaried, got {v:?}"),
        }
    }

    #[test]
    fn a_degenerate_policy_outranks_all_unresolved_in_the_verdict() {
        // Both are true of the same data. Reporting "nothing resolved" hides the reason, and
        // the reason is the part somebody can act on.
        let mut c = Calibration::new();
        for _ in 0..5 {
            c.observe(&advice("SELL_ALL", None));
        }
        assert!(matches!(c.verdict(), Verdict::ActionNeverVaried { .. }));
    }

    #[test]
    fn varied_advice_with_no_outcomes_is_all_unresolved() {
        let mut c = Calibration::new();
        c.observe(&advice("SELL_ALL", None));
        c.observe(&advice("SELL_PARTIAL", None));
        assert_eq!(c.verdict(), Verdict::AllUnresolved { n: 2 });
    }

    #[test]
    fn a_correct_bullish_call_scores_zero_error() {
        let mut c = Calibration::new();
        c.observe(&advice("BUY", Some(0.4)));
        assert_eq!(c.mean_abs_error(), Some(0.0));
        assert_eq!(c.correct, 1);
    }

    #[test]
    fn a_wrong_bullish_call_scores_full_error() {
        let mut c = Calibration::new();
        c.observe(&advice("BUY", Some(-0.4)));
        assert_eq!(c.mean_abs_error(), Some(1.0));
        assert_eq!(c.correct, 0);
    }

    #[test]
    fn a_hold_is_not_scored_because_it_claims_no_direction() {
        // Scoring it as a miss would penalise the one honest answer available to a policy
        // that does not know which way a pool will go.
        let mut c = Calibration::new();
        c.observe(&advice("HOLD", Some(0.9)));
        assert_eq!(c.no_lean, 1);
        assert_eq!(c.resolved, 1);
        assert_eq!(c.mean_abs_error(), None);
    }

    #[test]
    fn an_unknown_label_is_not_mapped_onto_a_known_action() {
        // The log holds months of output from builds whose action set may differ. Guessing
        // would invent agreement between two policies that never agreed.
        let a = advice("BUY_MOON", Some(1.0));
        assert!(a.action().is_none());
        let mut c = Calibration::new();
        c.observe(&a);
        assert_eq!(c.no_lean, 1);
        assert!(c.actions.contains_key("BUY_MOON"));
    }

    #[test]
    fn magnitude_does_not_weight_the_error() {
        // A magnitude-weighted error rewards being right about large moves; the advice is
        // about whether to enter, not how far it runs.
        let mut small = Calibration::new();
        small.observe(&advice("BUY", Some(-0.001)));
        let mut large = Calibration::new();
        large.observe(&advice("BUY", Some(-500.0)));
        assert_eq!(small.mean_abs_error(), large.mean_abs_error());
    }

    #[test]
    fn confidence_correlation_needs_samples_and_variance() {
        let mut c = Calibration::new();
        for _ in 0..10 {
            c.observe(&advice("BUY", Some(1.0)));
        }
        assert_eq!(
            c.confidence_discriminates(),
            None,
            "10 samples is not a correlation"
        );

        // Enough samples, but the confidence never moved: correlating a constant with
        // anything is a division by zero, not a finding.
        let mut flat = Calibration::new();
        for i in 0..40 {
            flat.observe(&advice("BUY", Some(if i % 2 == 0 { 1.0 } else { -1.0 })));
        }
        assert_eq!(flat.confidence_discriminates(), None);
    }

    #[test]
    fn confidence_that_tracks_correctness_shows_up_positive() {
        let mut c = Calibration::new();
        for i in 0..40 {
            let right = i % 2 == 0;
            c.observe(&Advice {
                action: "BUY".into(),
                confidence: Some(if right { 9.0 } else { 1.0 }),
                enforced: Some(true),
                realised_pnl: Some(if right { 1.0 } else { -1.0 }),
            });
        }
        let r = c.confidence_discriminates().expect("enough varied samples");
        assert!(r > 0.9, "correlation was {r}");
    }

    #[test]
    fn warm_up_advice_is_counted_apart_from_enforced_advice() {
        // Real advice with no consequence. Folding it in would credit the agent for calls
        // that never touched a position.
        let mut c = Calibration::new();
        c.observe(&Advice {
            action: "BUY".into(),
            confidence: None,
            enforced: Some(false),
            realised_pnl: Some(1.0),
        });
        assert_eq!(c.warm_up, 1);
        assert_eq!(c.resolved, 1);
        assert_eq!(c.enforcement_unknown, 0);
    }

    #[test]
    fn unknown_enforcement_is_its_own_bucket_and_not_credited_as_enforced() {
        // Defaulting an unknown to `true` would credit the agent for calls that never
        // touched a position — the same class of error as an unmeasured term rendering 0.00.
        let mut c = Calibration::new();
        c.observe(&Advice {
            action: "BUY".into(),
            confidence: None,
            enforced: None,
            realised_pnl: None,
        });
        assert_eq!(c.enforcement_unknown, 1);
        assert_eq!(c.warm_up, 0);
        assert!(c.report().contains("warm-up —"), "{}", c.report());
    }

    #[test]
    fn a_constant_policy_that_scores_well_is_marked_as_measuring_the_base_rate() {
        // The first real run: sixteen `SELL_PARTIAL` calls, sixteen losing trades, a perfect
        // score. A policy that always says bearish is right in every losing window, and the
        // error term cannot tell that apart from skill.
        let mut c = Calibration::new();
        for _ in 0..16 {
            c.observe(&advice("SELL_PARTIAL", Some(-1.0)));
        }
        assert_eq!(c.mean_abs_error(), Some(0.0));
        assert!(c.score_is_base_rate());
        let r = c.report();
        assert!(r.contains("base rate"), "{r}");
        assert!(r.contains("not skill"), "{r}");
    }

    #[test]
    fn a_varied_policy_score_is_not_marked_as_a_base_rate() {
        let mut c = Calibration::new();
        c.observe(&advice("BUY", Some(1.0)));
        c.observe(&advice("SELL_ALL", Some(-1.0)));
        assert!(!c.score_is_base_rate());
        assert!(!c.report().contains("base rate"));
    }

    #[test]
    fn the_score_is_still_printed_beside_the_caveat_not_replaced_by_it() {
        // Hiding it would be the other error: it is the measured quantity, and a reader who
        // cannot see it has to take the caveat on faith.
        let mut c = Calibration::new();
        for _ in 0..5 {
            c.observe(&advice("SELL_PARTIAL", Some(-1.0)));
        }
        assert!(c.report().contains("0.0000"), "{}", c.report());
    }

    #[test]
    fn an_empty_window_says_so_rather_than_scoring_nothing() {
        assert_eq!(Calibration::new().verdict(), Verdict::NoAdvice);
        assert_eq!(Calibration::new().mean_abs_error(), None);
    }

    #[test]
    fn one_piece_of_advice_is_not_a_constant_policy() {
        // `never varied` over a single sample is a statement about the sample size, not
        // about the policy.
        let mut c = Calibration::new();
        c.observe(&advice("SELL_ALL", None));
        assert!(!c.action_never_varied());
    }

    #[test]
    fn the_report_prints_an_em_dash_for_what_was_not_measured() {
        let mut c = Calibration::new();
        for _ in 0..3 {
            c.observe(&advice("SELL_PARTIAL", None));
        }
        let r = c.report();
        assert!(r.contains("mean abs error —"), "{r}");
        assert!(
            !r.contains("0.0000"),
            "an unmeasured error must not print as a number: {r}"
        );
    }
}
