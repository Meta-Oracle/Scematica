//! # Scematica Mesh — the system's own topology
//!
//! Scematica is already a multi-agent system and has never been visible as one. In a
//! single sniper process there are three DQN variants competing in a tournament with a
//! promotion every 1000 steps, separate regime nets that swap in below ε 0.3, five LLM
//! agents, six independent risk breakers that can each halt buys on their own, a filter
//! pipeline where every stage is a decision, and a Ψ gate measuring whether any of it can
//! be believed. Until now the way to ask "why did nothing trade?" was to grep JSON files.
//!
//! This crate reads what the running system leaves on disk and returns a [`Mesh`]: the
//! decision-making units, the edges between them, what each one last decided, and — first,
//! before any of that — whether each one can be seen at all.
//!
//! ```no_run
//! use scematica_mesh::Collector;
//! let mesh = Collector::new(".").collect();
//! println!("{}", mesh.summary.diagnosis);
//! ```
//!
//! ## The four rules
//!
//! 1. **A node exists because its source exists.** No file means a dark node, never an
//!    invented one and never a zero. `0 trades` and `cannot see the executor` are
//!    different claims and only one of them accuses the system of idleness.
//! 2. **Provenance is per node, not per page.** A single "live/simulated" banner is
//!    useless on a graph where one unit is fresh, one is three months stale and five write
//!    nothing at all — which is the real state of this system today.
//! 3. **Freshness budgets are per source**, derived from each writer's own cadence. See
//!    [`collect::Source`].
//! 4. **Rust is authoritative.** The web renderer is a view; the shapes here are the
//!    contract.
//!
//! ## What it is not
//!
//! Not to be confused with `scema-bot-mesh`, which lives in its own workspace and solves a
//! different problem entirely (deterministic, challengeable neural inference for BOT
//! Chain). This crate makes no cryptographic claim and runs no inference. It observes.
//!
//! It is also **not a new distributed agent runtime**. The mesh it draws is the one that
//! already exists. Nothing here fabricates coordination that is not happening.
//!
//! ## v0.0.2
//!
//! The honest next step is wiring, not rendering: [`edge::EdgeKind::Experience`] exists and
//! is presently always inactive, because the tournament variants do not share experience
//! with each other even though `scemadex_sdk::mesh::ExperienceBatch` and `PeerMarket` were
//! designed for exactly that. Making that edge light up for real is a capability addition.
//! Drawing it lit before then would be the one thing this crate refuses to do.

pub mod cognition;
pub mod collect;
pub mod history;
pub mod edge;
pub mod node;
pub mod topology;

pub use cognition::{Cognition, Coherence, GateVerdict, RiskField, Signals, Term, Uncertainty, TAU_PSI, TAU_PSI_FULL};
pub use collect::Collector;
pub use edge::{Edge, EdgeKind};
pub use node::{analyse_veto, Node, NodeKind, Provenance, Verdict, VetoAnalysis, NN_VETO_REL_MARGIN};
pub use topology::{Mesh, MeshSummary};

#[cfg(test)]
mod tests {
    use super::*;

    /// The public surface a caller needs to render a mesh without reaching into modules.
    #[test]
    fn the_crate_root_exposes_what_a_renderer_needs() {
        let mesh = Mesh::new(vec![], vec![], "t".into());
        assert_eq!(mesh.summary.nodes_total, 0);
        assert_eq!(mesh.summary.visibility, 0.0);
    }
}
