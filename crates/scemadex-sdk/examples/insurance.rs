//! Bond insurance / reinsurance (Primitive J): the hedge side of Conviction Routing.
//!
//!   cargo run -p scemadex-sdk --example insurance
//!
//! An agent bonds an inference and buys coverage priced off its reputation. When a
//! bond slashes, the insurance slice of the slash recapitalizes the pool and the
//! insured agent is paid its coverage — a lumpy tail loss becomes a small premium.
//! When a bond honors, the pool keeps the premium as yield.

use scemadex_sdk::{
    demo_intent, BondEngine, BondOutcome, Conviction, DisputeCoordinator, EscrowBondEngine,
    InsurancePool, ManualClock, PremiumConfig, Reputation, ReferenceRoutePolicy, RoutePolicy,
    SettlementConfig, SlashRouting, Usdc,
};

#[tokio::main]
async fn main() -> scemadex_sdk::Result<()> {
    // A slash routes 15% to the reinsurance pool; the pool also earns premiums.
    let config = SettlementConfig::optimistic(300).with_slash_routing(SlashRouting {
        to_caller_bps: 5_000,
        to_challengers_bps: 3_000,
        to_insurance_bps: 1_500,
        to_lineage_bps: 500,
    });
    let coord = DisputeCoordinator::new(config, ManualClock::new(1_700_000_000));
    let pool = InsurancePool::with_capital(PremiumConfig::default(), Usdc::from_usdc(10.0));

    let engine = EscrowBondEngine::with_defaults();
    let policy = ReferenceRoutePolicy;

    // A well-established agent: high honor rate over many bonds → cheap premiums.
    let reputation = Reputation {
        score: 0.9,
        samples: 500,
    };

    // --- Bond 1: insured, then slashed by an upheld challenge ---------------
    let sol = policy.solve(&demo_intent()).await?;
    let bond = engine.escrow(&sol).await?;
    coord.open(&bond, sol.conviction)?;

    // Insure 80% of the bond against a slash.
    let coverage = Usdc(bond.amount.0 * 80 / 100);
    let terms = pool.quote(coverage, reputation);
    let ins = pool.bind(&bond.intent_digest, "agent-alpha", coverage, reputation)?;
    println!(
        "bond {} micro-USDC | coverage {} for premium {} ({:.2}% of coverage)",
        bond.amount.0,
        coverage.0,
        terms.premium.0,
        terms.premium.0 as f64 / coverage.0 as f64 * 100.0
    );

    coord.provision(&bond.intent_digest, BondOutcome::Honored)?;
    coord.challenge(&bond.intent_digest, "skeptic", Usdc::from_usdc(1.0))?;
    let report = coord.resolve(&bond.intent_digest, BondOutcome::Slashed)?;
    let slash = report.slash.expect("slashed");

    // The slash's insurance slice recapitalizes the pool, then coverage pays out.
    let payout = pool
        .on_settlement(&bond.intent_digest, report.outcome, slash.insurance)?
        .expect("insured");
    let _ = ins;
    let insured_loss = bond.amount.0 as i64 - payout.paid.0 as i64;
    println!(
        "\nSLASHED: agent loses {} collateral, insurance pays back {} (slice {} banked)",
        bond.amount.0, payout.paid.0, slash.insurance.0
    );
    println!(
        "  net loss {} vs. {} uninsured | pool capital now {}",
        insured_loss,
        bond.amount.0,
        pool.capital().0
    );

    // --- Bond 2: insured, honors → the pool keeps the premium as yield -------
    let mut sol2 = policy.solve(&demo_intent()).await?;
    sol2.intent_digest = format!("{}-two", sol2.intent_digest);
    sol2.conviction = Conviction::clamped(0.7);
    let bond2 = engine.escrow(&sol2).await?;
    coord.open(&bond2, sol2.conviction)?;
    let cov2 = Usdc(bond2.amount.0 * 80 / 100);
    pool.bind(&bond2.intent_digest, "agent-alpha", cov2, reputation)?;
    coord.provision(&bond2.intent_digest, BondOutcome::Honored)?;
    coord.machine().clock().advance(301); // let the window elapse
    let cap_before = pool.capital();
    let r2 = coord.sweep()?.pop().expect("one matured bond");
    let p2 = pool
        .on_settlement(&bond2.intent_digest, r2.outcome, Usdc(0))?
        .expect("insured");
    println!(
        "\nHONORED: pool keeps premium {}, pays nothing | capital {} (unchanged)",
        p2.premium.0, pool.capital().0
    );
    assert_eq!(pool.capital(), cap_before);

    println!("\npolicies in force: {}", pool.active_policies());
    Ok(())
}
