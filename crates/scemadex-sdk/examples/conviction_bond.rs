//! Conviction Routing (Primitive D): the policy escrows a slashable bond against
//! its own promise, sized by its conviction. Meet the guaranteed minimum output
//! and the bond is honored; miss it and the bond is slashed to the caller.
//!
//!   cargo run -p scemadex-sdk --example conviction_bond
//!
//! This drives the `EscrowBondEngine` directly so we can show both a honored and
//! a slashed settlement and watch the on-chain-shaped ledger update — the raw
//! material the reputation oracle (Primitive C) sells.

use scemadex_sdk::{
    demo_intent, Amount, BondEngine, Conviction, EscrowBondEngine, Fill, ReferenceRoutePolicy,
    RoutePolicy,
};

#[tokio::main]
async fn main() -> scemadex_sdk::Result<()> {
    let engine = EscrowBondEngine::with_defaults();
    let policy = ReferenceRoutePolicy;

    // A higher-conviction solution escrows a larger bond and charges a larger
    // inference fee — confidence is *priced*.
    let solution = policy.solve(&demo_intent()).await?;
    let bond = engine.escrow(&solution).await?;
    println!("conviction {:.2}", solution.conviction.0);
    println!("  fee charged : {} micro-USDC", engine.quote_fee(solution.conviction).0);
    println!("  bond escrow : {} micro-USDC", bond.amount.0);
    println!("  guaranteed  : >= {} base units out", bond.min_out_raw);
    println!("  open bonds  : {}", engine.open_bonds());

    // Case 1 — the fill meets the guarantee: bond honored, agent reclaims it.
    let good_fill = Fill {
        amount_out: Amount::new(bond.min_out_raw, 6),
        executed_unix: 0,
    };
    let outcome = engine.settle(&bond, &good_fill).await?;
    println!("\nfill meets guarantee -> {outcome:?}");

    // Case 2 — a worse, lower-conviction solution that underdelivers: slashed.
    let weak = {
        let mut s = policy.solve(&demo_intent()).await?;
        s.conviction = Conviction::clamped(0.3);
        s.intent_digest = format!("{}-weak", s.intent_digest); // distinct bond key
        s
    };
    let weak_bond = engine.escrow(&weak).await?;
    let bad_fill = Fill {
        amount_out: Amount::new(weak_bond.min_out_raw / 2, 6),
        executed_unix: 0,
    };
    let outcome = engine.settle(&weak_bond, &bad_fill).await?;
    println!("fill misses guarantee -> {outcome:?}");

    let ledger = engine.ledger();
    println!(
        "\nledger: {} honored / {} slashed  (honor rate {:.0}%)",
        ledger.honored,
        ledger.slashed,
        ledger.honor_rate() * 100.0,
    );

    Ok(())
}
