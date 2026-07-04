//! Optimistic dispute settlement: the Counter-Market (E) wired into finality,
//! feeding the Scar Market (F).
//!
//!   cargo run -p scemadex-sdk --example dispute_settlement
//!
//! A bond no longer settles the instant a fill lands. It provisionally honors,
//! opens a dispute window, and only *finalizes* once the window elapses or a
//! challenge is adjudicated. Here a challenger is right: the upheld challenge
//! flips the bond to Slashed, the slashed collateral splits four ways, the
//! challenger is paid its routed slice, and the proven loss mints a scar.

use scemadex_sdk::{
    demo_intent, BondEngine, BondOutcome, DisputeCoordinator, EscrowBondEngine, ManualClock,
    ReferenceRoutePolicy, RoutePolicy, SettlementConfig, SlashRouting, Usdc,
};

#[tokio::main]
async fn main() -> scemadex_sdk::Result<()> {
    // 5-minute dispute window; a slash splits 50% to the wronged caller, 30% to
    // the challengers who caught it, 15% to a reinsurance pool, 5% up the lineage.
    let config = SettlementConfig::optimistic(300).with_slash_routing(SlashRouting {
        to_caller_bps: 5_000,
        to_challengers_bps: 3_000,
        to_insurance_bps: 1_500,
        to_lineage_bps: 500,
    });
    let clock = ManualClock::new(1_700_000_000);
    let coord = DisputeCoordinator::new(config, clock);

    // An agent escrows a conviction-weighted bond on its inference.
    let engine = EscrowBondEngine::with_defaults();
    let solution = ReferenceRoutePolicy.solve(&demo_intent()).await?;
    let bond = engine.escrow(&solution).await?;
    coord.open(&bond, solution.conviction)?;
    println!(
        "opened bond {} micro-USDC @ self-conviction {:.2}  (window 300s)",
        bond.amount.0, solution.conviction.0
    );

    // The fill lands and provisionally honors — money does NOT move yet.
    coord.provision(&bond.intent_digest, BondOutcome::Honored)?;
    println!("provisional: Honored (dispute window open)");

    // A skeptic re-executes the route, smells a bad fill, and stakes against it.
    coord.challenge(&bond.intent_digest, "skeptic", Usdc::from_usdc(1.0))?;
    println!(
        "challenged -> market conviction {:.2}, doubt spread {:+.2} (now Disputed)",
        coord.counter().market_conviction(&bond.intent_digest).unwrap(),
        coord.counter().doubt_spread(&bond.intent_digest).unwrap(),
    );

    // Adjudication (a re-check / oracle / proof) confirms the fill missed: the
    // challenge is upheld and finality flips to Slashed.
    let report = coord.resolve(&bond.intent_digest, BondOutcome::Slashed)?;
    println!("\nfinalized: {:?} via {:?}", report.outcome, report.reason);
    if let Some(slash) = report.slash {
        println!(
            "  slash split -> caller {}  challengers {}  insurance {}  lineage {}",
            slash.caller.0, slash.challengers.0, slash.insurance.0, slash.lineage.0
        );
    }
    if let Some(ref cs) = report.challenge {
        for (who, payout) in &cs.challenger_payouts {
            println!("  challenger {who} paid {} micro-USDC (stake + slice)", payout.0);
        }
    }

    // The proven loss is the only un-fakeable training signal — mint a scar.
    if report.is_adversarial_slash() {
        let scar = coord.mint_scar(
            &bond.intent_digest,
            "agent-that-lost",
            42,               // transitions in the trajectory
            vec![0xDE, 0xAD], // model-agnostic (state, action, reward) payload
            Usdc::from_usdc(0.25),
        )?;
        println!(
            "\nscar minted: {} micro-USDC of certified pain across {} transitions, asking {}",
            scar.slashed_collateral.0, scar.transitions, scar.price.0
        );
    }

    let stats = coord.counter().stats();
    println!(
        "\ncounter-market: {} settled, {} challenger wins",
        stats.settled, stats.challenger_wins
    );
    Ok(())
}
