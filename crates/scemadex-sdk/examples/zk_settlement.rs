//! **Shrinking the dispute window with a succinct proof** (Primitive I → settlement).
//!
//!   cargo run -p scemadex-sdk --example zk_settlement
//!
//! An agent bonds an inference under a *long* optimistic window — a day, say — so a
//! challenger has time to refute it. With the transparent spot-check backend
//! ([`scemadex_sdk::zkbackend`]) the agent instead attaches a **succinct proof** that
//! its output really is the committed model's forward pass. Anyone verifies it in
//! `O(queries · fan_in)` — no re-execution — and the bond resolves *immediately* via
//! [`DisputeCoordinator::resolve_via_oracle`] (reason `ProofVerified`). The
//! day-long window collapses to a single check. A **forged** proof fails
//! verification and slashes into the Scar Market, exactly as any proven loss does.

use scemadex_sdk::{
    prove_inference, Bond, BondOutcome, Conviction, DenseLayer, DisputeCoordinator, ManualClock,
    SettlementConfig, SlashRouting, SpotCheckConfig, TracedMlp, Usdc,
};

fn tiny_policy_net() -> TracedMlp {
    // 3 features → 4 ReLU hidden → 2 linear outputs (a toy Q-head).
    TracedMlp::new(
        3,
        vec![
            DenseLayer::new(
                vec![
                    vec![0.5, -0.2, 0.1],
                    vec![-0.3, 0.4, 0.2],
                    vec![0.1, 0.1, -0.5],
                    vec![0.2, -0.1, 0.3],
                ],
                vec![0.05, -0.05, 0.0, 0.1],
                true,
            ),
            DenseLayer::new(
                vec![vec![0.3, -0.2, 0.5, 0.1], vec![-0.1, 0.4, -0.3, 0.2]],
                vec![0.0, 0.01],
                false,
            ),
        ],
    )
}

fn bond(digest: &str) -> Bond {
    Bond {
        intent_digest: digest.into(),
        amount: Usdc::from_usdc(5.0),
        min_out_raw: 0,
        deadline_unix: 0,
    }
}

fn main() -> scemadex_sdk::Result<()> {
    let model = tiny_policy_net();
    let input = [0.7, -1.2, 0.4];
    // A relying party's soundness floor: reject any proof with fewer spot-checks.
    const MIN_QUERIES: usize = 6;

    // A full-day optimistic window — long, because unaided refutation is slow.
    let config = SettlementConfig::optimistic(86_400).with_slash_routing(SlashRouting {
        to_caller_bps: 5_000,
        to_challengers_bps: 3_000,
        to_insurance_bps: 1_000,
        to_lineage_bps: 1_000,
    });
    let coord = DisputeCoordinator::new(config, ManualClock::new(1_700_000_000));

    // ── Honest agent: attach a succinct proof, resolve the window instantly. ──
    let proof = prove_inference(&model, &input, SpotCheckConfig { num_queries: 12 });
    println!(
        "proof: {} queries, {} authenticated cells over a {}-leaf trace",
        proof.queries(),
        proof.cells.len(),
        proof.num_leaves
    );

    coord.open(&bond("honest"), Conviction::clamped(0.9))?;
    coord.provision("honest", BondOutcome::Honored)?;
    // The verifier checks the proof *without* the model's full forward pass.
    let verdict = if proof.verify_with_min(&input, MIN_QUERIES) {
        BondOutcome::Honored
    } else {
        BondOutcome::Slashed
    };
    let r = coord.resolve_via_oracle("honest", verdict)?;
    println!("honest  -> {:?} via {:?} (window never waited out)", r.outcome, r.reason);

    // ── Fraudulent agent: same window, but a tampered proof. ──
    let mut forged = prove_inference(&model, &input, SpotCheckConfig { num_queries: 12 });
    forged.output[0] += 0.5; // claim an inference the model didn't produce
    coord.open(&bond("fraud"), Conviction::clamped(0.9))?;
    coord.provision("fraud", BondOutcome::Honored)?;
    let verdict = if forged.verify_with_min(&input, MIN_QUERIES) {
        BondOutcome::Honored
    } else {
        BondOutcome::Slashed
    };
    let r = coord.resolve_via_oracle("fraud", verdict)?;
    println!("forged  -> {:?} via {:?}", r.outcome, r.reason);
    if r.outcome == BondOutcome::Slashed {
        let scar = coord.mint_scar("fraud", "verifier", 1, vec![], Usdc::from_usdc(0.5))?;
        println!(
            "  slashed -> scar: {} micro-USDC certified, split {:?}",
            scar.slashed_collateral.0,
            r.slash.map(|s| (s.caller.0, s.challengers.0, s.insurance.0, s.lineage.0))
        );
    }

    println!("\na succinct proof turns a day-long window into one check.");
    Ok(())
}
