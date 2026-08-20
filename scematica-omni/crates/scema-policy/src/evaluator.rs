//! [`Evaluator`]: a second opinion on a branch, with the right to have none.
//!
//! The utility equation in [`crate::utility`] is domain-blind by construction. Specialists
//! know things it does not — a trading policy has a value head, a deployment policy knows
//! the change window, a security reviewer knows the CVE feed. This trait is how they get a
//! vote.
//!
//! ## [`Applicability`] is the whole design
//!
//! Every evaluator must first answer *whether it has any business scoring this at all*,
//! and it must be able to say no in two distinguishable ways:
//!
//! * [`Applicability::OutOfDomain`] — "this is not the kind of thing I know about". A
//!   trading agent asked to rank a refactor.
//! * [`Applicability::Insufficient`] — "this is my domain, but I lack the inputs". The same
//!   trading agent on a market world, with no checkpoint loaded or too few training steps.
//!
//! Collapsing those into one "no score" loses the operator's next action: the first is
//! permanent and fine, the second is a missing file they can go and supply.
//!
//! The failure this prevents is the one that makes multi-model systems untrustworthy: a
//! specialist that answers anyway. Its output is correctly shaped, arrives with a
//! confidence, ranks alongside the real ones, and nothing downstream can tell. So
//! [`Evaluator::score`] is only ever called after [`Evaluator::applicability`] returns
//! [`Applicability::Applicable`], and returning `None` from `score` after claiming
//! applicability is a bug in the evaluator, not a soft signal.

use scema_sim::Projection;
use scema_world::{Goal, Hypothesis, Term, WorldState};
use serde::{Deserialize, Serialize};

/// Whether an evaluator should be consulted at all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Applicability {
    Applicable { note: String },
    /// Not this evaluator's kind of problem. Permanent for this world.
    OutOfDomain { note: String },
    /// The right kind of problem, but the inputs are missing. `note` must say which,
    /// because this is the arm an operator can act on.
    Insufficient { note: String },
}

impl Applicability {
    pub fn is_applicable(&self) -> bool {
        matches!(self, Applicability::Applicable { .. })
    }

    pub fn note(&self) -> &str {
        match self {
            Applicability::Applicable { note }
            | Applicability::OutOfDomain { note }
            | Applicability::Insufficient { note } => note,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Applicability::Applicable { .. } => "APPLICABLE",
            Applicability::OutOfDomain { .. } => "OUT-OF-DOMAIN",
            Applicability::Insufficient { .. } => "INSUFFICIENT",
        }
    }
}

/// One specialist's opinion of one branch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evaluation {
    pub evaluator: String,
    /// The specialist's own utility for this branch. Not commensurable with another
    /// evaluator's — see [`crate::decide`], which never averages them.
    pub utility: Term,
    /// How sure the specialist is, on its own scale. Unmeasured when the specialist has no
    /// way to quantify its own confidence, which is the common case and must not be
    /// papered over with a constant.
    pub confidence: Term,
    pub note: String,
}

/// A source of second opinions.
pub trait Evaluator {
    /// Stable name; hashed into the decision record.
    fn name(&self) -> &str;

    /// One sentence on what this evaluator knows, for `scema policy`.
    fn about(&self) -> &str;

    /// May this evaluator be consulted about this world and goal? Called once per cycle,
    /// not once per branch — applicability is a property of the problem, not the branch.
    fn applicability(&self, world: &WorldState, goal: &Goal) -> Applicability;

    /// Score one branch. Only called when [`Evaluator::applicability`] was
    /// [`Applicability::Applicable`].
    fn score(
        &self,
        world: &WorldState,
        goal: &Goal,
        hypothesis: &Hypothesis,
        projection: &Projection,
    ) -> Option<Evaluation>;
}
