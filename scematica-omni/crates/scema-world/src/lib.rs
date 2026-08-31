//! # scema-world — the universal state representation
//!
//! The bottom of Scematica Omni. Everything above it — perception, memory, simulation,
//! policy, verification — is written against these types and against nothing else, which
//! is what lets one agent loop run over a git repository, a web page and a market without
//! a branch per domain.
//!
//! This crate is **pure**: no I/O, no clock, no network, no filesystem. It is compiled
//! into the CLI, will be compiled into the daemon, and its JSON shape is the wire format
//! the browser extension speaks. Every dependency it acquires is one a reimplementer has
//! to match, so it has two: `serde` and `serde_json`.
//!
//! ## The three rules this crate exists to enforce
//!
//! 1. **[`Provenance`] before value.** An unreadable object is [`Provenance::Absent`] and
//!    carries no value at all. Rendering it as `0` turns "we could not see this" into
//!    "this is empty", which is an accusation rather than an observation.
//! 2. **[`Term`] carries its own evidence.** An unmeasured quantity contributes the
//!    neutral element and is flagged, so a score can never quietly stand on nothing. Every
//!    aggregate ships a [`Coverage`] beside it.
//! 3. **Ordered containers only.** `scema-verify` hashes these structures; a `HashMap`
//!    would give the same world two digests and make every decision record unverifiable.
//!
//! ## Shape of a cycle
//!
//! ```text
//!   Entity ──observe──▶ WorldState ──propose──▶ [Hypothesis]
//!                            │                      │
//!                            └──────────────────────┴──simulate──▶ [Projection]
//!                                                                      │
//!                                                        Goal ─────────┴──score──▶ Decision
//! ```
//!
//! `WorldState`, `Goal` and `Hypothesis` live here. `Projection` lives in `scema-sim` and
//! `Decision` in `scema-policy`, because both are computed rather than observed.

pub mod features;
pub mod goal;
pub mod hypothesis;
pub mod provenance;
pub mod term;
pub mod world;

pub use features::WorldFeatures;
pub use goal::{Constraint, ConstraintKind, Goal, GOAL_HYPOTHESIS_ID};
pub use hypothesis::{Action, Hypothesis, HypothesisOrigin, Reversibility, RiskClass};
pub use provenance::Provenance;
pub use term::{Coverage, Term};
pub use world::{
    parse_schema, Domain, Entity, EntityKind, Extent, Fact, Object, Polarity, Scalar, Signal,
    WorldState, WORLD_SCHEMA, WORLD_SCHEMA_MAJOR,
};

/// Unix seconds, for observers that need to stamp a `WorldState`.
///
/// Lives here rather than in each observer so that a test can be written against a fixed
/// clock by constructing the timestamp directly. This crate takes no clock dependency; it
/// only exposes the one call `std` already provides.
pub fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
