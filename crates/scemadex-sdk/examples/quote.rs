//! Quickstart: solve an intent into a bonded solution, then execute it.
//!
//!   cargo run -p scemadex-sdk --example quote
//!
//! Demonstrates Primitives A (metered inference routing) and B (intent solving)
//! with the lean reference wiring — no network, no keypair, no `solana-sdk`.

use scemadex_sdk::{demo_intent, reference_client};

#[tokio::main]
async fn main() -> scemadex_sdk::Result<()> {
    let dex = reference_client();
    let intent = demo_intent();

    println!(
        "intent: {} {:?} {} -> {}  (objective {:?})",
        intent.amount_in.ui(),
        intent.side,
        intent.input_mint,
        intent.output_mint,
        intent.objective,
    );

    // Primitive A + B: the policy turns "what I want" into a routed solution and
    // the engine escrows a bond against it. This (solution, bond) pair is the
    // unit other agents pay for in the mesh.
    let (solution, bond) = dex.quote(&intent).await?;
    println!("\nsolution (digest {})", solution.intent_digest);
    println!("  conviction : {:.2}", solution.conviction.0);
    println!("  rationale  : {}", solution.rationale);
    println!("  splits ok  : {}", solution.route.splits_valid());
    for leg in &solution.route.legs {
        println!(
            "  leg        : {:?} {} bps -> ~{} out",
            leg.venue,
            leg.split_bps,
            leg.expected_out.ui(),
        );
    }
    println!("  bond       : {} micro-USDC escrowed", bond.amount.0);

    // Full path: solve -> bond -> execute -> settle.
    let (fill, outcome) = dex.execute(&intent).await?;
    println!("\nexecuted: out ~{}  bond {:?}", fill.amount_out.ui(), outcome);

    Ok(())
}
