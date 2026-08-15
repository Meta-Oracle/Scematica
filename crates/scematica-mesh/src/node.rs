//! Nodes: the decision-making units of the running system, and what is known about each.
//!
//! The central type here is [`Provenance`], and it carries the whole design. Every other
//! observability surface in this repo answers "what is the value?"; this one answers
//! "**can this value be seen at all, and how recently?**" first, and only then reports it.
//!
//! That ordering is not stylistic. A dashboard that renders an unreadable node as `0`
//! makes a claim — *this unit did nothing* — that is indistinguishable on screen from the
//! true statement *we cannot see this unit*. One is an observation and the other is an
//! accusation, exactly as with a vault balance that could not be read. So [`Provenance`]
//! has an [`Provenance::Absent`] arm with no value attached, and the renderer is expected
//! to draw it as dark rather than as idle.

use serde::{Deserialize, Serialize};

/// Where a node's numbers came from, and whether they can still be believed.
///
/// The three "we have data" arms are separated by *measured age against that source's own
/// budget*, never a shared constant — see [`crate::collect::Source`]. The sniper rewrites
/// its metrics every 5 seconds while the LLM strategy agent may not write for an hour, so
/// a single global staleness threshold would either flag a healthy strategy file forever
/// or never flag a dead sniper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Provenance {
    /// Read from a real source, within that source's freshness budget.
    Live { age_secs: u64 },
    /// Read from a real source, but older than its budget. The values are real and were
    /// true once; they are not true *now* and must not be presented as current.
    Stale { age_secs: u64, budget_secs: u64 },
    /// The source does not exist. **Not zero, not idle — unseen.** A node in this state
    /// has no values at all, which is why the arm carries none.
    Absent,
    /// The value came from a simulation backend rather than a running system. Kept as a
    /// distinct arm rather than folded into `Live` because the whole web app is built on
    /// the rule that simulated output is labelled at every point it surfaces.
    Simulated,
}

impl Provenance {
    /// Does this node have values a reader may act on?
    ///
    /// `Stale` is deliberately **not** actionable. Values that were true an hour ago look
    /// exactly like values that are true now, and that resemblance is the entire hazard.
    pub fn is_actionable(&self) -> bool {
        matches!(self, Provenance::Live { .. })
    }

    /// Is anything at all known about this node?
    pub fn is_visible(&self) -> bool {
        !matches!(self, Provenance::Absent)
    }
}

/// What a node is, structurally. Drives grouping, iconography and colour family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    /// A source of pool events (Raydium, Pump.fun, whale-copy).
    Listener,
    /// One stage of the filter pipeline.
    Filter,
    /// A scoring unit that produces a number rather than a pass/fail.
    Scorer,
    /// An independent risk breaker that can halt buys on its own.
    Breaker,
    /// A learner — the DQ* agent or one tournament variant.
    Learner,
    /// An LLM agent (strategy, risk, debate, report).
    Reasoner,
    /// The Ψ data-integrity gate.
    Gate,
    /// Order construction and submission.
    Executor,
    /// A remote participant reached over the ScemaDEX relay.
    Peer,
}

impl NodeKind {
    /// Column the renderer places this kind in. Left-to-right is the direction a pool
    /// event actually travels, so an edge that points backwards is visibly a feedback
    /// path rather than an ordinary flow.
    pub fn layer(&self) -> u8 {
        match self {
            NodeKind::Listener => 0,
            NodeKind::Filter | NodeKind::Scorer => 1,
            NodeKind::Breaker => 2,
            NodeKind::Learner | NodeKind::Reasoner | NodeKind::Gate => 3,
            NodeKind::Executor => 4,
            NodeKind::Peer => 5,
        }
    }
}

/// What a node last decided.
///
/// `Unknown` exists so a visible node with an unreadable decision is distinguishable from
/// one that decided to allow. Absence of a veto is not evidence of a pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Allowing flow through.
    Pass,
    /// Actively blocking. The one verdict the renderer is allowed to make alarming.
    Veto,
    /// Not blocking, but damping — the DQ*'s weak-lean size-down, for instance.
    Damp,
    /// Operating, but on degraded inputs.
    Degraded,
    /// Idle by design, with nothing to decide.
    Idle,
    /// Visible, but its decision could not be determined.
    Unknown,
}

/// One decision-making unit, as observed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    /// Stable dotted identifier, e.g. `learner.dqstar`. Edges reference these, and the
    /// renderer keys animation state off them, so they must not change between polls.
    pub id: String,
    pub kind: NodeKind,
    /// Short display name.
    pub label: String,
    /// One line explaining what this unit does, for the reader who has never seen it.
    pub blurb: String,
    pub provenance: Provenance,
    pub verdict: Verdict,
    /// Normalised 0..1 throughput or utilisation, when the source supports one.
    /// `None` means not measurable — which the renderer must not draw as `0.0`.
    pub activity: Option<f64>,
    /// Ordered key/value facts, rendered verbatim in the node's detail panel.
    pub detail: Vec<(String, String)>,
    /// Why this node holds its current verdict, in one sentence, when there is something
    /// worth saying. This is the field that turns the picture into a diagnosis.
    pub reason: Option<String>,
}

impl Node {
    /// A node whose source could not be found.
    ///
    /// Constructed through its own function so that every dark node in the system is
    /// necessarily consistent: `Absent` provenance, `Unknown` verdict, no activity value,
    /// no invented details.
    pub fn absent(id: &str, kind: NodeKind, label: &str, blurb: &str) -> Self {
        Node {
            id: id.to_string(),
            kind,
            label: label.to_string(),
            blurb: blurb.to_string(),
            provenance: Provenance::Absent,
            verdict: Verdict::Unknown,
            activity: None,
            detail: Vec::new(),
            reason: Some("no source on disk — this unit is unseen, not idle".to_string()),
        }
    }
}

/// The DQ* buy-veto, evaluated exactly as `sniper.rs` evaluates it.
///
/// `crates/scematica-sniper/src/sniper.rs` is authoritative; this mirrors it because the
/// mesh's whole claim is that it shows what the system is really doing, and a veto
/// indicator computed from a different rule than the veto would be worse than none.
///
/// The rule: take the better of the two buy actions, take the Q of the chosen bearish
/// action, and call the veto decisive when the bearish Q clears the buy Q by
/// `NN_VETO_REL_MARGIN`. A weaker lean does not suppress the buy — it downgrades sizing,
/// which is [`Verdict::Damp`] rather than [`Verdict::Veto`].
pub const NN_VETO_REL_MARGIN: f64 = 0.15;

/// Result of evaluating the Q-vector against the veto rule.
#[derive(Clone, Debug, PartialEq)]
pub struct VetoAnalysis {
    pub verdict: Verdict,
    pub buy_q: f64,
    pub bearish_q: f64,
    /// How far the bearish action leads the best buy, as a ratio. `2.0` means the bearish
    /// Q is three times the buy Q.
    pub lead_ratio: Option<f64>,
    pub reason: String,
}

/// Evaluate the veto from a raw Q-vector and the agent's readiness flag.
///
/// Indices come from [`scematica_nn::action::TradeAction`] rather than integer literals.
/// The order is load-bearing — reading `Q[1]` as a buy when it is a sell inverts the
/// diagnosis — and pinning it to the producing crate's own enum means a reordering there
/// is a compile error here instead of a wrong colour on a chart.
pub fn analyse_veto(q_values: &[f64], ready_to_advise: bool) -> VetoAnalysis {
    use scematica_nn::action::TradeAction;

    let q = |a: TradeAction| q_values.get(a.index()).copied();

    let buy_q = match (q(TradeAction::BuyStandard), q(TradeAction::BuyAggressive)) {
        (Some(a), Some(b)) => a.max(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => {
            return VetoAnalysis {
                verdict: Verdict::Unknown,
                buy_q: f64::NAN,
                bearish_q: f64::NAN,
                lead_ratio: None,
                reason: "Q-vector too short to contain the buy actions".to_string(),
            }
        }
    };
    let bearish_q = match (q(TradeAction::SellPartial), q(TradeAction::SellAll)) {
        (Some(a), Some(b)) => a.max(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => {
            return VetoAnalysis {
                verdict: Verdict::Unknown,
                buy_q: f64::NAN,
                bearish_q: f64::NAN,
                lead_ratio: None,
                reason: "Q-vector too short to contain the sell actions".to_string(),
            }
        }
    };

    // Below the training threshold the agent has no say at all, so its Q-vector — however
    // lopsided — is not vetoing anything. Reporting a veto here would send an operator
    // hunting a gate that is not closed.
    if !ready_to_advise {
        return VetoAnalysis {
            verdict: Verdict::Idle,
            buy_q,
            bearish_q,
            lead_ratio: None,
            reason: "not yet advising — under the 10k train-step threshold, so it neither sizes nor vetoes".to_string(),
        };
    }

    let decisive = bearish_q > 0.0 && (buy_q <= 0.0 || bearish_q >= buy_q * (1.0 + NN_VETO_REL_MARGIN));
    let lead_ratio = if buy_q > 0.0 { Some(bearish_q / buy_q) } else { None };

    if decisive {
        let how = match lead_ratio {
            Some(r) => format!("bearish Q leads the best buy by {:.1}x", r),
            None => "best buy Q is non-positive".to_string(),
        };
        VetoAnalysis {
            verdict: Verdict::Veto,
            buy_q,
            bearish_q,
            lead_ratio,
            reason: format!("suppressing buys — {how} (threshold is +{:.0}%)", NN_VETO_REL_MARGIN * 100.0),
        }
    } else if bearish_q > buy_q {
        VetoAnalysis {
            verdict: Verdict::Damp,
            buy_q,
            bearish_q,
            lead_ratio,
            reason: "leaning bearish but under the veto margin — buys are sized down, not blocked".to_string(),
        }
    } else {
        VetoAnalysis {
            verdict: Verdict::Pass,
            buy_q,
            bearish_q,
            lead_ratio,
            reason: "buy actions lead — not gating entries".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live `scematica-nn-stats.json` captured while building this crate. It is a
    /// full veto, and it is the reason the bot had 0 trades attempted against 171 pools
    /// passed — which is precisely the diagnosis this whole feature exists to make
    /// visible at a glance instead of by hand.
    #[test]
    fn the_real_captured_q_vector_is_a_full_veto() {
        let q = [42.975_571_727_974_99, 5.209_099_758_311_705, 12.720_168_124_042_097, 43.030_303_01, 40.0];
        let a = analyse_veto(&q, true);
        assert_eq!(a.verdict, Verdict::Veto);
        assert_eq!(a.buy_q, 12.720_168_124_042_097, "best buy is BuyAggressive, not Hold");
        assert_eq!(a.bearish_q, 43.030_303_01);
        assert!(a.lead_ratio.unwrap() > 3.0);
    }

    /// Hold sits at index 0 and is not a buy. Folding it into `buy_q` would make the
    /// captured vector above read as *no veto*, because Hold's 42.98 nearly matches the
    /// bearish 43.03 — the exact wrong answer, from one index.
    #[test]
    fn hold_is_not_counted_as_a_buy() {
        let q = [100.0, 1.0, 1.0, 50.0, 0.0];
        let a = analyse_veto(&q, true);
        assert_eq!(a.buy_q, 1.0);
        assert_eq!(a.verdict, Verdict::Veto);
    }

    #[test]
    fn a_weak_bearish_lean_damps_rather_than_vetoes() {
        // 10% lead, under the 15% margin.
        let a = analyse_veto(&[0.0, 10.0, 9.0, 11.0, 0.0], true);
        assert_eq!(a.verdict, Verdict::Damp);
    }

    #[test]
    fn a_bullish_vector_passes() {
        let a = analyse_veto(&[1.0, 50.0, 60.0, 5.0, 2.0], true);
        assert_eq!(a.verdict, Verdict::Pass);
    }

    /// A lopsided vector from an agent that is not advising yet gates nothing.
    #[test]
    fn an_untrained_agent_is_idle_not_vetoing() {
        let a = analyse_veto(&[0.0, 1.0, 1.0, 900.0, 0.0], false);
        assert_eq!(a.verdict, Verdict::Idle);
    }

    /// A non-positive best buy is a veto whenever the bearish side is positive at all,
    /// matching the `buy_q <= 0.0` branch in sniper.rs.
    #[test]
    fn non_positive_buy_q_vetoes_on_any_positive_bearish() {
        let a = analyse_veto(&[0.0, -5.0, -3.0, 0.001, 0.0], true);
        assert_eq!(a.verdict, Verdict::Veto);
        assert_eq!(a.lead_ratio, None, "no ratio is defined against a non-positive buy");
    }

    #[test]
    fn a_truncated_q_vector_is_unknown_not_zero() {
        let a = analyse_veto(&[1.0, 2.0], true);
        assert_eq!(a.verdict, Verdict::Unknown);
    }

    #[test]
    fn absent_nodes_carry_no_values() {
        let n = Node::absent("learner.x", NodeKind::Learner, "X", "y");
        assert_eq!(n.provenance, Provenance::Absent);
        assert_eq!(n.verdict, Verdict::Unknown);
        assert!(n.activity.is_none(), "an unseen node must not report activity");
        assert!(n.detail.is_empty());
    }

    #[test]
    fn stale_data_is_never_actionable() {
        assert!(Provenance::Live { age_secs: 1 }.is_actionable());
        assert!(!Provenance::Stale { age_secs: 900, budget_secs: 30 }.is_actionable());
        assert!(!Provenance::Absent.is_actionable());
        assert!(!Provenance::Simulated.is_actionable());
        // …but stale is still *visible*, which is what separates it from absent.
        assert!(Provenance::Stale { age_secs: 900, budget_secs: 30 }.is_visible());
        assert!(!Provenance::Absent.is_visible());
    }
}
