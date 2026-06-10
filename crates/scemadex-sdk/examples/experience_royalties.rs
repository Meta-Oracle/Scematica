//! Experience royalties (Primitive G): training data as a yield-bearing asset.
//! Buy experience from the mesh, train on it, and a slice of every fee your
//! policy later earns streams back up the lineage to the sellers.
//!
//!   cargo run -p scemadex-sdk --example experience_royalties
//!
//! Sellers stop dumping junk transitions for one-shot fees — a successful
//! student is an annuity, so only your best experience is worth listing.

use scemadex_sdk::{
    ExperienceBatch, LineageLedger, LocalPeerMarket, PeerMarket, Usdc,
};

#[tokio::main]
async fn main() -> scemadex_sdk::Result<()> {
    let market = LocalPeerMarket::new();
    let lineage = LineageLedger::new();

    // Two veteran agents list the experience they lived through.
    for (seller, transitions, price) in
        [("agent-alpha", 7_500u32, 0.5), ("agent-beta", 2_500u32, 0.2)]
    {
        market
            .sell_experience(ExperienceBatch {
                peer_id: seller.into(),
                transitions,
                price: Usdc::from_usdc(price),
                payload: vec![0u8; 32],
            })
            .await?;
    }

    // A young agent buys both batches and records the provenance as it trains.
    for _ in 0..2 {
        let batch = market.buy_experience(Usdc::from_usdc(1.0)).await?;
        println!(
            "student trained on {} transitions from {} (batch {})",
            batch.transitions,
            batch.peer_id,
            batch.digest()
        );
        lineage.record_training("student", &batch)?;
    }
    println!("policy lineage root: {}", lineage.lineage_root("student"));

    // The student's policy starts earning inference fees. 10% of each fee is
    // the royalty pool, split pro-rata by contributed transitions.
    println!("\nstudent earns fees; royalties stream up the lineage (10%):");
    for fee_usdc in [1.0, 1.0, 2.0] {
        let split = lineage.distribute("student", Usdc::from_usdc(fee_usdc), 1_000)?;
        println!(
            "  fee {:.2} USDC -> student keeps {:.4}, royalties {:?}",
            fee_usdc,
            split.student_keeps.as_usdc(),
            split
                .payouts
                .iter()
                .map(|(s, p)| format!("{s}: {:.4}", p.as_usdc()))
                .collect::<Vec<_>>()
        );
    }

    println!(
        "\ncumulative dividends: agent-alpha {:.4} USDC, agent-beta {:.4} USDC",
        lineage.royalties_earned("agent-alpha").as_usdc(),
        lineage.royalties_earned("agent-beta").as_usdc()
    );
    println!("-> selling your best experience beats selling junk: students that win pay forever");

    Ok(())
}
