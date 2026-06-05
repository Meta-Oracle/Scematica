//! Intent solving (Primitive B): express *what* you want, not *how* to route it.
//!
//!   cargo run -p scemadex-sdk --example intent_solving
//!
//! The same trade is submitted under each `Objective` — Price, Speed, Stealth —
//! to show that the caller never specifies a path. A real (`scematica`-feature)
//! policy weights its venue/split/timing search by the objective; the reference
//! policy just routes single-venue so the surface stays dependency-free.

use scemadex_sdk::{
    conviction_client, Address, Amount, Constraints, Intent, Objective, Side,
};

fn trade(objective: Objective) -> Intent {
    Intent {
        // 0.5 SOL -> USDC
        input_mint: Address::new("So11111111111111111111111111111111111111112").unwrap(),
        output_mint: Address::new("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(),
        amount_in: Amount::new(500_000_000, 9),
        side: Side::Sell,
        objective,
        constraints: Constraints {
            max_slippage_bps: 100,
            deadline_unix: 0,
            max_legs: Some(3),
        },
    }
}

#[tokio::main]
async fn main() -> scemadex_sdk::Result<()> {
    let dex = conviction_client();

    for objective in [Objective::Price, Objective::Speed, Objective::Stealth] {
        let intent = trade(objective);
        let (solution, bond) = dex.quote(&intent).await?;
        println!(
            "{:?}: {} leg(s), conviction {:.2}, bond {} micro-USDC -> {}",
            objective,
            solution.route.legs.len(),
            solution.conviction.0,
            bond.amount.0,
            solution.rationale,
        );
    }

    Ok(())
}
