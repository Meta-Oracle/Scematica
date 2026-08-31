//! The first evaluator that is applicable to every world.
//!
//! Every specialist in this runtime has been domain-bound. `dqstar` declines on anything
//! non-trading, correctly, because `TradeState` is pool-and-position shaped — which leaves a
//! repository, a DOM and a set of oracle feeds reaching the policy layer with no second
//! opinion available at all. [`WorldFeatures`] is the same twelve numbers for all four
//! producers, so this evaluator is the same evaluator for all four.
//!
//! ## What it actually knows, which is one thing
//!
//! **Whether the world was legible enough for a branch to mean anything.** That is not a
//! judgement about the branch — it is a judgement about the evidence underneath it, and it is
//! deliberately the only thing this evaluator claims. A second opinion that pretended to
//! domain knowledge it does not have is the exact failure `Applicability` exists to prevent,
//! and being domain-agnostic is not a licence to have opinions about everything.
//!
//! So it scores every branch in a given world **identically**. That looks useless until you
//! see what it is for: a branch ranked top on a world where four of twelve features were
//! measured should not read the same as one ranked top on a world where all twelve were, and
//! nothing in the utility equation says so, because `Coverage` qualifies the *score* rather
//! than the *world*. This is the term that carries it.
//!
//! ## Why it never vetoes
//!
//! A measured negative from a qualified specialist vetoes outright in [`crate::decide`]. This
//! one must never trigger that, and its utility is therefore clamped at or above zero. An
//! illegible world is a reason to abstain — which `Abstention::TooLittleMeasured` already
//! handles, with a better message — and not a reason to reject a particular branch. Letting
//! it veto would silently convert "we could not see much" into "this specific plan is bad".

use scema_sim::Projection;
use scema_world::{Goal, Hypothesis, Term, WorldFeatures, WorldState};

use crate::evaluator::{Applicability, Evaluation, Evaluator};

/// Coverage below which a world is too dark to support a second opinion at all.
///
/// A third of the feature vector. Below it this reports `Insufficient` rather than scoring
/// near zero, because those are different claims: a low score says "the evidence is thin",
/// and `Insufficient` says "there is not enough here for me to have a view", which is the
/// one an operator can act on by observing more.
pub const MIN_COVERAGE: f64 = 1.0 / 3.0;

/// Scores how legible the world underneath a branch was.
#[derive(Debug, Default, Clone, Copy)]
pub struct LegibilityEvaluator;

impl Evaluator for LegibilityEvaluator {
    fn name(&self) -> &str {
        "legibility"
    }

    fn about(&self) -> &str {
        "How much of the world was actually observed, for any domain. Never vetoes: an \
         illegible world is a reason to abstain, not a reason to reject one branch."
    }

    fn applicability(&self, world: &WorldState, _goal: &Goal) -> Applicability {
        let f = WorldFeatures::of(world);
        let c = f.coverage();
        let fraction = c.fraction();

        if world.objects.is_empty() && world.signals.is_empty() {
            return Applicability::Insufficient {
                note: "nothing was perceived — no objects and no signals, so there is no \
                       legibility to report. Observe something first."
                    .into(),
            };
        }
        if fraction < MIN_COVERAGE {
            return Applicability::Insufficient {
                note: format!(
                    "only {} of the {} feature(s) were measured, which is below the {:.0}% \
                     this evaluator needs to have a view at all",
                    c.measured,
                    c.total,
                    MIN_COVERAGE * 100.0
                ),
            };
        }
        // Deliberately no domain test. That is the point of this evaluator: it is the one
        // second opinion a repository, a DOM, a market and an oracle set can all receive.
        Applicability::Applicable {
            note: format!("{} of {} feature(s) measured", c.measured, c.total),
        }
    }

    fn score(
        &self,
        world: &WorldState,
        _goal: &Goal,
        _hypothesis: &Hypothesis,
        _projection: &Projection,
    ) -> Option<Evaluation> {
        let f = WorldFeatures::of(world);
        let c = f.coverage();

        // Two things, multiplied: how much of the feature vector was measured, and how much
        // of what was perceived is actionable. A world can be fully featured and entirely
        // stale — `alchem-world.json` is close — and a world can be entirely live with
        // almost nothing perceived. Neither alone is legibility.
        let coverage = c.fraction();
        let legible = match (&f.legibility.measured, f.legibility.value) {
            (true, v) => v,
            // Unmeasured legibility means no objects were perceived. The neutral element for
            // a *product* is 1.0, not 0.0 — the historical bug this workspace has paid for
            // twice is writing zero here and pinning the aggregate shut.
            (false, _) => 1.0,
        };

        let utility = (coverage * legible).clamp(0.0, 1.0);

        Some(Evaluation {
            evaluator: self.name().to_string(),
            utility: Term::measured(
                "Lg",
                "legibility",
                utility,
                format!(
                    "{} of {} feature(s) measured{}",
                    c.measured,
                    c.total,
                    if f.legibility.measured {
                        format!(", {:.0}% of objects actionable", f.legibility.value * 100.0)
                    } else {
                        ", no objects perceived".to_string()
                    }
                ),
            ),
            // Honestly unmeasured. This evaluator has no way to quantify how sure it is
            // about its own reading, and a constant here would be a number with nothing
            // behind it — the thing `Term` exists to make impossible to write by accident.
            confidence: Term::absent(
                "Cf",
                "confidence",
                0.0,
                "this evaluator has no second source to check itself against",
            ),
            note: if f.blind_spots.value > 0.0 {
                format!(
                    "the observer reported {} thing(s) it could not read; this score is about \
                     the evidence, not the branch",
                    f.blind_spots.value as u64
                )
            } else {
                "this score is about the evidence underneath every branch, not about any one \
                 of them"
                    .to_string()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_world::*;

    fn goal() -> Goal {
        Goal::new("g", "do something")
    }

    fn hypo(id: &str) -> Hypothesis {
        Hypothesis::new(id, "do it", HypothesisOrigin::Human)
    }

    /// A real projection from the real simulator — this evaluator ignores it, but building
    /// one by hand would pin a struct layout the test does not care about.
    fn proj(w: &WorldState, h: &Hypothesis) -> Projection {
        use scema_sim::{Simulator, StructuralSimulator};
        StructuralSimulator::new().project(w, &goal(), h)
    }

    fn world(domain: Domain) -> WorldState {
        WorldState {
            schema: Some(WORLD_SCHEMA.into()),
            observer: "t".into(),
            entity: Entity { kind: EntityKind::Repository, locator: "x".into(), label: "x".into() },
            domain,
            observed_at: 0,
            objects: vec![],
            facts: vec![],
            signals: vec![],
            extent: Extent { observed: 1, total: Some(2), note: String::new() },
            blind_spots: vec![],
        }
    }

    fn obj(p: Provenance) -> Object {
        Object {
            id: "o".into(),
            kind: "thing".into(),
            label: "o".into(),
            attrs: Default::default(),
            provenance: p,
        }
    }

    fn populated(domain: Domain) -> WorldState {
        let mut w = world(domain);
        w.objects = vec![
            obj(Provenance::Live { age_secs: 0 }),
            obj(Provenance::Live { age_secs: 0 }),
        ];
        w.signals = vec![Signal {
            id: "s".into(),
            polarity: Polarity::Risk,
            label: "s".into(),
            detail: String::new(),
            magnitude: 0.2,
            measured: true,
            targets: vec![],
            evidence: vec!["counted".into()],
        }];
        w
    }

    #[test]
    fn it_applies_to_every_domain_which_is_the_entire_point() {
        // `dqstar` declines on all but one of these. This is the second opinion a
        // repository, a DOM and an oracle set can actually receive.
        for d in [Domain::Software, Domain::Web, Domain::Data, Domain::Trading, Domain::Infrastructure] {
            let w = populated(d.clone());
            let a = LegibilityEvaluator.applicability(&w, &goal());
            assert!(a.is_applicable(), "declined on {d:?}: {}", a.note());
        }
    }

    #[test]
    fn an_unperceived_world_is_insufficient_not_a_low_score() {
        // "The evidence is thin" and "there is not enough here for me to have a view" are
        // different claims, and only the second tells the operator to go and observe.
        let a = LegibilityEvaluator.applicability(&world(Domain::Software), &goal());
        match a {
            Applicability::Insufficient { note } => assert!(note.contains("nothing was perceived")),
            other => panic!("expected Insufficient, got {other:?}"),
        }
    }

    #[test]
    fn it_never_returns_a_negative_utility_so_it_can_never_veto() {
        // A measured negative from a qualified specialist vetoes outright in `decide`. An
        // illegible world is a reason to abstain, not a reason to reject one branch.
        let w = populated(Domain::Software);
        let h = hypo("h");
        let p = proj(&w, &h);
        let e = LegibilityEvaluator.score(&w, &goal(), &h, &p).expect("applicable");
        assert!(e.utility.value >= 0.0, "utility {} would veto", e.utility.value);
    }

    #[test]
    fn its_confidence_is_unmeasured_rather_than_a_constant() {
        // A number with nothing behind it is precisely what `Term` exists to make hard to
        // write by accident, and a specialist inventing its own confidence is the shape of
        // every untrustworthy ensemble.
        let w = populated(Domain::Software);
        let h = hypo("h");
        let p = proj(&w, &h);
        let e = LegibilityEvaluator.score(&w, &goal(), &h, &p).unwrap();
        assert!(!e.confidence.measured);
    }

    #[test]
    fn a_stale_world_scores_below_a_live_one_with_the_same_coverage() {
        let mut live = populated(Domain::Software);
        let mut stale = populated(Domain::Software);
        stale.objects = vec![
            obj(Provenance::Stale { age_secs: 99, budget_secs: 1 }),
            obj(Provenance::Stale { age_secs: 99, budget_secs: 1 }),
        ];
        let h = hypo("h");
        live.observed_at = 0;
        stale.observed_at = 0;
        let pa = proj(&live, &h);
        let pb = proj(&stale, &h);
        let a = LegibilityEvaluator.score(&live, &goal(), &h, &pa).unwrap();
        let b = LegibilityEvaluator.score(&stale, &goal(), &h, &pb).unwrap();
        assert!(a.utility.value > b.utility.value, "{} vs {}", a.utility.value, b.utility.value);
    }

    #[test]
    fn every_branch_in_one_world_scores_the_same_and_that_is_deliberate() {
        // This evaluator judges the evidence, not the plan. Varying by branch would be it
        // pretending to domain knowledge it does not have.
        let w = populated(Domain::Software);
        let h1 = hypo("a");
        let h2 = hypo("b");
        let p1 = proj(&w, &h1);
        let p2 = proj(&w, &h2);
        let a = LegibilityEvaluator.score(&w, &goal(), &h1, &p1).unwrap();
        let b = LegibilityEvaluator.score(&w, &goal(), &h2, &p2).unwrap();
        assert_eq!(a.utility.value, b.utility.value);
    }
}
