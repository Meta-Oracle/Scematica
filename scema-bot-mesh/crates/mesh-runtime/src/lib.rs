//! `mesh-runtime` — the surface a game integrates against.
//!
//! [`mesh_core`] gives deterministic inference and per-claim commitments. This crate makes
//! them usable at frame rate:
//!
//! * [`agent::Agent`] — load a policy, act, accumulate claims.
//! * [`batch::ClaimBatch`] — many claims under one 32-byte root, with inclusion proofs.
//! * [`format`] — the `.mesh` weight file, bit-exact so the commitment survives a save.
//!
//! ```
//! use mesh_core::{fixed::Fx, net::PolicyNet};
//! use mesh_runtime::{Agent, RecordMode};
//!
//! let mut agent = Agent::new(PolicyNet::new(4, &[8], 3), RecordMode::All);
//!
//! let action = agent.act(&[Fx::ONE, Fx::ZERO, Fx::from_f64(0.5), Fx::ZERO]).unwrap();
//! assert!(action < 3);
//!
//! // Later, off the hot path: one root for the whole batch.
//! let anchor = agent.flush().unwrap();
//! assert_eq!(anchor.claim_count, 1);
//! ```
//!
//! The division of labour is deliberate: acting is synchronous and local, anchoring is
//! batched and asynchronous. A chain outage delays commitments; it never drops a frame.

#![forbid(unsafe_code)]

pub mod agent;
pub mod batch;
pub mod format;

pub use agent::{Agent, AgentError, Anchor, RecordMode};
pub use batch::{verify_proof, ClaimBatch, ProofStep};
pub use format::FormatError;
