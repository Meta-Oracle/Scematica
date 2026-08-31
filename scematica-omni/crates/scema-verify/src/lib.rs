//! # scema-verify — proof-carrying decisions
//!
//! An agent that can say *why* it acted is worth more than one that acted well, because the
//! second claim is only checkable through the first. This crate makes the "why" into an
//! artefact: a [`DecisionRecord`] holding the world as perceived, the goal as given, every
//! branch considered, every projected term with its provenance, the λ weights in force, and
//! the choice — under a commitment that a third party can recompute.
//!
//! ```text
//!   world ──┐
//!   goal ───┤
//!   hypotheses ─┼─▶ sha256 each ──▶ root ──▶ id (8 hex)
//!   projections ┤
//!   policy ─────┤
//!   decision ───┘
//! ```
//!
//! **Read [`record`]'s note on what this proves before quoting it at anyone.** Briefly: it
//! catches edits to a record, it does not attest that the world was as described, and it is
//! tamper-evident rather than tamper-proof until the [`Commitment::root`] is anchored
//! somewhere the author does not control.
//!
//! The encoding rules that make the digest reproducible live in [`canonical`], and they are
//! stricter than JSON: sorted keys, tagged types, normalised `-0.0` and NaN. `serde_json`'s
//! own output is not stable enough to hash.

pub mod outcome;
pub mod canonical;
pub mod record;
pub mod store;

pub use canonical::{canonical_bytes, digest, digest_of_digests, Digest};
pub use record::{verify, Commitment, DecisionRecord, Mismatch, Verification};
pub use store::RecordStore;

pub use outcome::{resolve, Calibration, Checked, Resolution, Unresolvable};
