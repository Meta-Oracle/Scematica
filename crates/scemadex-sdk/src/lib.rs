//! # ScemaDEX SDK — an Agentic Liquidity Layer
//!
//! ScemaDEX is not "a DEX." It is an SDK in which **the routing intelligence
//! itself is a metered, learning, accountable product.** Autonomous agents
//! solve swap *intents*, **bond** their promises, sell their inferences, and
//! trade learned experience with one another — all settled in stablecoins over
//! the x402 payment rails.
//!
//! ## The four composing primitives
//!
//! - **A · Metered inference routing.** Every quote is produced by a learning
//!   policy and can be sold per-call via x402. The *quality of the decision* is
//!   the SKU. See [`policy`].
//! - **B · Intent solving.** Callers express *what they want*
//!   ([`intent::Intent`] with an [`intent::Objective`]), not a path spec. The
//!   policy decides venue / split / timing.
//! - **C · Signal & reputation oracle.** Reputation, pool scores and advice are
//!   monetized read endpoints. See [`oracle`].
//! - **D · Conviction Routing (the defining pillar).** The policy escrows a
//!   slashable performance **bond** against its own promise. Meet the
//!   guarantee → reclaim the bond and collect the fee; miss it → the bond
//!   settles to the caller. This is what makes a paid black-box inference
//!   *trustworthy*. See [`bond`].
//!
//! ## The headline: a mesh of agents trading intelligence
//!
//! Compose A–D across many nodes and you get the revolutionary endgame: a
//! [`mesh::PeerMarket`] where agents **sell bonded inferences and learned
//! experience, and buy better ones from peers** — an economy of machine
//! intelligence, not a swap widget. An agent earns USDC selling what it knows
//! and spends USDC to learn faster.
//!
//! ## Architecture: lean core, injected power
//!
//! The published crate carries **no `solana-sdk` or bot dependency**. It defines
//! the trait surface ([`policy::RoutePolicy`], [`bond::BondEngine`],
//! [`venue::VenueExecutor`], [`oracle::SignalSource`], [`mesh::PeerMarket`]) plus
//! reference implementations. Enabling the `scematica` feature injects the real
//! Deep Q* agent, the Raydium/Orca/Meteora/Jupiter executors, and the x402
//! facilitator from the bot workspace.
//!
//! ```
//! use scemadex_sdk::reference_client;
//! # async fn run() -> scemadex_sdk::Result<()> {
//! let dex = reference_client();
//! let intent = scemadex_sdk::demo_intent();
//! let (solution, bond) = dex.quote(&intent).await?;
//! assert!(solution.route.splits_valid());
//! let _ = bond;
//! # Ok(()) }
//! ```

pub mod bond;
pub mod client;
pub mod error;
pub mod intent;
pub mod mesh;
pub mod oracle;
pub mod policy;
pub mod primitives;
pub mod route;
pub mod venue;

/// Concrete wiring of the SDK traits to the real Scematica Deep Q* agent. Only
/// compiled with the `scematica` feature; the default published crate carries no
/// such dependency.
#[cfg(feature = "scematica")]
pub mod integration;

/// Natural-language intent parsing + route/bond narration via an LLM. Requires
/// the `ai` feature.
#[cfg(feature = "ai")]
pub mod ai;

/// Networked [`PeerMarket`] client over an HTTP/JSON relay. Requires the `net`
/// feature.
#[cfg(feature = "net")]
pub mod net;

pub use bond::{
    Bond, BondConfig, BondEngine, BondLedger, BondOutcome, EscrowBondEngine, NoBondEngine,
};
pub use client::ScemaDex;
pub use error::{Result, ScemaDexError};
pub use intent::{Constraints, Intent, Objective, Side};
pub use mesh::{ExperienceBatch, InferenceOffer, LocalPeerMarket, PeerMarket};
pub use oracle::{Advice, PoolScore, Reputation, SignalSource};
pub use policy::{Conviction, ReferenceRoutePolicy, RoutePolicy, Solution};
pub use primitives::{Address, Amount, Usdc};
pub use route::{Fill, Route, RouteLeg, Venue};
pub use venue::{SimVenueExecutor, SwapInstructions, VenueExecutor};

/// Build a [`ScemaDex`] wired with the lean reference implementations. Useful for
/// examples, tests, and consumers who haven't enabled the `scematica` feature.
pub fn reference_client() -> ScemaDex<ReferenceRoutePolicy, NoBondEngine, SimVenueExecutor> {
    ScemaDex::new(ReferenceRoutePolicy, NoBondEngine, SimVenueExecutor)
}

/// A canonical WSOL → USDC demo intent used by docs and tests.
pub fn demo_intent() -> Intent {
    Intent {
        input_mint: Address::new("So11111111111111111111111111111111111111112")
            .expect("valid WSOL mint"),
        output_mint: Address::new("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")
            .expect("valid USDC mint"),
        amount_in: Amount::new(1_000_000_000, 9), // 1 SOL
        side: Side::Sell,
        objective: Objective::Price,
        constraints: Constraints {
            max_slippage_bps: 150,
            deadline_unix: 0,
            max_legs: Some(3),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn quote_produces_valid_bonded_solution() {
        let dex = reference_client();
        let (solution, bond) = dex.quote(&demo_intent()).await.unwrap();
        assert!(solution.route.splits_valid(), "splits must sum to 10_000 bps");
        assert!((0.0..=1.0).contains(&solution.conviction.0));
        assert_eq!(bond.intent_digest, solution.intent_digest);
    }

    #[tokio::test]
    async fn execute_honors_bond_when_fill_meets_promise() {
        let dex = reference_client();
        let (fill, outcome) = dex.execute(&demo_intent()).await.unwrap();
        assert!(fill.amount_out.raw > 0);
        assert_eq!(outcome, BondOutcome::Honored);
    }

    #[test]
    fn intent_digest_is_stable() {
        let a = demo_intent().digest();
        let b = demo_intent().digest();
        assert_eq!(a, b);
    }
}
