//! [`decide`]: turning projections into a choice, or into an explicit refusal to choose.
//!
//! ## Abstention is a first-class outcome
//!
//! Most of this module is about *not* picking. A ranking always exists — sort five numbers
//! and something comes first — so an agent that always acts on the top row is an agent with
//! no way to express "none of these are worth doing", which is the correct answer far more
//! often than it is comfortable. [`Abstention`] enumerates the reasons, and each one is a
//! different instruction to the operator:
//!
//! | Reason | What the operator should do |
//! |---|---|
//! | [`Abstention::NoCandidates`] | nothing was proposed — check the hypothesisers |
//! | [`Abstention::AllForbidden`] | every branch violates a constraint — the goal is unsatisfiable as stated |
//! | [`Abstention::NoPositiveUtility`] | acting is worse than not acting — accept, or lower the bar deliberately |
//! | [`Abstention::TooLittleMeasured`] | the ranking stands on almost nothing — go and observe more |
//! | [`Abstention::Contested`] | a specialist that *is* qualified disagrees with the top branch |
//!
//! ## Specialist opinions are attached, never averaged in
//!
//! An [`Evaluation`] from `scema-policy`'s general equation and one from a trained Q-network
//! are not the same quantity, and averaging them produces a number in no unit at all. So
//! the ranking is by the general utility alone, specialist evaluations ride alongside, and
//! their only mechanical power is to **contest** — a qualified specialist with a measured
//! negative opinion of the top branch blocks it rather than shading it downward. A veto is
//! legible; a weighted blend of incommensurable scores is not.

use scema_sim::Projection;
use scema_world::{Coverage, Goal, Hypothesis, WorldState};
use serde::{Deserialize, Serialize};

use crate::evaluator::{Applicability, Evaluation, Evaluator};
use crate::utility::{Utility, UtilityWeights};

/// Thresholds that turn a ranking into a decision.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecisionConfig {
    pub weights: UtilityWeights,
    /// Minimum measured fraction of the winning projection's five terms.
    ///
    /// `0.4` — two of five. Below that the ranking is mostly arithmetic on neutral
    /// elements, and the honest report is "go and look at the world", not a choice.
    pub min_coverage: f64,
    /// A qualified specialist's measured utility at or below this contests the top branch.
    ///
    /// `0.0`, i.e. any measured negative opinion. Deliberately not a margin: this is a
    /// gate on disagreement, not a tie-break, and the specialist has already declined if it
    /// had nothing to say.
    pub veto_at_or_below: f64,
}

impl Default for DecisionConfig {
    fn default() -> Self {
        DecisionConfig {
            weights: UtilityWeights::default(),
            min_coverage: 0.4,
            veto_at_or_below: 0.0,
        }
    }
}

/// One branch that was allowed to compete.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ranked {
    pub hypothesis: String,
    pub statement: String,
    pub utility: Utility,
    /// Second opinions from every qualified specialist, in evaluator order.
    pub evaluations: Vec<Evaluation>,
}

/// A branch that was removed before ranking.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Excluded {
    pub hypothesis: String,
    pub statement: String,
    /// The constraint, quoted. Excluded branches stay in the record: an agent that drops
    /// what it was not permitted to consider cannot explain the shape of its own choice.
    pub reason: String,
}

/// Why no branch was chosen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Abstention {
    NoCandidates,
    AllForbidden { count: usize },
    NoPositiveUtility { best: f64 },
    TooLittleMeasured { coverage: Coverage, floor: f64 },
    Contested { by: String, utility: f64, note: String },
}

impl Abstention {
    pub fn headline(&self) -> String {
        match self {
            Abstention::NoCandidates => "no hypotheses were proposed".into(),
            Abstention::AllForbidden { count } => {
                format!("all {count} branch(es) violate a constraint on the goal")
            }
            Abstention::NoPositiveUtility { best } => {
                format!("the best branch scores {best:.3}; acting is worse than not acting")
            }
            Abstention::TooLittleMeasured { coverage, floor } => format!(
                "the ranking stands on {} measured term(s) ({:.0}% < {:.0}% floor)",
                coverage.label(),
                coverage.fraction() * 100.0,
                floor * 100.0
            ),
            Abstention::Contested { by, utility, .. } => {
                format!("`{by}` is qualified here and scores the top branch {utility:.3}")
            }
        }
    }
}

/// The output of the policy layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    /// The chosen hypothesis id, or `None` — in which case `abstention` says why.
    pub chosen: Option<String>,
    /// Every branch that competed, best first.
    pub ranked: Vec<Ranked>,
    pub excluded: Vec<Excluded>,
    pub abstention: Option<Abstention>,
    pub config: DecisionConfig,
    /// Per-evaluator applicability for this world, including the ones that declined.
    /// Recorded so a reader can tell a specialist that approved from one that never spoke.
    pub evaluator_status: Vec<EvaluatorStatus>,
    /// Measured fraction across every term of every ranked projection.
    pub coverage: Coverage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvaluatorStatus {
    pub evaluator: String,
    pub about: String,
    pub applicability: Applicability,
}

/// Rank the branches and decide, or refuse to.
///
/// `hypotheses` and `projections` are matched by id, not by position: a simulator is free
/// to reorder or drop, and a positional match would silently score one branch with
/// another's projection.
pub fn decide(
    world: &WorldState,
    goal: &Goal,
    hypotheses: &[Hypothesis],
    projections: &[Projection],
    evaluators: &[&dyn Evaluator],
    config: DecisionConfig,
) -> Decision {
    let evaluator_status: Vec<EvaluatorStatus> = evaluators
        .iter()
        .map(|e| EvaluatorStatus {
            evaluator: e.name().to_string(),
            about: e.about().to_string(),
            applicability: e.applicability(world, goal),
        })
        .collect();

    let qualified: Vec<&&dyn Evaluator> = evaluators
        .iter()
        .zip(&evaluator_status)
        .filter(|(_, s)| s.applicability.is_applicable())
        .map(|(e, _)| e)
        .collect();

    let mut ranked: Vec<Ranked> = Vec::new();
    let mut excluded: Vec<Excluded> = Vec::new();

    for h in hypotheses {
        let Some(p) = projections.iter().find(|p| p.hypothesis == h.id) else {
            continue;
        };
        if let Some(reason) = &p.forbidden_by {
            excluded.push(Excluded {
                hypothesis: h.id.clone(),
                statement: h.statement.clone(),
                reason: reason.clone(),
            });
            continue;
        }
        let evaluations = qualified
            .iter()
            .filter_map(|e| e.score(world, goal, h, p))
            .collect();
        ranked.push(Ranked {
            hypothesis: h.id.clone(),
            statement: h.statement.clone(),
            utility: config.weights.apply(p),
            evaluations,
        });
    }

    // Descending by utility. Ties break on the id so the order is deterministic — this
    // structure gets hashed, and a stable sort over an unstable comparator would give the
    // same decision two digests.
    ranked.sort_by(|a, b| {
        b.utility
            .value
            .partial_cmp(&a.utility.value)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.hypothesis.cmp(&b.hypothesis))
    });

    let measured: usize = ranked.iter().map(|r| r.utility.coverage.measured).sum();
    let total: usize = ranked.iter().map(|r| r.utility.coverage.total).sum();
    let coverage = Coverage { measured, total };

    let abstention = evaluate_abstention(&ranked, &excluded, hypotheses.len(), &config);
    let chosen = if abstention.is_none() {
        ranked.first().map(|r| r.hypothesis.clone())
    } else {
        None
    };

    Decision { chosen, ranked, excluded, abstention, config, evaluator_status, coverage }
}

fn evaluate_abstention(
    ranked: &[Ranked],
    excluded: &[Excluded],
    proposed: usize,
    config: &DecisionConfig,
) -> Option<Abstention> {
    if proposed == 0 {
        return Some(Abstention::NoCandidates);
    }
    let Some(best) = ranked.first() else {
        return Some(if excluded.is_empty() {
            Abstention::NoCandidates
        } else {
            Abstention::AllForbidden { count: excluded.len() }
        });
    };
    if best.utility.value <= 0.0 {
        return Some(Abstention::NoPositiveUtility { best: best.utility.value });
    }
    if best.utility.coverage.fraction() < config.min_coverage {
        return Some(Abstention::TooLittleMeasured {
            coverage: best.utility.coverage,
            floor: config.min_coverage,
        });
    }
    // A specialist only reaches this point if it declared itself qualified, so a measured
    // negative here is a real disagreement between two things that both understand the
    // problem. An *unmeasured* specialist utility is silence and carries no veto.
    for e in &best.evaluations {
        if e.utility.measured && e.utility.value <= config.veto_at_or_below {
            return Some(Abstention::Contested {
                by: e.evaluator.clone(),
                utility: e.utility.value,
                note: e.utility.note.clone(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_sim::{ShadowDelta, Simulator, StructuralSimulator};
    use scema_world::{
        Action, Constraint, Domain, Entity, EntityKind, Extent, HypothesisOrigin, Object,
        Polarity, Provenance, Reversibility, RiskClass, Signal, Term,
    };

    fn world(signals: Vec<Signal>) -> WorldState {
        WorldState {
            schema: Some(scema_world::WORLD_SCHEMA.into()),
            observer: "t".into(),
            entity: Entity { kind: EntityKind::Repository, locator: ".".into(), label: "t".into() },
            domain: Domain::Software,
            observed_at: 0,
            objects: vec![Object::new("o", "file", "o", Provenance::Live { age_secs: 0 })],
            facts: vec![],
            signals,
            extent: Extent::complete(1, "walked"),
            blind_spots: vec![],
        }
    }

    fn opportunity(id: &str, magnitude: f64) -> Signal {
        Signal {
            id: id.into(),
            polarity: Polarity::Opportunity,
            label: id.into(),
            detail: String::new(),
            magnitude,
            measured: true,
            targets: vec![],
            evidence: vec!["counted".into()],
        }
    }

    fn grounded_hyp(id: &str, signal: &str) -> Hypothesis {
        Hypothesis::new(id, format!("address {signal}"), HypothesisOrigin::Heuristic { rule: "t".into() })
            .grounded(signal)
            .doing(Action::new("a", RiskClass::Write, "src/x.rs", "edit", Reversibility::Trivial))
    }

    fn run(w: &WorldState, g: &Goal, hs: &[Hypothesis], evs: &[&dyn Evaluator]) -> Decision {
        let ps = StructuralSimulator.project_all(w, g, hs);
        decide(w, g, hs, &ps, evs, DecisionConfig::default())
    }

    #[test]
    fn nothing_proposed_abstains_with_no_candidates() {
        let d = run(&world(vec![]), &Goal::new("g", "x"), &[], &[]);
        assert_eq!(d.abstention, Some(Abstention::NoCandidates));
        assert!(d.chosen.is_none());
    }

    #[test]
    fn an_ungrounded_branch_never_wins_by_default() {
        // It is the only candidate and it still must not be chosen: with no measured gain,
        // its utility cannot exceed zero.
        let hs = vec![Hypothesis::new("h1", "rewrite everything", HypothesisOrigin::Human)];
        let d = run(&world(vec![]), &Goal::new("g", "x"), &hs, &[]);
        assert!(d.chosen.is_none());
        assert!(matches!(d.abstention, Some(Abstention::NoPositiveUtility { .. })));
    }

    #[test]
    fn a_grounded_branch_is_chosen_and_the_reason_is_reconstructible() {
        let w = world(vec![opportunity("s1", 0.9)]);
        let hs = vec![grounded_hyp("h1", "s1")];
        let d = run(&w, &Goal::new("g", "x"), &hs, &[]);
        assert_eq!(d.chosen.as_deref(), Some("h1"));
        let top = &d.ranked[0];
        let summed: f64 = top.utility.contributions.iter().map(|c| c.effect).sum();
        assert!((summed - top.utility.value).abs() < 1e-12);
    }

    #[test]
    fn every_branch_forbidden_abstains_and_the_branches_stay_in_the_record() {
        let w = world(vec![opportunity("s1", 0.9)]);
        let g = Goal::new("g", "x").with_constraint(Constraint::must_not("src/", "hands off"));
        let hs = vec![grounded_hyp("h1", "s1")];
        let d = run(&w, &g, &hs, &[]);
        assert_eq!(d.abstention, Some(Abstention::AllForbidden { count: 1 }));
        assert_eq!(d.excluded.len(), 1, "an excluded branch must remain visible");
        assert!(d.excluded[0].reason.contains("src/"));
    }

    #[test]
    fn ranking_matches_hypotheses_to_projections_by_id_not_position() {
        let w = world(vec![opportunity("s1", 0.9), opportunity("s2", 0.2)]);
        let g = Goal::new("g", "x");
        let hs = vec![grounded_hyp("h1", "s1"), grounded_hyp("h2", "s2")];
        let mut ps = StructuralSimulator.project_all(&w, &g, &hs);
        ps.reverse(); // a simulator is free to reorder
        let d = decide(&w, &g, &hs, &ps, &[], DecisionConfig::default());
        assert_eq!(d.chosen.as_deref(), Some("h1"), "h1 has the larger measured gain");
    }

    struct AlwaysContests;
    impl Evaluator for AlwaysContests {
        fn name(&self) -> &str {
            "contrarian"
        }
        fn about(&self) -> &str {
            "test double"
        }
        fn applicability(&self, _w: &WorldState, _g: &Goal) -> Applicability {
            Applicability::Applicable { note: "test".into() }
        }
        fn score(
            &self,
            _w: &WorldState,
            _g: &Goal,
            _h: &Hypothesis,
            _p: &Projection,
        ) -> Option<Evaluation> {
            Some(Evaluation {
                evaluator: "contrarian".into(),
                utility: Term::measured("Q", "q", -0.4, "disagrees"),
                confidence: Term::measured("d", "d", 0.9, "sure"),
                note: String::new(),
            })
        }
    }

    struct Silent;
    impl Evaluator for Silent {
        fn name(&self) -> &str {
            "silent"
        }
        fn about(&self) -> &str {
            "test double"
        }
        fn applicability(&self, _w: &WorldState, _g: &Goal) -> Applicability {
            Applicability::Applicable { note: "test".into() }
        }
        fn score(
            &self,
            _w: &WorldState,
            _g: &Goal,
            _h: &Hypothesis,
            _p: &Projection,
        ) -> Option<Evaluation> {
            Some(Evaluation {
                evaluator: "silent".into(),
                utility: Term::absent("Q", "q", 0.0, "no opinion"),
                confidence: Term::absent("d", "d", 0.0, "n/a"),
                note: String::new(),
            })
        }
    }

    #[test]
    fn a_qualified_specialist_can_veto_the_top_branch() {
        let w = world(vec![opportunity("s1", 0.9)]);
        let hs = vec![grounded_hyp("h1", "s1")];
        let d = run(&w, &Goal::new("g", "x"), &hs, &[&AlwaysContests]);
        assert!(d.chosen.is_none());
        assert!(matches!(d.abstention, Some(Abstention::Contested { .. })));
    }

    #[test]
    fn an_unmeasured_specialist_opinion_is_silence_and_cannot_veto() {
        // The distinction that keeps "I have no view" from acting like "I object".
        let w = world(vec![opportunity("s1", 0.9)]);
        let hs = vec![grounded_hyp("h1", "s1")];
        let d = run(&w, &Goal::new("g", "x"), &hs, &[&Silent]);
        assert_eq!(d.chosen.as_deref(), Some("h1"));
    }

    #[test]
    fn declining_evaluators_are_still_recorded() {
        let w = world(vec![opportunity("s1", 0.9)]);
        let hs = vec![grounded_hyp("h1", "s1")];
        let dq = crate::dqstar::DqStarEvaluator::unloaded();
        let d = run(&w, &Goal::new("g", "x"), &hs, &[&dq]);
        assert_eq!(d.evaluator_status.len(), 1);
        assert!(matches!(
            d.evaluator_status[0].applicability,
            Applicability::OutOfDomain { .. }
        ));
        assert!(d.ranked[0].evaluations.is_empty(), "a declining evaluator must not score");
    }

    #[test]
    fn a_thinly_measured_ranking_abstains_rather_than_choosing() {
        let w = world(vec![opportunity("s1", 0.9)]);
        let g = Goal::new("g", "x");
        // One measured term out of five, and a positive utility: without the coverage
        // floor this would be chosen on almost nothing.
        let p = Projection {
            hypothesis: "h1".into(),
            simulator: "t".into(),
            expected_gain: Term::measured("R", "gain", 0.9, "counted"),
            risk: Term::absent("K", "risk", 0.0, "n/a"),
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
            coverage: Coverage { measured: 1, total: 5 },
        };
        let hs = vec![grounded_hyp("h1", "s1")];
        let d = decide(&w, &g, &hs, &[p], &[], DecisionConfig::default());
        assert!(matches!(d.abstention, Some(Abstention::TooLittleMeasured { .. })));
    }
}
