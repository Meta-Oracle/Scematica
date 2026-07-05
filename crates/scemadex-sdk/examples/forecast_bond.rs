//! Generalizing the rail: a **forecast venue** — not a swap — settled through the
//! exact same machinery (Primitive B/D generalized via the `Outcome` seam).
//!
//!   cargo run -p scemadex-sdk --example forecast_bond
//!
//! An agent stakes collateral on a directional call ("SOL ≥ $150 at resolution").
//! Ground truth is a `Measurement`, judged against a `Promise::Forecast`. The
//! [`ForecastVenue`] drives the whole lifecycle over a `DisputeCoordinator`, so the
//! settlement machine, counter-market, and scar market never learn this isn't a
//! swap — they only ever speak `BondOutcome`. A wrong forecast slashes and mints a
//! scar, exactly as a missed swap fill would; a doubter can even challenge a call
//! and have reality adjudicate.

use scemadex_sdk::{
    Bond, BondOutcome, Conviction, ForecastVenue, ManualClock, Measurement, Promise,
    SettlementConfig, SlashRouting, Usdc,
};

fn forecast_bond(digest: &str) -> Bond {
    // A forecast bond carries no swap min-out; its guarantee is the Promise it is
    // submitted with.
    Bond {
        intent_digest: digest.into(),
        amount: Usdc::from_usdc(5.0),
        min_out_raw: 0,
        deadline_unix: 0,
    }
}

fn report(digest: &str, r: &scemadex_sdk::SettlementReport) {
    println!("{digest}: {:?} via {:?}", r.outcome, r.reason);
}

fn main() -> scemadex_sdk::Result<()> {
    // Optimistic window + a 50/50 caller/challenger slash split so doubting a wrong
    // call actually pays. A ManualClock keeps the demo deterministic.
    let config = SettlementConfig::optimistic(3_600).with_slash_routing(SlashRouting {
        to_caller_bps: 5_000,
        to_challengers_bps: 5_000,
        to_insurance_bps: 0,
        to_lineage_bps: 0,
    });
    let venue = ForecastVenue::new(config, ManualClock::new(1_700_000_000));

    // The realized price at resolution — the same ground truth judges every call.
    let truth = Measurement::new(162.0, 1_700_000_500);

    // A · "SOL ≥ 150" — correct, unchallenged → honored.
    venue.submit(
        &forecast_bond("sol-ge-150"),
        Promise::Forecast { strike: 150.0, expect_above: true },
        Conviction::clamped(0.8),
    )?;
    report("sol-ge-150", &venue.resolve("sol-ge-150", truth)?);

    // B · "SOL ≥ 200" — wrong → slashed → scar (un-fakeable proof of loss).
    venue.submit(
        &forecast_bond("sol-ge-200"),
        Promise::Forecast { strike: 200.0, expect_above: true },
        Conviction::clamped(0.6),
    )?;
    let b = venue.resolve("sol-ge-200", truth)?;
    report("sol-ge-200", &b);
    if b.outcome == BondOutcome::Slashed {
        let scar = venue.mint_scar("sol-ge-200", "forecaster", 1, vec![], Usdc::from_usdc(0.5))?;
        println!(
            "  slashed -> scar minted: {} micro-USDC of certified pain",
            scar.slashed_collateral.0
        );
    }

    // C · "SOL ≥ 155" — a skeptic doubts it, but reality (162) vindicates the agent;
    //     the challenge is rejected and the skeptic's stake becomes agent premium.
    venue.submit(
        &forecast_bond("sol-ge-155"),
        Promise::Forecast { strike: 155.0, expect_above: true },
        Conviction::clamped(0.7),
    )?;
    venue.challenge("sol-ge-155", "skeptic", Usdc::from_usdc(1.0))?;
    let c = venue.resolve("sol-ge-155", truth)?;
    report("sol-ge-155", &c);
    if let Some(cs) = c.challenge {
        println!("  challenge rejected -> agent keeps {} micro-USDC premium", cs.agent_premium.0);
    }

    println!("\nsame settlement stack, zero swap-specific code.");
    Ok(())
}
