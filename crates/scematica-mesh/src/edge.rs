//! Edges: how one unit's decision reaches another.
//!
//! Edges are typed rather than uniform because the interesting question an operator asks
//! is never "is there a connection" — the wiring is static and they already know it — but
//! "**which connection is currently stopping things**". A veto edge and a signal edge look
//! identical on a plain graph and mean opposite things.

use serde::{Deserialize, Serialize};

/// What kind of influence an edge carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    /// Ordinary forward flow: pool events moving toward execution.
    Signal,
    /// A unit that can block the target. Drawn distinctly, and the only kind allowed to
    /// look alarming when active.
    Veto,
    /// A permission check the target must clear — the Ψ gate over the LLM agents.
    Gate,
    /// Tournament promotion: a variant becoming primary.
    Promotion,
    /// Learned experience moving between agents. Presently only ever inactive, because
    /// nothing wires it yet; see the v0.0.2 note in `lib.rs`.
    Experience,
}

/// A directed connection between two [`crate::node::Node`]s.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    /// Source node id.
    pub from: String,
    /// Target node id.
    pub to: String,
    pub kind: EdgeKind,
    /// Is this edge doing something right now?
    ///
    /// For [`EdgeKind::Veto`] this means *actively blocking*, which is the single most
    /// important bit on the whole page. `None` means the endpoints could not be read, and
    /// must not be rendered as `false` — an unreadable veto is not a cleared one.
    pub active: Option<bool>,
    /// Optional short label rendered on the edge, e.g. `171/474 passed`.
    pub label: Option<String>,
}

impl Edge {
    pub fn signal(from: &str, to: &str) -> Self {
        Edge { from: from.into(), to: to.into(), kind: EdgeKind::Signal, active: None, label: None }
    }

    pub fn veto(from: &str, to: &str) -> Self {
        Edge { from: from.into(), to: to.into(), kind: EdgeKind::Veto, active: None, label: None }
    }

    pub fn gate(from: &str, to: &str) -> Self {
        Edge { from: from.into(), to: to.into(), kind: EdgeKind::Gate, active: None, label: None }
    }

    pub fn with_active(mut self, active: Option<bool>) -> Self {
        self.active = active;
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Is this edge currently blocking flow? Only a veto edge can, and only when it is
    /// known to be active — an unknown veto reports `false` here but renders as unknown,
    /// because the summary must not accuse a gate of being shut on missing evidence.
    pub fn is_blocking(&self) -> bool {
        self.kind == EdgeKind::Veto && self.active == Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_active_veto_edges_block() {
        assert!(Edge::veto("a", "b").with_active(Some(true)).is_blocking());
        assert!(!Edge::veto("a", "b").with_active(Some(false)).is_blocking());
        assert!(!Edge::signal("a", "b").with_active(Some(true)).is_blocking());
    }

    /// An unreadable veto is not a cleared veto. It reports `false` for the blocking
    /// count (which counts *known* blocks) while keeping `active: None` so the renderer
    /// can draw the difference.
    #[test]
    fn an_unknown_veto_is_not_counted_as_clear() {
        let e = Edge::veto("a", "b");
        assert!(!e.is_blocking());
        assert_eq!(e.active, None, "the uncertainty must survive into the payload");
    }
}
