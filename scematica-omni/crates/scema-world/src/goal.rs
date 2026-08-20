//! [`Goal`]: what the agent is trying to bring about, and what it may not do to get there.
//!
//! Constraints are part of the goal rather than a separate policy object on purpose. A
//! goal without them ("optimise this application") is satisfiable by deleting the tests,
//! and every safety layer that tries to catch that afterwards is arguing with an objective
//! it was not given. Here the objective itself carries the prohibitions, so a hypothesis
//! that violates one is not *penalised* — it is [`Constraint::forbids`] and never ranked.

use serde::{Deserialize, Serialize};

/// A hard limit on how a goal may be pursued.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Constraint {
    pub kind: ConstraintKind,
    /// What is constrained, in the observer's vocabulary: an object id, a path prefix, a
    /// verb name. Matching is by substring against an action's target and detail, which is
    /// crude and deliberately over-broad: a constraint that fails to match is a permission
    /// nobody granted.
    pub subject: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    /// The agent must never do this. Absolute; no utility can outweigh it.
    MustNot,
    /// Whatever else changes, this must still hold afterwards.
    MustPreserve,
    /// A ceiling on resource use, expressed in `detail`.
    Budget,
    /// A time limit, expressed in `detail`.
    Deadline,
}

impl Constraint {
    pub fn must_not(subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Constraint { kind: ConstraintKind::MustNot, subject: subject.into(), detail: detail.into() }
    }

    pub fn must_preserve(subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Constraint { kind: ConstraintKind::MustPreserve, subject: subject.into(), detail: detail.into() }
    }

    /// Does this constraint forbid touching `target` outright?
    ///
    /// Only `MustNot` forbids. `MustPreserve` is a post-condition, not a prohibition: it
    /// says the thing must still hold afterwards, which is checkable only after the fact
    /// and therefore raises risk rather than blocking.
    pub fn forbids(&self, target: &str) -> bool {
        self.kind == ConstraintKind::MustNot
            && !self.subject.is_empty()
            && target.to_lowercase().contains(&self.subject.to_lowercase())
    }
}

/// What the agent is being asked to achieve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    /// Stable id so a decision record can cite the goal it was serving.
    pub id: String,
    /// The request, in the operator's words. Kept verbatim — a normalised or
    /// model-rewritten goal is a different goal, and the record has to show the one that
    /// was actually given.
    pub statement: String,
    pub constraints: Vec<Constraint>,
    /// Free-form horizon ("this session", "before the next deploy"). Uninterpreted; it
    /// exists so a human reading an old record knows what "soon" meant at the time.
    pub horizon: Option<String>,
    /// Signal ids the **operator asserts** this goal addresses.
    ///
    /// This field exists because the alternative was tried and was wrong. An earlier
    /// version inferred grounding by keyword overlap between the goal statement and the
    /// signals, and on the first real run it grounded "add tests to the scema-cli crate" in
    /// a marker backlog in a different crate — because `scema` is a substring of every unit
    /// name in this repository. The branch then inherited a *measured* expected gain from
    /// evidence that had nothing to do with it, which is precisely the laundering the
    /// simulator refuses to do on its own.
    ///
    /// So there is no inference. A goal is an instruction, and an instruction is not
    /// evidence: an ungrounded goal branch scores at or below zero and the agent abstains,
    /// which is the true answer. When the operator does know the goal addresses a counted
    /// signal, they say so, the claim is hashed into the decision record along with
    /// everything else, and a reader can see who made it.
    ///
    /// An id naming no signal in the observed world is dropped by the simulator rather than
    /// trusted — see `scema_sim::StructuralSimulator`.
    pub grounded_in: Vec<String>,
}

impl Goal {
    pub fn new(id: impl Into<String>, statement: impl Into<String>) -> Self {
        Goal {
            id: id.into(),
            statement: statement.into(),
            constraints: vec![],
            horizon: None,
            grounded_in: vec![],
        }
    }

    /// Assert that this goal addresses a counted signal.
    pub fn grounded(mut self, signal_id: impl Into<String>) -> Self {
        self.grounded_in.push(signal_id.into());
        self
    }

    pub fn with_constraint(mut self, c: Constraint) -> Self {
        self.constraints.push(c);
        self
    }

    /// The first constraint that forbids `target`, if any.
    pub fn violated_by(&self, target: &str) -> Option<&Constraint> {
        self.constraints.iter().find(|c| c.forbids(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn must_not_forbids_and_must_preserve_does_not() {
        let g = Goal::new("g1", "speed this up")
            .with_constraint(Constraint::must_not("config.toml", "never edit live config"))
            .with_constraint(Constraint::must_preserve("tests", "the suite must still pass"));

        assert!(g.violated_by("crates/x/config.toml").is_some());
        // MustPreserve is a post-condition, not a veto: it must not block a hypothesis
        // that touches tests, only make the outcome checkable.
        assert!(g.violated_by("crates/x/tests/mod.rs").is_none());
    }

    #[test]
    fn an_empty_subject_forbids_nothing() {
        // Guards the substring rule: `"".contains` is true for every target, which would
        // turn a malformed constraint into a total ban and look like a bug in the agent.
        let g = Goal::new("g", "x").with_constraint(Constraint::must_not("", "malformed"));
        assert!(g.violated_by("anything").is_none());
    }
}
