//! Live integration tests for the real Jupiter quote → solve → execute path.
//!
//! These hit the Jupiter v6 aggregator over the network, so they are gated
//! behind the `live-tests` feature and are NEVER run by the default test suite
//! or CI. Run them deliberately:
//!
//!   cargo test -p scemadex-integrations --features live-tests -- --nocapture
//!
//! They exercise quoting and transaction *building* only — they do not sign or
//! submit anything on-chain (no keypair, no funds at risk). The signed-submit
//! path (`JupiterVenueExecutor::with_signer`) is intentionally not covered here;
//! settling a real transaction belongs in a manual, funded, devnet harness.
#![cfg(feature = "live-tests")]

use std::str::FromStr;

use scemadex_integrations::jupiter::{JupiterRoutePolicy, JupiterVenueExecutor};
use scemadex_sdk::{demo_intent, RoutePolicy, VenueExecutor};
use solana_sdk::pubkey::Pubkey;

/// A real Jupiter quote must produce a positive expected output, a valid single
/// full-order route, a conviction in [0,1], and a Jupiter rationale — i.e. the
/// Conviction-Routing bond is sized against an actual fill estimate.
#[tokio::test]
async fn jupiter_quote_yields_real_solution() {
    let policy = JupiterRoutePolicy::new();
    let solution = policy
        .solve(&demo_intent())
        .await
        .expect("live Jupiter quote should succeed (needs network)");

    assert!(
        solution.route.expected_out.raw > 0,
        "expected_out must be a real, positive quote"
    );
    assert!(solution.route.splits_valid(), "splits must sum to 10_000 bps");
    assert!(
        (0.0..=1.0).contains(&solution.conviction.0),
        "conviction must be normalized"
    );
    assert!(
        solution.rationale.contains("Jupiter"),
        "rationale should describe the Jupiter route, got: {}",
        solution.rationale
    );
    eprintln!(
        "live solution: out={} conviction={:.3} :: {}",
        solution.route.expected_out.raw, solution.conviction.0, solution.rationale
    );
}

/// In dry mode (no signer), `execute()` returns the freshly-quoted fill with a
/// zero timestamp and never touches the chain.
#[tokio::test]
async fn dry_execute_reports_quoted_fill_without_submitting() {
    let policy = JupiterRoutePolicy::new();
    let solution = policy.solve(&demo_intent()).await.expect("quote");

    // System program id is a valid pubkey; it is only used to build (not sign).
    let owner = Pubkey::from_str("11111111111111111111111111111111").unwrap();
    let executor = JupiterVenueExecutor::new(owner);

    let fill = executor
        .execute(&solution.route)
        .await
        .expect("dry execute should re-quote and return a fill");

    assert!(fill.amount_out.raw > 0, "dry fill must carry a quoted amount");
    assert_eq!(
        fill.executed_unix, 0,
        "dry mode must not stamp an execution time (no submission)"
    );
}
