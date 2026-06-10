//! The Scar Market (Primitive F): selling slash-certified failure data. A
//! slashed bond is the only un-fakeable proof a decision cost real collateral —
//! so the trajectory behind it is verified negative knowledge.
//!
//!   cargo run -p scemadex-sdk --example scar_market
//!
//! An agent underdelivers, its bond slashes, and the loss becomes inventory:
//! a certified lesson other agents pay for to train veto heads against.

use scemadex_sdk::{
    certify_scar, demo_intent, Amount, BondEngine, BondOutcome, EscrowBondEngine, Fill,
    LocalScarMarket, ReferenceRoutePolicy, RoutePolicy, ScarMarket, Usdc,
};

#[tokio::main]
async fn main() -> scemadex_sdk::Result<()> {
    let engine = EscrowBondEngine::with_defaults();
    let policy = ReferenceRoutePolicy;
    let market = LocalScarMarket::new();

    // An agent bonds an inference, underdelivers, and gets slashed.
    let solution = policy.solve(&demo_intent()).await?;
    let bond = engine.escrow(&solution).await?;
    let bad_fill = Fill {
        amount_out: Amount::new(bond.min_out_raw / 2, 6),
        executed_unix: 0,
    };
    let outcome = engine.settle(&bond, &bad_fill).await?;
    println!("bond settled -> {outcome:?} ({} micro-USDC lost)", bond.amount.0);

    // A scar can only be minted from that slash; an honored bond is refused.
    let trajectory = vec![0u8; 64]; // model-agnostic (state, action, reward, ...) encoding
    let scar = certify_scar(&bond, outcome, "agent-icarus", 12, trajectory, Usdc::from_usdc(0.25))?;
    println!(
        "certified scar: {} transitions, {} micro-USDC collateral behind it",
        scar.transitions, scar.slashed_collateral.0
    );
    assert!(
        certify_scar(&bond, BondOutcome::Honored, "forger", 1, vec![], Usdc(1)).is_err(),
        "honored outcomes must never certify"
    );
    println!("forged (honored) scar rejected -> only real losses sell here");

    // The loss becomes inventory. Buyers pick maximum certified pain per dollar.
    market.sell_scar(scar).await?;
    println!("\nlisted {} scar(s)", market.scar_count());
    let lesson = market.buy_scar(Usdc::from_usdc(1.0)).await?;
    println!(
        "bought scar from {} for {} micro-USDC ({:.0}x collateral per price)",
        lesson.peer_id,
        lesson.price.0,
        lesson.collateral_per_price()
    );
    println!("-> feed it to a veto head as a verified negative example");

    Ok(())
}
