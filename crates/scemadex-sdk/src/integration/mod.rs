//! Bridge from the lean SDK traits to the real Scematica Deep Q* agent.
//!
//! Feature-gated (`scematica`) so the published core never depends on the bot.
//! The x402-settled bond engine lives in the separate, unpublished
//! `scemadex-integrations` crate because its `scematica-protocol` dependency
//! pulls the proprietary `scematica-core` stack.

mod dq_policy;
pub use dq_policy::{DqRoutePolicy, PoolTelemetry};
