//! The utility equation, and the arithmetic that keeps it honest.
//!
//! ```text
//!   U = R − λ₁K − λ₂C − λ₃U + λ₄V
//! ```
//!
//! Gain, minus hazard, minus cost, minus ignorance, plus the freedom to change your mind.
//! Every one of those five is a [`Term`] from a [`Projection`], so every one of them
//! carries whether anybody measured it — and because all four penalties and the bonus are
//! **additive** with a neutral element of `0.0`, an unmeasured term drops out of the sum
//! rather than dragging it to zero.
//!
//! That last point is the whole reason the equation is additive rather than multiplicative.
//! A multiplicative form is more expressive and it is a trap: an unmeasured factor is
//! either `1.0` (and the equation lies about certainty) or `0.0` (and the score is pinned
//! shut by dimensions nobody has built). This repository has paid for the `0.0` version
//! twice. Additive terms with a `0.0` neutral have the property that matters here —
//! **ignorance is silent, not fatal, and it is visible in the coverage** rather than
//! smuggled into the number.
//!
//! ## The λ weights are a policy, not a discovery
//!
//! They encode how much this operator dislikes risk relative to cost. They are not fitted
//! to anything and must never be presented as if they were. [`UtilityWeights`] is hashed
//! into the decision record precisely so that a ranking can be re-read later against the
//! preferences that produced it.

use scema_sim::Projection;
use scema_world::{Coverage, Term};
use serde::{Deserialize, Serialize};

/// How much each penalty counts against gain.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UtilityWeights {
    /// λ₁ — hazard of acting.
    pub risk: f64,
    /// λ₂ — cost.
    pub cost: f64,
    /// λ₃ — ignorance about the world.
    pub uncertainty: f64,
    /// λ₄ — bonus for being able to undo it.
    pub reversibility: f64,
}

impl Default for UtilityWeights {
    /// Risk-averse defaults, stated rather than derived.
    ///
    /// `risk` outweighs `cost` roughly two to one, and `uncertainty` is weighted as
    /// heavily as cost because in this design "we did not look properly" is a real
    /// objection to acting and not a rounding error. `reversibility` is a genuine bonus:
    /// a plan you can walk back from is worth taking on thinner evidence, which is the
    /// only mechanism here that lets an agent act while still unsure.
    fn default() -> Self {
        UtilityWeights { risk: 0.6, cost: 0.3, uncertainty: 0.3, reversibility: 0.25 }
    }
}

impl UtilityWeights {
    /// Weights that will not act on anything it cannot undo unless the gain is measured.
    pub fn cautious() -> Self {
        UtilityWeights { risk: 1.0, cost: 0.3, uncertainty: 0.6, reversibility: 0.5 }
    }
}

/// The result of applying [`UtilityWeights`] to a [`Projection`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Utility {
    pub value: f64,
    /// Per-term contribution to `value`, in equation order, for rendering an explanation
    /// that adds up. A score a reader cannot decompose is a score they have to trust.
    pub contributions: Vec<Contribution>,
    /// Measured fraction of the five inputs.
    pub coverage: Coverage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contribution {
    pub symbol: String,
    /// Signed effect on the total, weight already applied.
    pub effect: f64,
    pub measured: bool,
    pub note: String,
}

fn contribute(t: &Term, weight: f64, sign: f64) -> Contribution {
    Contribution {
        symbol: t.symbol.clone(),
        effect: sign * weight * t.value,
        measured: t.measured,
        note: t.note.clone(),
    }
}

impl UtilityWeights {
    /// Apply the equation to one projection.
    pub fn apply(&self, p: &Projection) -> Utility {
        let contributions = vec![
            contribute(&p.expected_gain, 1.0, 1.0),
            contribute(&p.risk, self.risk, -1.0),
            contribute(&p.cost, self.cost, -1.0),
            contribute(&p.uncertainty, self.uncertainty, -1.0),
            contribute(&p.reversibility, self.reversibility, 1.0),
        ];
        let value = contributions.iter().map(|c| c.effect).sum();
        Utility { value, contributions, coverage: p.coverage }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_sim::{ShadowDelta, StructuralSimulator, Simulator};
    use scema_world::{
        Domain, Entity, EntityKind, Extent, Goal, Hypothesis, HypothesisOrigin, WorldState,
    };

    fn empty_world() -> WorldState {
        WorldState {
            observer: "t".into(),
            entity: Entity { kind: EntityKind::Unknown, locator: "".into(), label: "".into() },
            domain: Domain::Unknown,
            observed_at: 0,
            objects: vec![],
            facts: vec![],
            signals: vec![],
            extent: Extent::complete(0, "t"),
            blind_spots: vec![],
        }
    }

    fn projection_with(gain: Term, risk: Term) -> Projection {
        Projection {
            hypothesis: "h".into(),
            simulator: "t".into(),
            expected_gain: gain,
            risk,
            cost: Term::absent("C", "cost", 0.0, "n/a"),
            uncertainty: Term::absent("U", "uncertainty", 0.0, "n/a"),
            reversibility: Term::absent("V", "reversibility", 0.0, "n/a"),
            failure_modes: vec![],
            shadow: ShadowDelta {
                touched_objects: vec![],
                addresses_signals: vec![],
                unaddressed_risks: vec![],
            },
            forbidden_by: None,
            coverage: Coverage { measured: 0, total: 5 },
        }
    }

    #[test]
    fn unmeasured_terms_drop_out_of_the_sum_instead_of_pinning_it() {
        // The lesson this workspace exists to encode. A projection where only the gain was
        // measured must score the gain, not zero.
        let p = projection_with(
            Term::measured("R", "gain", 0.8, "counted"),
            Term::absent("K", "risk", 0.0, "no actions"),
        );
        let u = UtilityWeights::default().apply(&p);
        assert!((u.value - 0.8).abs() < 1e-9);
    }

    #[test]
    fn measured_risk_does_reduce_the_score() {
        let p = projection_with(
            Term::measured("R", "gain", 0.8, "counted"),
            Term::measured("K", "risk", 0.5, "declared execute step"),
        );
        let u = UtilityWeights::default().apply(&p);
        assert!((u.value - (0.8 - 0.6 * 0.5)).abs() < 1e-9);
    }

    #[test]
    fn contributions_sum_to_the_value_so_an_explanation_adds_up() {
        let p = projection_with(
            Term::measured("R", "gain", 0.4, "x"),
            Term::measured("K", "risk", 0.9, "y"),
        );
        let u = UtilityWeights::cautious().apply(&p);
        let summed: f64 = u.contributions.iter().map(|c| c.effect).sum();
        assert!((summed - u.value).abs() < 1e-12);
    }

    #[test]
    fn an_ungrounded_branch_on_an_empty_world_cannot_score_positive() {
        // End-to-end with the real simulator: nothing observed, nothing cited, no actions.
        // The only measurable term is uncertainty, and it can only subtract.
        let h = Hypothesis::new("h", "do something bold", HypothesisOrigin::Human);
        let p = StructuralSimulator.project(&empty_world(), &Goal::new("g", "x"), &h);
        let u = UtilityWeights::default().apply(&p);
        assert!(u.value <= 0.0, "got {}", u.value);
    }
}
