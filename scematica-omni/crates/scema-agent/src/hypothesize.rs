//! [`Hypothesizer`]: where competing futures come from.
//!
//! The branches are as good as this layer, and this layer is deliberately dumb. Three
//! implementations ship:
//!
//! * [`SignalHypothesizer`] — one branch per counted signal in the world. Every branch it
//!   makes is grounded by construction, which is the only way a branch can earn a measured
//!   expected gain downstream.
//! * [`GoalHypothesizer`] — the branch that is simply *what was asked for*. It is grounded
//!   only by [`Goal::grounded_in`], which the operator sets deliberately; it never infers
//!   grounding from the wording. Ungrounded, it scores at or below zero and the agent
//!   abstains. That is the honest outcome: **an instruction is not evidence.**
//!
//!   An earlier version did infer it, by keyword overlap, and the first run against this
//!   repository grounded "add tests to the scema-cli crate" in a marker backlog in a
//!   different crate — `scema` being a substring of every unit name here. The branch
//!   inherited a measured expected gain from unrelated evidence, which is exactly the
//!   laundering `scema-sim` refuses to do. See [`Goal::grounded_in`].
//! * [`MemoryHypothesizer`] — procedures that worked on this subject before.
//!
//! ## The slot that is empty on purpose
//!
//! A model-backed hypothesiser — an LLM reading the world state and proposing branches — is
//! the obvious fourth, and [`HypothesisOrigin::Model`] exists for it. It is not implemented
//! here because it changes what the runtime is: every other component in this workspace is
//! deterministic and reproducible, which is what makes a decision record verifiable at all.
//! A model in this position is fine — it only *proposes*, and the simulator still refuses
//! to score an ungrounded branch — but it needs its prompt, its model id and its raw output
//! committed into the record, and that is a design step rather than a wiring step.

use scema_memory::{MemoryBody, MemoryKind, MemoryStore, Recall};
use scema_world::{
    Action, Goal, Hypothesis, HypothesisOrigin, Polarity, RiskClass, Signal, WorldState,
    GOAL_HYPOTHESIS_ID,
};

/// Something that proposes candidate futures.
pub trait Hypothesizer {
    fn name(&self) -> &str;
    fn propose(&self, world: &WorldState, goal: &Goal) -> Vec<Hypothesis>;
}

// What reversing an edit costs is a property of the domain, so the table lives on
// `Domain::edit_reversibility` rather than here. It moved when `Domain` became an open enum:
// a `match` with a `_ => Unknown` arm in this file was fine while there were four domains
// and quietly wrong once a producer could name its own, because every new domain would have
// landed on the fallback without anyone reading this function again.

/// One branch per counted signal.
#[derive(Clone, Debug, Default)]
pub struct SignalHypothesizer;

impl SignalHypothesizer {
    fn action_for(&self, world: &WorldState, s: &Signal, idx: usize) -> Action {
        let target = s
            .targets
            .first()
            .cloned()
            .unwrap_or_else(|| world.entity.locator.clone());
        Action::new(
            format!("a{idx}"),
            RiskClass::Write,
            target,
            format!("address `{}`", s.label),
            world.domain.edit_reversibility(),
        )
    }
}

impl Hypothesizer for SignalHypothesizer {
    fn name(&self) -> &str {
        "signal"
    }

    fn propose(&self, world: &WorldState, _goal: &Goal) -> Vec<Hypothesis> {
        world
            .signals
            .iter()
            .enumerate()
            // Only counted signals. An estimated one would produce a branch that looks
            // grounded and is not, and `scema-sim` would then have to unpick it.
            .filter(|(_, s)| s.measured)
            .map(|(i, s)| {
                let verb = match s.polarity {
                    Polarity::Risk => "mitigate",
                    Polarity::Opportunity => "take",
                };
                Hypothesis::new(
                    format!("h-{}", s.id.replace([':', '/', ' '], "-")),
                    format!("{verb}: {}", s.label),
                    HypothesisOrigin::Heuristic { rule: format!("one branch per counted signal ({})", s.id) },
                )
                .because(format!("{} — {}", s.detail, s.evidence.join("; ")))
                .grounded(s.id.clone())
                .doing(self.action_for(world, s, i))
            })
            .collect()
    }
}

/// The branch that is exactly what the operator asked for.
#[derive(Clone, Debug, Default)]
pub struct GoalHypothesizer;

impl Hypothesizer for GoalHypothesizer {
    fn name(&self) -> &str {
        "goal"
    }

    fn propose(&self, world: &WorldState, goal: &Goal) -> Vec<Hypothesis> {
        if goal.statement.trim().is_empty() {
            return vec![];
        }
        let mut h =
            Hypothesis::new(GOAL_HYPOTHESIS_ID, goal.statement.clone(), HypothesisOrigin::Human)
            .because(if goal.grounded_in.is_empty() {
                "the operator asked for this and cited no counted signal; an instruction is not evidence"
                    .to_string()
            } else {
                format!(
                    "the operator asserts this addresses: {}",
                    goal.grounded_in.join(", ")
                )
            })
            .doing(Action::new(
                "a0",
                RiskClass::Write,
                world.entity.locator.clone(),
                goal.statement.clone(),
                world.domain.edit_reversibility(),
            ));
        for g in &goal.grounded_in {
            h = h.grounded(g.clone());
        }
        vec![h]
    }
}

/// Branches recalled from procedures that have worked on this subject.
pub struct MemoryHypothesizer<'a> {
    store: &'a MemoryStore,
}

impl<'a> MemoryHypothesizer<'a> {
    pub fn new(store: &'a MemoryStore) -> Self {
        MemoryHypothesizer { store }
    }
}

impl Hypothesizer for MemoryHypothesizer<'_> {
    fn name(&self) -> &str {
        "memory"
    }

    fn propose(&self, world: &WorldState, _goal: &Goal) -> Vec<Hypothesis> {
        let query = Recall::about(world.entity.locator.clone()).limit(5);
        let Ok(hits) = self.store.recall(MemoryKind::Procedural, &query) else {
            // A memory that cannot be read yields no branches. It must not be an error:
            // the loop has to run on a machine with no history at all.
            return vec![];
        };
        hits.iter()
            .filter_map(|r| match &r.body {
                MemoryBody::Procedure { name, steps, successes, failures } => {
                    // A procedure that has failed more often than it has worked is recalled
                    // but not proposed. Recording it and re-proposing it are different
                    // things, and only the second is a recommendation.
                    if failures > successes {
                        return None;
                    }
                    let mut h = Hypothesis::new(
                        format!("h-mem-{}", r.id),
                        format!("apply the `{name}` procedure"),
                        HypothesisOrigin::Memory { record: r.id.clone() },
                    )
                    .because(format!(
                        "{successes} success(es), {failures} failure(s) recorded against this procedure"
                    ));
                    for (i, step) in steps.iter().enumerate() {
                        h = h.doing(Action::new(
                            format!("a{i}"),
                            RiskClass::Write,
                            world.entity.locator.clone(),
                            step.clone(),
                            world.domain.edit_reversibility(),
                        ));
                    }
                    Some(h)
                }
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_world::{Domain, Reversibility};
    use scema_world::{Entity, EntityKind, Extent, Object, Provenance};

    fn signal(id: &str, label: &str, measured: bool) -> Signal {
        Signal {
            id: id.into(),
            polarity: Polarity::Risk,
            label: label.into(),
            detail: "d".into(),
            magnitude: 0.5,
            measured,
            targets: vec!["unit:crates/x".into()],
            evidence: vec!["counted".into()],
        }
    }

    fn world(signals: Vec<Signal>, domain: Domain) -> WorldState {
        WorldState {
            schema: Some(scema_world::WORLD_SCHEMA.into()),
            observer: "t".into(),
            entity: Entity {
                kind: EntityKind::Repository,
                locator: "/repo".into(),
                label: "repo".into(),
            },
            domain,
            observed_at: 0,
            objects: vec![Object::new("o", "file", "o", Provenance::Live { age_secs: 0 })],
            facts: vec![],
            signals,
            extent: Extent::complete(1, "t"),
            blind_spots: vec![],
        }
    }

    #[test]
    fn only_counted_signals_become_branches() {
        let w = world(
            vec![signal("s1", "counted thing", true), signal("s2", "guessed thing", false)],
            Domain::Software,
        );
        let hs = SignalHypothesizer.propose(&w, &Goal::new("g", "x"));
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].grounded_in, vec!["s1".to_string()]);
    }

    #[test]
    fn every_signal_branch_is_grounded_by_construction() {
        let w = world(vec![signal("s1", "a", true), signal("s2", "b", true)], Domain::Software);
        let hs = SignalHypothesizer.propose(&w, &Goal::new("g", "x"));
        assert!(hs.iter().all(|h| !h.grounded_in.is_empty()));
    }

    #[test]
    fn a_goal_never_grounds_itself_from_its_own_wording() {
        // The regression this rule exists for. The goal names the `x` crate and a signal
        // about the `x` crate is right there, and it still must not be picked up: word
        // overlap grounded an unrelated crate the first time it was tried, because every
        // unit name in the host repository shares a prefix.
        let w = world(vec![signal("untested:x", "`x` has no tests", true)], Domain::Software);
        let hs = GoalHypothesizer.propose(&w, &Goal::new("g", "add tests to the x crate"));
        assert_eq!(hs.len(), 1);
        assert!(
            hs[0].grounded_in.is_empty(),
            "an instruction is not evidence; this branch must not borrow grounding"
        );
    }

    #[test]
    fn an_operator_can_ground_a_goal_deliberately() {
        let w = world(vec![signal("untested:x", "`x` has no tests", true)], Domain::Software);
        let g = Goal::new("g", "add tests to the x crate").grounded("untested:x");
        let hs = GoalHypothesizer.propose(&w, &g);
        assert_eq!(hs[0].grounded_in, vec!["untested:x".to_string()]);
        assert!(hs[0].rationale.contains("operator asserts"));
    }

    #[test]
    fn an_unknown_domain_yields_unclassified_reversibility() {
        // The conservative default. Only a domain whose undo cost is actually understood
        // may claim a reversibility, and everything else stays unmeasured downstream.
        let w = world(vec![signal("s1", "a", true)], Domain::Unknown);
        let hs = SignalHypothesizer.propose(&w, &Goal::new("g", "x"));
        assert_eq!(hs[0].worst_reversibility(), Some(Reversibility::Unknown));
    }

    #[test]
    fn an_empty_goal_proposes_nothing() {
        let w = world(vec![], Domain::Software);
        assert!(GoalHypothesizer.propose(&w, &Goal::new("g", "   ")).is_empty());
    }
}
