//! The Counter-Market (Primitive E): adversarial conviction staking. Any agent
//! can stake *against* an open bond — the first market that prices individual
//! machine inferences, not events.
//!
//!   cargo run -p scemadex-sdk --example counter_market
//!
//! Honored → challengers forfeit stakes to the agent (a premium for being
//! doubted and right). Slashed → challengers split the bond pro-rata. The gap
//! between self-conviction and market-implied conviction (the doubt spread) is
//! a brand-new policy feature.

use scemadex_sdk::{
    demo_intent, Amount, BondEngine, CounterMarket, EscrowBondEngine, Fill,
    ReferenceRoutePolicy, RoutePolicy, Usdc,
};

#[tokio::main]
async fn main() -> scemadex_sdk::Result<()> {
    let engine = EscrowBondEngine::with_defaults();
    let policy = ReferenceRoutePolicy;
    let counter = CounterMarket::new();

    // An agent quotes and bonds an inference, then lists it on the counter-market.
    let solution = policy.solve(&demo_intent()).await?;
    let bond = engine.escrow(&solution).await?;
    counter.list(&bond, solution.conviction)?;
    println!(
        "listed bond {} micro-USDC at self-conviction {:.2}",
        bond.amount.0, solution.conviction.0
    );

    // Two peers smell overconfidence and stake against it.
    counter.challenge(&bond.intent_digest, "skeptic-alpha", Usdc::from_usdc(0.5))?;
    counter.challenge(&bond.intent_digest, "skeptic-beta", Usdc::from_usdc(1.5))?;
    println!(
        "challenged with {} micro-USDC total stake",
        counter.total_stake(&bond.intent_digest).0
    );
    println!(
        "market conviction {:.2}  (self {:.2})  -> doubt spread {:+.2}",
        counter.market_conviction(&bond.intent_digest).unwrap(),
        solution.conviction.0,
        counter.doubt_spread(&bond.intent_digest).unwrap(),
    );

    // Case 1 — the fill honors the bond: the doubters pay the agent a premium.
    let good_fill = Fill {
        amount_out: Amount::new(bond.min_out_raw, 6),
        executed_unix: 0,
    };
    let outcome = engine.settle(&bond, &good_fill).await?;
    let settlement = counter.settle(&bond.intent_digest, outcome)?;
    println!(
        "\nbond {:?} -> agent premium {} micro-USDC from forfeited stakes",
        outcome, settlement.agent_premium.0
    );

    // Case 2 — a second, weaker inference gets challenged and slashed: the
    // challengers reclaim their stakes plus the bond, pro-rata.
    let weak = {
        let mut s = policy.solve(&demo_intent()).await?;
        s.intent_digest = format!("{}-weak", s.intent_digest);
        s
    };
    let weak_bond = engine.escrow(&weak).await?;
    counter.list(&weak_bond, weak.conviction)?;
    counter.challenge(&weak_bond.intent_digest, "skeptic-alpha", Usdc::from_usdc(1.0))?;
    counter.challenge(&weak_bond.intent_digest, "skeptic-beta", Usdc::from_usdc(3.0))?;
    let bad_fill = Fill {
        amount_out: Amount::new(weak_bond.min_out_raw / 2, 6),
        executed_unix: 0,
    };
    let outcome = engine.settle(&weak_bond, &bad_fill).await?;
    let settlement = counter.settle(&weak_bond.intent_digest, outcome)?;
    println!("\nbond {:?} -> challenger payouts:", outcome);
    for (who, payout) in &settlement.challenger_payouts {
        println!("  {who}: {} micro-USDC (stake + bond share)", payout.0);
    }

    let stats = counter.stats();
    println!(
        "\ncounter-market: {} settled, {} agent wins / {} challenger wins",
        stats.settled, stats.agent_wins, stats.challenger_wins
    );
    Ok(())
}
