//! # scema-policy — deciding, and declining to decide
//!
//! Three things, in order of how much they matter:
//!
//! 1. [`utility`] — the risk-adjusted equation `U = R − λ₁K − λ₂C − λ₃U + λ₄V`, additive
//!    so that an unmeasured term is silent rather than fatal.
//! 2. [`evaluator`] — the [`Evaluator`] trait, whose defining feature is
//!    [`Applicability`]: a specialist must be able to say "not my domain" and "my domain,
//!    but I lack the inputs" as *different* answers.
//! 3. [`decide`] — ranking, and the five ways it can refuse to pick anything.
//!
//! [`render`] is here too, and deliberately: it is the only place in Rust where a
//! [`scema_world::Term`] becomes a string, and its single rule — an unmeasured term prints
//! `—`, never `0.00` — is the last line of defence for everything the type system below has
//! been protecting. A rule that encodes a claim about trust gets one implementation.
//!
//! The Deep Q* agent from `scematica-nn` is wired in at [`dqstar`] as one evaluator among
//! several, and on a software world it declines. That is the whole relationship between
//! this runtime and the trading bot it grew out of: the DQN is a specialist inside a larger
//! loop, not the loop.

pub mod decide;
pub mod evaluator;
pub mod render;
pub mod utility;

#[cfg(feature = "dqstar")]
pub mod dqstar;

pub use decide::{decide, Abstention, Decision, DecisionConfig, EvaluatorStatus, Excluded, Ranked};
pub use evaluator::{Applicability, Evaluation, Evaluator};
pub use utility::{Contribution, Utility, UtilityWeights};
