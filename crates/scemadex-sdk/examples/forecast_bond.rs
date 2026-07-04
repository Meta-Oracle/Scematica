//! Generalizing the rail: a **forecast** bond — not a swap — settled through the
//! exact same machinery (Primitive B/D generalized via the `Outcome` seam).
//!
//!   cargo run -p scemadex-sdk --example forecast_bond
//!
//! An agent stakes collateral on a directional call ("SOL ≥ $150 at resolution").
//! Ground truth is a `Measurement`, judged against a `Promise::Forecast`. The
//! DisputeCoordinator, settlement machine, and Scar Market don't know or care that
//! this isn't a swap — they only ever speak `BondOutcome`. A wrong forecast slashes
//! and mints a scar, exactly as a missed swap fill would.

use scemadex_sdk::{
    Bond, BondOutcome, Conviction, DisputeCoordinator, ManualClock, Measurement, Outcome, Promise,
    SettlementConfig, Usdc,
};

fn forecast_bond(digest: &str) -> Bond {
    // A forecast bond carries no swap min-out; its guarantee is the Promise below.
    Bond {
        intent_digest: digest.into(),
        amount: Usdc::from_usdc(5.0),
        min_out_raw: 0, // unused for a forecast — the machine never reads it
        deadline_unix: 0,
    }
}

fn settle_forecast(
    coord: &DisputeCoordinator<ManualClock>,
    digest: &str,
    promise: &Promise,
    truth: Measurement,
) -> scemadex_sdk::Result<()> {
    // Judge ground truth against the promise via the generic Outcome seam.
    let outcome = truth.evaluate(promise);
    coord.provision(digest, outcome)?;
    // Ground truth is authoritative; let the (short) window elapse and finalize.
    coord.machine().clock().advance(120);
    let report = coord.sweep()?.pop().expect("one matured forecast");

    println!(
        "{digest}: measured {:.0} -> {:?} via {:?}",
        truth.value, report.outcome, report.reason
    );
    if report.outcome == BondOutcome::Slashed {
        // A ground-truth slash is un-fakeable proof of loss, just like a swap slash.
        let scar = coord.mint_scar(digest, "forecaster", 1, vec![], Usdc::from_usdc(0.5))?;
        println!(
            "  slashed -> scar minted: {} micro-USDC of certified pain",
            scar.slashed_collateral.0
        );
    }
    Ok(())
}

fn main() -> scemadex_sdk::Result<()> {
    let coord = DisputeCoordinator::new(SettlementConfig::optimistic(60), ManualClock::new(1_700_000_000));

    // The realized price at resolution — the same ground truth judges both calls.
    let truth = Measurement::new(162.0, 1_700_000_500);

    // Forecast A: "SOL ≥ 150" — correct → honored.
    let a = Promise::Forecast { strike: 150.0, expect_above: true };
    let bond_a = forecast_bond("sol-ge-150");
    coord.open(&bond_a, Conviction::clamped(0.8))?;
    settle_forecast(&coord, "sol-ge-150", &a, truth)?;

    // Forecast B: "SOL ≥ 200" — wrong → slashed → scar.
    let b = Promise::Forecast { strike: 200.0, expect_above: true };
    let bond_b = forecast_bond("sol-ge-200");
    coord.open(&bond_b, Conviction::clamped(0.6))?;
    settle_forecast(&coord, "sol-ge-200", &b, truth)?;

    println!("\nsame settlement stack, zero swap-specific code.");
    Ok(())
}
