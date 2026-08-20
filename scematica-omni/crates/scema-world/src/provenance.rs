//! [`Provenance`]: whether a perceived value can be seen at all, and how recently.
//!
//! Mirrors `scematica_mesh::node::Provenance` arm for arm, deliberately. The bot's mesh
//! answers the question for *state files written by the sniper*; this one answers it for
//! *arbitrary environments an agent perceives* — a repository, a web page, a process, a
//! market. Same four arms, same semantics, two different subjects. If they ever diverge,
//! `scematica-mesh` is authoritative for bot state and this type is authoritative for
//! perception; neither is a port of the other.
//!
//! The ordering rule is the load-bearing part, and it is the same one:
//!
//! > Answer "**can this be seen?**" first, and only then report the value.
//!
//! A renderer that draws an unreadable object as `0` makes a claim — *this thing is
//! empty* — indistinguishable on screen from the true statement *we could not read this
//! thing*. One is an observation and the other is an accusation. So [`Provenance::Absent`]
//! carries no value, and every consumer is expected to render it as dark rather than idle.

use serde::{Deserialize, Serialize};

/// Where an observation came from, and whether it can still be believed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Provenance {
    /// Read from a real source, within that source's freshness budget.
    Live { age_secs: u64 },
    /// Read from a real source, but older than its budget. The value is real and was true
    /// once; it is not true *now* and must not be presented as current.
    ///
    /// Budgets are per-source and measured, never a shared constant: a DOM read is stale
    /// in seconds, a git history is not stale in a day.
    Stale { age_secs: u64, budget_secs: u64 },
    /// The source does not exist, or could not be read. **Not zero, not empty — unseen.**
    /// The arm carries no value, which is the entire point of it being an arm.
    Absent,
    /// Produced by a simulator rather than observed. Kept distinct rather than folded into
    /// `Live` because this whole stack is built on the rule that simulated output is
    /// labelled at every point it surfaces.
    Simulated,
}

impl Provenance {
    /// Read within budget, from a source with the given budget.
    pub fn fresh(age_secs: u64, budget_secs: u64) -> Self {
        if age_secs <= budget_secs {
            Provenance::Live { age_secs }
        } else {
            Provenance::Stale { age_secs, budget_secs }
        }
    }

    /// May a reader act on this value?
    ///
    /// `Stale` is deliberately **not** actionable. A value that was true an hour ago looks
    /// exactly like one that is true now, and that resemblance is the whole hazard.
    pub fn is_actionable(&self) -> bool {
        matches!(self, Provenance::Live { .. })
    }

    /// Is anything at all known here?
    pub fn is_visible(&self) -> bool {
        !matches!(self, Provenance::Absent)
    }

    /// Short label for rendering.
    pub fn label(&self) -> &'static str {
        match self {
            Provenance::Live { .. } => "LIVE",
            Provenance::Stale { .. } => "STALE",
            Provenance::Absent => "ABSENT",
            Provenance::Simulated => "SIMULATED",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_is_visible_but_not_actionable() {
        let p = Provenance::fresh(120, 30);
        assert!(matches!(p, Provenance::Stale { .. }));
        assert!(p.is_visible());
        assert!(!p.is_actionable(), "stale values must not be treated as current");
    }

    #[test]
    fn absent_carries_no_value_and_is_not_visible() {
        let p = Provenance::Absent;
        assert!(!p.is_visible());
        assert!(!p.is_actionable());
    }

    #[test]
    fn simulated_never_reads_as_live() {
        assert!(!Provenance::Simulated.is_actionable());
        assert_eq!(Provenance::Simulated.label(), "SIMULATED");
    }
}
