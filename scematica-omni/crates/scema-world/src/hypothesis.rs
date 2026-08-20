//! [`Hypothesis`] and [`Action`]: competing futures, and what it would take to reach them.
//!
//! The single structural difference between this runtime and a request/response assistant
//! lives here. An assistant produces *an* action; this produces a set of candidate futures
//! and lets the simulation and policy layers argue about them. A `Hypothesis` is therefore
//! never "the answer" — it is one branch, and it carries [`HypothesisOrigin`] so a reader
//! can tell a rule from a model from a memory from a human instruction.
//!
//! [`Action`] classifies by *what it would cost to be wrong*, not by what it does. That is
//! why [`Reversibility`] and [`RiskClass`] are separate axes: reading a file you should not
//! have read is irreversible disclosure at zero mechanical risk, and restarting a service
//! is high mechanical risk that is trivially undone. Collapsing them into one "danger"
//! number loses exactly the distinction a policy needs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What an action would do, in the vocabulary the approval layer uses.
///
/// Mirrors the four risk classes in `alchem-link`'s `TrustPolicy` on purpose — an operator
/// who has learned one permission model should not have to learn a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskClass {
    Read,
    Network,
    Write,
    Execute,
    /// Moves money. Its own class, above `Execute`, because the loss is not recoverable by
    /// re-running anything.
    Financial,
}

impl RiskClass {
    /// A base hazard in `[0, 1]`. Ordinal, not empirical — it ranks classes against each
    /// other and is not a probability of anything.
    pub fn base_hazard(&self) -> f64 {
        match self {
            RiskClass::Read => 0.05,
            RiskClass::Network => 0.15,
            RiskClass::Write => 0.40,
            RiskClass::Execute => 0.65,
            RiskClass::Financial => 0.90,
        }
    }
}

/// How hard it would be to undo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reversibility {
    /// Undone by a single obvious step (git checkout, close the tab).
    Trivial,
    /// Undone with effort and a known procedure.
    Recoverable,
    /// Undone only at real cost.
    Costly,
    /// Not undoable at all.
    Irreversible,
    /// Nobody has determined which of the above applies.
    ///
    /// A distinct arm rather than a default of `Recoverable`, because an optimistic
    /// default here is how an agent talks itself into an irreversible action.
    Unknown,
}

impl Reversibility {
    /// Score in `[0, 1]`, higher is safer, or `None` when unknown.
    ///
    /// `None` rather than a middling number: the caller must decide what to do with
    /// ignorance, and in this workspace that means an unmeasured [`crate::Term`], not a
    /// 0.5 that reads as a measurement.
    pub fn score(&self) -> Option<f64> {
        match self {
            Reversibility::Trivial => Some(1.0),
            Reversibility::Recoverable => Some(0.7),
            Reversibility::Costly => Some(0.3),
            Reversibility::Irreversible => Some(0.0),
            Reversibility::Unknown => None,
        }
    }
}

/// One step the agent would take.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub verb: RiskClass,
    /// What it would act on: a path, a URL, an object id. Constraint matching runs against
    /// this and `detail`.
    pub target: String,
    pub detail: String,
    pub reversibility: Reversibility,
}

impl Action {
    pub fn new(
        id: impl Into<String>,
        verb: RiskClass,
        target: impl Into<String>,
        detail: impl Into<String>,
        reversibility: Reversibility,
    ) -> Self {
        Action {
            id: id.into(),
            verb,
            target: target.into(),
            detail: detail.into(),
            reversibility,
        }
    }
}

/// Where a candidate future came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HypothesisOrigin {
    /// A deterministic rule fired on something in the world.
    Heuristic { rule: String },
    /// A model proposed it. `name` is the model, so a record can be re-read years later
    /// and say which one.
    Model { name: String },
    /// The operator asked for exactly this.
    Human,
    /// Recalled from a stored procedure or a past episode.
    Memory { record: String },
}

/// One candidate future, and the steps that would produce it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hypothesis {
    pub id: String,
    pub statement: String,
    /// Why this was proposed. Free-form, but expected to cite object ids or signal ids
    /// from the world — a rationale that cites nothing is how an unfounded branch gets
    /// ranked next to a founded one.
    pub rationale: String,
    pub origin: HypothesisOrigin,
    pub actions: Vec<Action>,
    /// Signal ids from the world that motivated this. Empty is meaningful and common: it
    /// marks a hypothesis with no observed support, and `scema-sim` refuses to score an
    /// expected gain for one.
    pub grounded_in: Vec<String>,
    /// The extension point for domain-specific evaluators.
    ///
    /// A trading policy needs to know which trade this branch *is*; a deployment policy
    /// needs an environment name. Neither belongs in this crate, which must stay
    /// domain-agnostic, and neither should require a new field per specialism. So a
    /// specialist reads its own namespaced key (`dqstar.action`, `deploy.env`) and treats
    /// a missing one as grounds to decline, never to guess.
    ///
    /// `BTreeMap` for the same reason as everywhere else here: this is hashed.
    pub tags: BTreeMap<String, String>,
}

impl Hypothesis {
    pub fn new(
        id: impl Into<String>,
        statement: impl Into<String>,
        origin: HypothesisOrigin,
    ) -> Self {
        Hypothesis {
            id: id.into(),
            statement: statement.into(),
            rationale: String::new(),
            origin,
            actions: vec![],
            grounded_in: vec![],
            tags: BTreeMap::new(),
        }
    }

    pub fn because(mut self, rationale: impl Into<String>) -> Self {
        self.rationale = rationale.into();
        self
    }

    pub fn doing(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }

    pub fn grounded(mut self, signal_id: impl Into<String>) -> Self {
        self.grounded_in.push(signal_id.into());
        self
    }

    pub fn tagged(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// The most dangerous class among its actions. `None` for a hypothesis with no
    /// actions — a pure recommendation, which is a real and useful thing to rank.
    pub fn worst_risk_class(&self) -> Option<RiskClass> {
        self.actions.iter().map(|a| a.verb).max()
    }

    /// The least reversible of its actions. `Unknown` dominates: one step nobody has
    /// classified makes the whole plan unclassified, which is the conservative reading and
    /// the only one that does not launder ignorance into a safe-looking score.
    pub fn worst_reversibility(&self) -> Option<Reversibility> {
        if self.actions.is_empty() {
            return None;
        }
        if self.actions.iter().any(|a| a.reversibility == Reversibility::Unknown) {
            return Some(Reversibility::Unknown);
        }
        self.actions.iter().map(|a| a.reversibility).max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(v: RiskClass, r: Reversibility) -> Action {
        Action::new("a", v, "t", "d", r)
    }

    #[test]
    fn one_unknown_step_makes_the_whole_plan_unknown() {
        let h = Hypothesis::new("h", "s", HypothesisOrigin::Human)
            .doing(act(RiskClass::Read, Reversibility::Trivial))
            .doing(act(RiskClass::Write, Reversibility::Unknown));
        assert_eq!(h.worst_reversibility(), Some(Reversibility::Unknown));
    }

    #[test]
    fn worst_class_wins_not_the_last_one_added() {
        let h = Hypothesis::new("h", "s", HypothesisOrigin::Human)
            .doing(act(RiskClass::Financial, Reversibility::Trivial))
            .doing(act(RiskClass::Read, Reversibility::Trivial));
        assert_eq!(h.worst_risk_class(), Some(RiskClass::Financial));
    }

    #[test]
    fn an_actionless_hypothesis_is_valid_and_unclassified() {
        let h = Hypothesis::new("h", "recommend a benchmark first", HypothesisOrigin::Human);
        assert_eq!(h.worst_risk_class(), None);
        assert_eq!(h.worst_reversibility(), None);
    }

    #[test]
    fn unknown_reversibility_has_no_score_rather_than_a_middling_one() {
        assert_eq!(Reversibility::Unknown.score(), None);
        assert_eq!(Reversibility::Irreversible.score(), Some(0.0));
    }
}
