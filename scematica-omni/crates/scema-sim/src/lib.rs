//! # scema-sim — the counterfactual layer
//!
//! Between "the agent has ideas" and "the agent acts" there is supposed to be a step where
//! the ideas are made to compete. This crate is that step. It takes a [`WorldState`], a
//! [`Goal`] and a set of [`Hypothesis`] branches and returns a [`Projection`] per branch:
//! expected gain, risk, cost, uncertainty and reversibility, each as a [`Term`] that says
//! whether anybody measured it.
//!
//! ## The rule that shapes everything here
//!
//! > **A projection may not invent a number.**
//!
//! A simulator that outputs `+31% predicted performance` for a refactor nobody has
//! benchmarked has produced a hallucination with a decimal point on it, and the decimal
//! point is what makes it dangerous — it survives into a ranking, a report and a decision
//! record looking exactly like a measurement. So [`StructuralSimulator`] scores an
//! expected gain **only** for a hypothesis grounded in a measured signal that the observer
//! actually counted. Everything else is [`Term::absent`] with `0.0` and a note saying what
//! would have to exist.
//!
//! The consequence is uncomfortable and correct: on a world that was barely perceived,
//! most branches project a gain of exactly zero and the agent abstains. That is the true
//! answer. The alternative — a plausible ranking over invented gains — is the failure this
//! whole workspace is built to avoid.
//!
//! ## What a structural simulator can and cannot know
//!
//! It knows what the *plan* declares (how many steps, of what risk class, how reversible)
//! and what the *world* recorded (which signals exist, how legible it was, what could not
//! be read). Those are real observations about the decision, and they are enough to rank
//! branches by hazard and by ignorance. They are not enough to predict an outcome, and
//! this crate never claims to. Predicting outcomes requires either a domain model or an
//! executed experiment; both are [`Simulator`] implementations somebody can add, which is
//! why this is a trait and not a function.

use scema_world::{
    Coverage, Goal, Hypothesis, Polarity, Reversibility, Signal, Term, WorldState,
};
use serde::{Deserialize, Serialize};

/// Anything that can project a hypothesis forward.
///
/// Implementations range from the structural one below (no domain knowledge, no execution)
/// through domain models to simulators that actually run the experiment in a sandbox. All
/// of them must obey the module rule: an unmeasured dimension is [`Term::absent`].
pub trait Simulator {
    /// Stable name, recorded in the projection and hashed into the decision record.
    fn name(&self) -> &str;

    fn project(&self, world: &WorldState, goal: &Goal, hypothesis: &Hypothesis) -> Projection;

    fn project_all(&self, world: &WorldState, goal: &Goal, hs: &[Hypothesis]) -> Vec<Projection> {
        hs.iter().map(|h| self.project(world, goal, h)).collect()
    }
}

/// A way this branch could go wrong.
///
/// `likelihood` is a [`Term`] like everything else, and it is usually unmeasured. A named
/// failure mode with an honest "nobody has estimated this" is worth far more than a
/// number: it is the thing a human reads before approving.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FailureMode {
    pub label: String,
    pub detail: String,
    pub likelihood: Term,
}

/// What the world would look like afterwards, to the extent that is knowable.
///
/// Not a predicted `WorldState` — producing one would mean fabricating attribute values
/// for every object the plan touches. It is the *delta the plan claims*: what it would
/// touch, which observed signals it would address, and which it would leave standing.
/// `unaddressed_risks` is the most useful field on the struct and the one a plan author
/// never volunteers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShadowDelta {
    pub touched_objects: Vec<String>,
    pub addresses_signals: Vec<String>,
    pub unaddressed_risks: Vec<String>,
}

/// The projected consequences of one hypothesis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    pub hypothesis: String,
    pub simulator: String,
    /// `R` — expected gain, additive, neutral `0.0`. Measured only from counted signals.
    pub expected_gain: Term,
    /// `K` — hazard of acting, additive penalty, neutral `0.0`.
    pub risk: Term,
    /// `C` — cost proxy, additive penalty, neutral `0.0`.
    pub cost: Term,
    /// `U` — ignorance about the world this plan runs in, additive penalty, neutral `0.0`.
    pub uncertainty: Term,
    /// `V` — reversibility, additive bonus, neutral `0.0`.
    pub reversibility: Term,
    pub failure_modes: Vec<FailureMode>,
    pub shadow: ShadowDelta,
    /// Constraint violated by this branch, if any. A projection carrying this must never
    /// be ranked — see `scema_policy::Decision`.
    pub forbidden_by: Option<String>,
    /// How many of the five terms were measured. Never separated from the numbers.
    pub coverage: Coverage,
}

impl Projection {
    /// The five terms in equation order.
    pub fn terms(&self) -> [&Term; 5] {
        [&self.expected_gain, &self.risk, &self.cost, &self.uncertainty, &self.reversibility]
    }
}

/// Simulation from the structure of the plan and the legibility of the world.
///
/// Takes no domain knowledge and executes nothing. See the crate note for what that buys
/// and what it costs.
#[derive(Clone, Debug, Default)]
pub struct StructuralSimulator;

impl StructuralSimulator {
    pub fn new() -> Self {
        StructuralSimulator
    }

    /// Signals the hypothesis cites that actually exist in the world.
    ///
    /// A citation of a signal id that is not present is dropped rather than trusted. That
    /// happens when a hypothesis outlives the world it was proposed against, and treating
    /// a dangling citation as support is how a stale plan keeps its score.
    fn cited<'a>(&self, world: &'a WorldState, h: &Hypothesis) -> Vec<&'a Signal> {
        h.grounded_in
            .iter()
            .filter_map(|id| world.signals.iter().find(|s| &s.id == id))
            .collect()
    }

    fn gain_term(&self, world: &WorldState, h: &Hypothesis) -> Term {
        let cited = self.cited(world, h);
        if cited.is_empty() {
            return Term::absent(
                "R",
                "expected gain",
                0.0,
                "hypothesis cites no signal in this world; no observed basis for a gain",
            );
        }
        let counted: Vec<&&Signal> = cited.iter().filter(|s| s.measured).collect();
        if counted.is_empty() {
            return Term::absent(
                "R",
                "expected gain",
                0.0,
                format!(
                    "cites {} signal(s), all of them estimates rather than counts",
                    cited.len()
                ),
            );
        }
        let mean = counted.iter().map(|s| s.magnitude).sum::<f64>() / counted.len() as f64;
        Term::measured(
            "R",
            "expected gain",
            mean,
            format!(
                "mean magnitude of {} counted signal(s): {}",
                counted.len(),
                counted.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", ")
            ),
        )
        .clamped(0.0, 1.0)
    }

    fn risk_term(&self, world: &WorldState, h: &Hypothesis) -> Term {
        let Some(worst) = h.worst_risk_class() else {
            return Term::absent(
                "K",
                "hazard of acting",
                0.0,
                "hypothesis declares no actions; nothing to be hazardous",
            );
        };
        let base = worst.base_hazard();
        // A plan that reaches into objects the observer flagged as risky is more hazardous
        // than the same plan elsewhere. Only counted risk signals escalate — an estimated
        // one would let a rule of thumb veto by arithmetic.
        let overlap: Vec<&Signal> = world
            .risks()
            .filter(|s| s.measured && self.touches(h, &s.targets))
            .collect();
        let escalation = overlap.iter().map(|s| s.magnitude).fold(0.0_f64, f64::max) * 0.5;
        let note = if overlap.is_empty() {
            format!("worst declared action class {worst:?}; no counted risk signal on its targets")
        } else {
            format!(
                "worst declared action class {worst:?}, escalated by counted risk(s): {}",
                overlap.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", ")
            )
        };
        Term::measured("K", "hazard of acting", base + escalation, note).clamped(0.0, 1.0)
    }

    fn cost_term(&self, h: &Hypothesis) -> Term {
        if h.actions.is_empty() {
            return Term::absent("C", "cost", 0.0, "no declared steps to cost");
        }
        // Deliberately a step count and nothing more. It is a real property of the plan;
        // it is not an effort estimate, and the note has to say so or a reader will take
        // 0.4 for "40% of a day".
        let steps = h.actions.len() as f64;
        Term::measured(
            "C",
            "cost",
            (steps / 10.0).min(1.0),
            format!("{steps} declared step(s), normalised at 10; no effort or spend estimate exists"),
        )
    }

    fn uncertainty_term(&self, world: &WorldState) -> Term {
        if world.objects.is_empty() && world.blind_spots.is_empty() {
            return Term::absent(
                "U",
                "uncertainty",
                0.0,
                "observer returned no objects and reported no blind spots; nothing to reason about",
            );
        }
        let illegible = 1.0 - world.legibility();
        // A blind spot is a thing the observer *tried* to read and could not, so it is
        // evidence of ignorance rather than a guess about it. Saturating at five keeps one
        // unreadable directory from pinning uncertainty at maximum.
        let blind = (world.blind_spots.len() as f64 / 5.0).min(1.0);
        // Unknown extent is its own penalty: a depth-limited walk can be perfectly legible
        // about a small fraction of a large thing.
        let unbounded = if world.extent.fraction().is_none() { 0.2 } else { 0.0 };
        let value = (0.5 * illegible + 0.3 * blind + unbounded).min(1.0);
        Term::measured(
            "U",
            "uncertainty",
            value,
            format!(
                "{:.0}% of observed objects unreadable or stale, {} blind spot(s), extent {}",
                illegible * 100.0,
                world.blind_spots.len(),
                if world.extent.fraction().is_none() { "unbounded" } else { "bounded" }
            ),
        )
    }

    fn reversibility_term(&self, h: &Hypothesis) -> Term {
        match h.worst_reversibility() {
            None => Term::absent(
                "V",
                "reversibility",
                0.0,
                "hypothesis declares no actions; nothing to reverse",
            ),
            Some(Reversibility::Unknown) => Term::absent(
                "V",
                "reversibility",
                0.0,
                "at least one step is unclassified; the plan cannot be called reversible",
            ),
            Some(r) => Term::measured(
                "V",
                "reversibility",
                r.score().unwrap_or(0.0),
                format!("least reversible declared step is {r:?}"),
            ),
        }
    }

    fn touches(&self, h: &Hypothesis, targets: &[String]) -> bool {
        if targets.is_empty() {
            // A signal about the entity as a whole is on every plan's path.
            return true;
        }
        h.actions.iter().any(|a| {
            targets
                .iter()
                .any(|t| a.target.contains(t.as_str()) || t.contains(a.target.as_str()))
        })
    }

    fn failure_modes(&self, world: &WorldState, h: &Hypothesis) -> Vec<FailureMode> {
        let mut out = Vec::new();

        if matches!(h.worst_reversibility(), Some(Reversibility::Irreversible)) {
            out.push(FailureMode {
                label: "irreversible step".into(),
                detail: "at least one declared step cannot be undone; a wrong branch is permanent"
                    .into(),
                likelihood: Term::absent(
                    "p",
                    "likelihood",
                    0.0,
                    "no base rate exists for this plan; severity is known, probability is not",
                ),
            });
        }
        if matches!(h.worst_reversibility(), Some(Reversibility::Unknown)) {
            out.push(FailureMode {
                label: "unclassified step".into(),
                detail: "a step nobody has classified may be the irreversible one".into(),
                likelihood: Term::absent("p", "likelihood", 0.0, "unclassified by construction"),
            });
        }
        for s in world.risks().filter(|s| self.touches(h, &s.targets)) {
            out.push(FailureMode {
                label: s.label.clone(),
                detail: s.detail.clone(),
                likelihood: if s.measured {
                    Term::measured("p", "likelihood", s.magnitude, format!("counted signal {}", s.id))
                } else {
                    Term::absent(
                        "p",
                        "likelihood",
                        0.0,
                        format!("signal {} is an estimate, not a count", s.id),
                    )
                },
            });
        }
        if !world.blind_spots.is_empty() {
            out.push(FailureMode {
                label: "acting on a partly-unseen world".into(),
                detail: format!(
                    "the observer could not read: {}",
                    world.blind_spots.join("; ")
                ),
                likelihood: Term::absent(
                    "p",
                    "likelihood",
                    0.0,
                    "unknowable by definition — the point is that it was not seen",
                ),
            });
        }
        out
    }

    fn shadow(&self, world: &WorldState, h: &Hypothesis) -> ShadowDelta {
        let touched: Vec<String> = h.actions.iter().map(|a| a.target.clone()).collect();
        let addresses: Vec<String> = h.grounded_in.clone();
        let unaddressed: Vec<String> = world
            .signals
            .iter()
            .filter(|s| s.polarity == Polarity::Risk && !addresses.contains(&s.id))
            .map(|s| s.id.clone())
            .collect();
        ShadowDelta { touched_objects: touched, addresses_signals: addresses, unaddressed_risks: unaddressed }
    }
}

impl Simulator for StructuralSimulator {
    fn name(&self) -> &str {
        "structural"
    }

    fn project(&self, world: &WorldState, goal: &Goal, h: &Hypothesis) -> Projection {
        // Constraint checking happens here rather than in the policy layer so that a
        // forbidden branch is still *projected* and still shows up in the record. An agent
        // that silently drops the branch it was not allowed to take cannot explain the
        // shape of its own choice afterwards.
        let forbidden_by = h.actions.iter().find_map(|a| {
            goal.violated_by(&a.target)
                .or_else(|| goal.violated_by(&a.detail))
                .map(|c| format!("{:?} {}: {}", c.kind, c.subject, c.detail))
        });

        let expected_gain = self.gain_term(world, h);
        let risk = self.risk_term(world, h);
        let cost = self.cost_term(h);
        let uncertainty = self.uncertainty_term(world);
        let reversibility = self.reversibility_term(h);
        let coverage = Coverage::of(&[&expected_gain, &risk, &cost, &uncertainty, &reversibility]);

        Projection {
            hypothesis: h.id.clone(),
            simulator: self.name().to_string(),
            expected_gain,
            risk,
            cost,
            uncertainty,
            reversibility,
            failure_modes: self.failure_modes(world, h),
            shadow: self.shadow(world, h),
            forbidden_by,
            coverage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_world::{
        Action, Constraint, Domain, Entity, EntityKind, Extent, HypothesisOrigin, Object,
        Provenance, RiskClass,
    };

    fn sig(id: &str, polarity: Polarity, magnitude: f64, measured: bool, targets: &[&str]) -> Signal {
        Signal {
            id: id.into(),
            polarity,
            label: id.into(),
            detail: String::new(),
            magnitude,
            measured,
            targets: targets.iter().map(|s| s.to_string()).collect(),
            evidence: vec![],
        }
    }

    fn world(signals: Vec<Signal>, blind: Vec<String>) -> WorldState {
        WorldState {
            schema: Some(scema_world::WORLD_SCHEMA.into()),
            observer: "test".into(),
            entity: Entity { kind: EntityKind::Repository, locator: ".".into(), label: "t".into() },
            domain: Domain::Software,
            observed_at: 0,
            objects: vec![Object::new("o1", "file", "o1", Provenance::Live { age_secs: 0 })],
            facts: vec![],
            signals,
            extent: Extent::complete(1, "walked"),
            blind_spots: blind,
        }
    }

    fn hyp(id: &str) -> Hypothesis {
        Hypothesis::new(id, "do a thing", HypothesisOrigin::Heuristic { rule: "t".into() })
    }

    #[test]
    fn an_ungrounded_hypothesis_gets_no_expected_gain() {
        // The headline rule. A plausible-sounding branch with nothing behind it must not
        // score, however good it sounds.
        let w = world(vec![], vec![]);
        let g = Goal::new("g", "improve");
        let p = StructuralSimulator.project(&w, &g, &hyp("h1"));
        assert_eq!(p.expected_gain.value, 0.0);
        assert!(!p.expected_gain.measured);
        assert!(p.expected_gain.note.contains("no signal"));
    }

    #[test]
    fn an_estimated_signal_does_not_become_a_measured_gain() {
        let w = world(vec![sig("s1", Polarity::Opportunity, 0.9, false, &[])], vec![]);
        let g = Goal::new("g", "improve");
        let h = hyp("h1").grounded("s1");
        let p = StructuralSimulator.project(&w, &g, &h);
        assert!(!p.expected_gain.measured, "a guessed magnitude must not launder into a measurement");
        assert_eq!(p.expected_gain.value, 0.0);
    }

    #[test]
    fn a_counted_signal_does_produce_a_measured_gain() {
        let w = world(vec![sig("s1", Polarity::Opportunity, 0.6, true, &[])], vec![]);
        let g = Goal::new("g", "improve");
        let p = StructuralSimulator.project(&w, &g, &hyp("h1").grounded("s1"));
        assert!(p.expected_gain.measured);
        assert!((p.expected_gain.value - 0.6).abs() < 1e-9);
    }

    #[test]
    fn a_dangling_citation_is_dropped_not_trusted() {
        let w = world(vec![], vec![]);
        let g = Goal::new("g", "improve");
        let p = StructuralSimulator.project(&w, &g, &hyp("h1").grounded("s-does-not-exist"));
        assert!(!p.expected_gain.measured);
    }

    #[test]
    fn a_forbidden_branch_is_projected_but_marked() {
        let w = world(vec![], vec![]);
        let g = Goal::new("g", "improve").with_constraint(Constraint::must_not("config.toml", "no"));
        let h = hyp("h1").doing(Action::new(
            "a1",
            RiskClass::Write,
            "crates/x/config.toml",
            "edit",
            Reversibility::Trivial,
        ));
        let p = StructuralSimulator.project(&w, &g, &h);
        assert!(p.forbidden_by.is_some(), "the branch must still appear in the record");
    }

    #[test]
    fn unknown_reversibility_is_absent_rather_than_zero_scored() {
        let w = world(vec![], vec![]);
        let g = Goal::new("g", "improve");
        let h = hyp("h1").doing(Action::new(
            "a1",
            RiskClass::Write,
            "x",
            "y",
            Reversibility::Unknown,
        ));
        let p = StructuralSimulator.project(&w, &g, &h);
        assert!(!p.reversibility.measured);
        assert_eq!(p.reversibility.value, 0.0);
        assert!(p.failure_modes.iter().any(|f| f.label == "unclassified step"));
    }

    #[test]
    fn blind_spots_raise_uncertainty_and_add_a_named_failure_mode() {
        let clean = StructuralSimulator.project(&world(vec![], vec![]), &Goal::new("g", "x"), &hyp("h"));
        let blind = StructuralSimulator.project(
            &world(vec![], vec!["target/ (permission denied)".into()]),
            &Goal::new("g", "x"),
            &hyp("h"),
        );
        assert!(blind.uncertainty.value > clean.uncertainty.value);
        assert!(blind.failure_modes.iter().any(|f| f.label.contains("unseen")));
    }

    #[test]
    fn coverage_reports_how_many_of_the_five_terms_were_real() {
        let w = world(vec![], vec![]);
        let p = StructuralSimulator.project(&w, &Goal::new("g", "x"), &hyp("h"));
        // No actions and no citations: only uncertainty is measurable.
        assert_eq!(p.coverage.label(), "1/5");
    }

    #[test]
    fn unaddressed_risks_are_reported_even_when_the_plan_ignores_them() {
        let w = world(vec![sig("r1", Polarity::Risk, 0.8, true, &[])], vec![]);
        let p = StructuralSimulator.project(&w, &Goal::new("g", "x"), &hyp("h"));
        assert_eq!(p.shadow.unaddressed_risks, vec!["r1".to_string()]);
    }
}
