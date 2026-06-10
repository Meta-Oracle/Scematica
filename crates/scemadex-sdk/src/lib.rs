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
//! ## The adversarial layer: four primitives with no precedent
//!
//! - **E · The Counter-Market.** Any agent can stake *against* an open bond
//!   ([`counter::CounterMarket`]) — the first market that prices individual
//!   machine inferences. The gap between self-conviction and market-implied
//!   conviction (the *doubt spread*) is a brand-new policy feature.
//! - **F · The Scar Market.** Slashed bonds are the only un-fakeable proof a
//!   decision cost real collateral; [`scar::certify_scar`] turns the trajectory
//!   behind a slash into sellable, verified negative training data.
//! - **G · Experience royalties.** [`lineage::LineageLedger`] records which
//!   purchased experience trained which policy, and streams a royalty slice of
//!   every downstream fee back to the sellers — training data as a
//!   yield-bearing asset.
//! - **H · Bonded teaching.** A teacher answers a student's most uncertain
//!   states per metered query, bonding its tuition against the student's
//!   *measured* improvement ([`teach::TeachingEngine`]) — distillation with a
//!   money-back guarantee.
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
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod bond;
pub mod client;
pub mod counter;
pub mod error;
pub mod intent;
pub mod lineage;
pub mod mesh;
pub mod oracle;
pub mod policy;
pub mod primitives;
pub mod route;
pub mod scar;
pub mod teach;
pub mod venue;

/// Concrete wiring of the SDK traits to the real Scematica Deep Q* agent. Only
/// compiled with the `scematica` feature; the default published crate carries no
/// such dependency.
#[cfg(feature = "scematica")]
#[cfg_attr(docsrs, doc(cfg(feature = "scematica")))]
pub mod integration;

/// Natural-language intent parsing + route/bond narration via an LLM. Requires
/// the `ai` feature.
#[cfg(feature = "ai")]
#[cfg_attr(docsrs, doc(cfg(feature = "ai")))]
pub mod ai;

/// Networked [`PeerMarket`] client over an HTTP/JSON relay. Requires the `net`
/// feature.
#[cfg(feature = "net")]
#[cfg_attr(docsrs, doc(cfg(feature = "net")))]
pub mod net;

pub use bond::{
    Bond, BondConfig, BondEngine, BondLedger, BondOutcome, EscrowBondEngine, NoBondEngine,
};
pub use client::ScemaDex;
pub use counter::{Challenge, ChallengeSettlement, CounterMarket, CounterStats};
pub use error::{Result, ScemaDexError};
pub use intent::{Constraints, Intent, Objective, Side};
pub use lineage::{LineageEntry, LineageLedger, RoyaltySplit};
pub use mesh::{ExperienceBatch, InferenceOffer, LocalPeerMarket, PeerMarket};
pub use oracle::{Advice, PoolScore, Reputation, SignalSource};
pub use policy::{Conviction, ReferenceRoutePolicy, RoutePolicy, Solution};
pub use primitives::{Address, Amount, Usdc};
pub use route::{Fill, Route, RouteLeg, Venue};
pub use scar::{certify_scar, LocalScarMarket, ScarMarket, ScarRecord};
pub use teach::{
    ReferenceTeacher, TeachAnswer, TeachReceipt, TeachTerms, Teacher, TeachingEngine,
};
pub use venue::{SimVenueExecutor, SwapInstructions, VenueExecutor};

/// Build a [`ScemaDex`] wired with the lean reference implementations. Useful for
/// examples, tests, and consumers who haven't enabled the `scematica` feature.
///
/// Note the bond engine here is [`NoBondEngine`], which escrows a *zero* bond —
/// it exercises the intent/route surface but does **not** demonstrate Conviction
/// Routing (Primitive D). For the conviction-weighted, ledger-tracking bond
/// engine, use [`conviction_client`].
pub fn reference_client() -> ScemaDex<ReferenceRoutePolicy, NoBondEngine, SimVenueExecutor> {
    ScemaDex::new(ReferenceRoutePolicy, NoBondEngine, SimVenueExecutor)
}

/// Build a [`ScemaDex`] whose bond engine is the conviction-weighted
/// [`EscrowBondEngine`] — the lean reference wiring that actually exercises
/// **Conviction Routing** (Primitive D) end-to-end, offline.
///
/// Unlike [`reference_client`], each [`ScemaDex::quote`] here escrows a real,
/// conviction-sized bond with a guaranteed-minimum-output haircut, and
/// [`ScemaDex::execute`] settles it (honored or slashed) into the engine's
/// [`BondLedger`]. The only thing missing versus the `scematica` feature is the
/// on-chain x402 USDC transfer; the settlement state machine is identical.
///
/// ```
/// use scemadex_sdk::{conviction_client, demo_intent, BondOutcome};
/// # async fn run() -> scemadex_sdk::Result<()> {
/// let dex = conviction_client();
/// let (solution, bond) = dex.quote(&demo_intent()).await?;
/// assert!(bond.amount.0 > 0, "conviction-weighted bond is escrowed");
/// assert!(bond.min_out_raw <= solution.route.expected_out.raw);
/// let (_fill, outcome) = dex.execute(&demo_intent()).await?;
/// assert_eq!(outcome, BondOutcome::Honored);
/// # Ok(()) }
/// ```
pub fn conviction_client() -> ScemaDex<ReferenceRoutePolicy, EscrowBondEngine, SimVenueExecutor> {
    ScemaDex::new(
        ReferenceRoutePolicy,
        EscrowBondEngine::with_defaults(),
        SimVenueExecutor,
    )
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
