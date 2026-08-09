//! # scematica-suite
//!
//! The Scematica **meta-crate**: a single dependency that re-exports the entire
//! Solana sniper + cross-DEX arb + AI + Deep Q\* + x402 + ScemaDEX + cognitive
//! stack, so downstream code can pull the whole thing with one line:
//!
//! ```toml
//! scematica-suite = "1"
//! ```
//!
//! ```no_run
//! use scematica_suite::{core, nn, scemadex};
//!
//! let agent = nn::DQNAgent::default();
//! let dex = scemadex::reference_client();
//! let _ = (agent, dex, core::metrics::METRICS_FILE);
//! ```
//!
//! It also installs a `scematica` launcher binary that dispatches to the
//! component executables (`dashboard`, `sniper`, `protocol`, `scema-ddqn`,
//! `scemadex`, …) — see the crate README.
//!
//! Each component remains independently usable; this crate is purely a
//! convenience aggregator and adds no logic of its own.

#[doc(inline)]
pub use scematica_core as core;
#[doc(inline)]
pub use scematica_executor as executor;
#[doc(inline)]
pub use scematica_protocol as protocol;
#[doc(inline)]
pub use scematica_ai as ai;
#[doc(inline)]
pub use scematica_nn as nn;
#[doc(inline)]
pub use scematica_sniper as sniper;
#[doc(inline)]
pub use scematica_dashboard as dashboard;
#[doc(inline)]
pub use scemadex_sdk as scemadex;
/// Cognitive architecture — library only; it ships no binary, so the `scematica`
/// launcher has no `sentience` subcommand to dispatch to.
#[doc(inline)]
pub use scematica_sentience as sentience;

/// The version of the suite meta-crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
