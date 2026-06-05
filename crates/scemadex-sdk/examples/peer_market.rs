//! The headline primitive: a mesh where autonomous agents **trade intelligence**.
//!
//!   cargo run -p scemadex-sdk --example peer_market
//!
//! Two things change hands here, both settled in USDC over x402:
//!   * bonded *inferences* — "here is a solved route, and I've bonded it"
//!   * batches of *experience* — RL transitions a peer sells so others learn faster
//!
//! This uses the in-process `LocalPeerMarket`; the networked `RemotePeerMarket`
//! (the `net` feature) slots in behind the same trait, so this code is unchanged.

use scemadex_sdk::{
    conviction_client, demo_intent, ExperienceBatch, InferenceOffer, LocalPeerMarket, PeerMarket,
    Usdc,
};

#[tokio::main]
async fn main() -> scemadex_sdk::Result<()> {
    let market = LocalPeerMarket::new();
    let intent = demo_intent();

    // A seller agent solves the intent and lists its bonded inference for sale.
    let seller = conviction_client();
    let (solution, _bond) = seller.quote(&intent).await?;
    market
        .sell_inference(InferenceOffer {
            solution: solution.clone(),
            price: Usdc::from_usdc(0.05),
            peer_id: "agent-alpha".into(),
        })
        .await?;
    // A competing, cheaper offer for the same intent.
    market
        .sell_inference(InferenceOffer {
            solution,
            price: Usdc::from_usdc(0.02),
            peer_id: "agent-beta".into(),
        })
        .await?;
    println!("listed {} inference offer(s)", market.offer_count());

    // A buyer agent purchases the best (cheapest matching) inference.
    let bought = market.buy_inference(&intent).await?;
    println!(
        "bought inference from {} for {} USDC (conviction {:.2})",
        bought.peer_id,
        bought.price.as_usdc(),
        bought.solution.conviction.0,
    );
    println!("remaining offers: {}", market.offer_count());

    // Experience market: sell a batch of learned transitions, then buy it under
    // a price cap.
    market
        .sell_experience(ExperienceBatch {
            peer_id: "agent-alpha".into(),
            transitions: 10_000,
            price: Usdc::from_usdc(0.50),
            payload: vec![/* opaque (state, action, reward, next_state) encoding */],
        })
        .await?;
    let batch = market.buy_experience(Usdc::from_usdc(1.0)).await?;
    println!(
        "\nbought {} transitions from {} for {} USDC",
        batch.transitions,
        batch.peer_id,
        batch.price.as_usdc(),
    );

    Ok(())
}
