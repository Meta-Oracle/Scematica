//! The bot's brain pricing a ScemaDEX bond.
//!
//! This closes the loop between the two product lines: the **trained Deep Q\***
//! agent (`scematica-nn`) supplies the **conviction** that sizes a
//! Conviction-Routing bond against a **real Jupiter quote**, and the realized
//! fill feeds back into the agent. The conviction is produced by the DQ* value
//! head — not a stub — so a forked trait surface can't reproduce it without the
//! weights.
//!
//!   # uses the bot's live checkpoint if present, else a fresh (untrained) agent
//!   SCEMATICA_NN_CHECKPOINT=scematica-nn-agent.json \
//!     cargo run -p scemadex-integrations --example agent_conviction
//!
//! Requires network access (Jupiter v6 quote). Not run in CI.

use scemadex_integrations::jupiter::JupiterRoutePolicy;
use scemadex_sdk::{
    demo_intent, Amount, BondEngine, BondOutcome, EscrowBondEngine, Fill, RoutePolicy,
};
use scematica_nn::DQNAgent;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Load the bot's trained agent (checkpoint written every 10 min by the
    //    sniper), or fall back to a fresh agent so the example still runs.
    let ckpt = std::env::var("SCEMATICA_NN_CHECKPOINT")
        .unwrap_or_else(|_| "scematica-nn-agent.json".to_string());
    let (agent, source) = match DQNAgent::load(&ckpt) {
        Ok(a) => (a, format!("trained checkpoint `{ckpt}`")),
        Err(_) => (
            DQNAgent::new(),
            "fresh (untrained) agent — set SCEMATICA_NN_CHECKPOINT for real weights".to_string(),
        ),
    };
    println!("agent: {source}");

    // 2. Agent-backed routing policy: conviction now comes from the DQ* value
    //    head (discounted by the quote's price impact), not the 0.6 stub.
    let policy = JupiterRoutePolicy::new().with_agent(agent);
    let intent = demo_intent();

    // 3. Real Jupiter quote → solution whose conviction is the agent's.
    let solution = policy.solve(&intent).await?;
    println!("\n── solution (real Jupiter quote, agent conviction) ──");
    println!(
        "conviction : {:.3}   (from the DQ* value head)",
        solution.conviction.0
    );
    println!("rationale  : {}", solution.rationale);
    println!(
        "expected   : {} base units out",
        solution.route.expected_out.raw
    );

    // 4. The bond — and the inference fee — are sized by that conviction.
    let engine = EscrowBondEngine::with_defaults();
    let bond = engine.escrow(&solution).await?;
    println!("\n── bond (conviction-weighted) ──");
    println!("bond       : {} µUSDC escrowed", bond.amount.0);
    println!(
        "fee        : {} µUSDC (priced by conviction)",
        engine.quote_fee(solution.conviction).0
    );
    println!("guarantee  : ≥ {} base units out", bond.min_out_raw);

    // 5. Close the reinforcement loop: realized fill vs. the bonded promise.
    //    A Jupiter fill at the quoted amount honors the bond; observe_outcome
    //    rewards the agent (or penalises it on a slash) from the realized result.
    let fill_out = solution.route.expected_out.raw;
    let fill = Fill {
        amount_out: Amount::new(fill_out, 0),
        executed_unix: 0,
    };
    let outcome = engine.settle(&bond, &fill).await?;
    policy.observe_outcome(
        &intent,
        solution.route.expected_out.raw,
        fill_out,
        outcome == BondOutcome::Slashed,
    );
    println!("\noutcome    : {outcome:?} — fed back to the agent (RL loop closed)");
    Ok(())
}
