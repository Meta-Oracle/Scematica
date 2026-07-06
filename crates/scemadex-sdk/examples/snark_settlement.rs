//! **Collapsing a dispute window with a REAL zk-SNARK** (Primitive I → settlement).
//!
//!   cargo run -p scemadex-sdk --features snark --example snark_settlement
//!
//! The mirror of `zk_settlement` (transparent backend), but the proof is an arkworks
//! **Groth16 / BN254** proof: constant-size (~200 bytes), zero-knowledge over the
//! weights, cryptographic soundness. An agent bonds an inference under a *day-long*
//! optimistic window, then attaches a SNARK proving its output is the committed
//! model's forward pass. Anyone verifies it against the model's verifying key —
//! without the weights and without re-executing the net — and the bond resolves
//! immediately via [`DisputeCoordinator::resolve_via_oracle`] (`ProofVerified`). A
//! **forged** claim fails verification and slashes into the Scar Market.

use scemadex_sdk::zksnark::{ProvenModel, SnarkConfig};
use scemadex_sdk::{
    Bond, BondOutcome, Conviction, DenseLayer, DisputeCoordinator, InferenceProof, ManualClock,
    SettlementConfig, SlashRouting, TracedMlp, Usdc, SNARK_SOUNDNESS,
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
    use ark_std::rand::{rngs::StdRng, SeedableRng};
    // A CSPRNG for the demo setup; production replaces this with a ceremony.
    let mut rng = StdRng::seed_from_u64(0x5CE_11A);
    let model = tiny_policy_net();
    let input = [0.7, -1.2, 0.4];

    // One-time, per-model Groth16 setup. The verifying key IS the model's identity;
    // in production this is a ceremony, not a single-party in-process call.
    let proven = ProvenModel::setup(&model, SnarkConfig::default(), &mut rng)?;
    println!("model_id (vk hash): {}", hex16(&proven.model_id()));

    // A full-day optimistic window — long, because *unaided* refutation is slow.
    let config = SettlementConfig::optimistic(86_400).with_slash_routing(SlashRouting {
        to_caller_bps: 5_000,
        to_challengers_bps: 3_000,
        to_insurance_bps: 1_000,
        to_lineage_bps: 1_000,
    });
    let coord = DisputeCoordinator::new(config, ManualClock::new(1_700_000_000));

    // ── Honest agent: attach a SNARK, resolve the window instantly. ──
    let proof = proven.prove(&input, &mut rng)?;
    println!(
        "proof: {} bytes (constant-size), claims output {:?}",
        proof.proof_bytes.len(),
        proof
            .claimed_output()
            .iter()
            .map(|y| (y * 1000.0).round() / 1000.0)
            .collect::<Vec<_>>()
    );

    coord.open(&bond("honest"), Conviction::clamped(0.9))?;
    coord.provision("honest", BondOutcome::Honored)?;
    // The verifier checks the SNARK against the VK — no weights, no forward pass.
    let verdict = if proof.verify_inference(&input, SNARK_SOUNDNESS) {
        BondOutcome::Honored
    } else {
        BondOutcome::Slashed
    };
    let r = coord.resolve_via_oracle("honest", verdict)?;
    println!("honest  -> {:?} via {:?} (window never waited out)", r.outcome, r.reason);

    // ── Fraudulent agent: same model + window, but a tampered output claim. ──
    let mut forged = proven.prove(&input, &mut rng)?;
    forged.output_q[0] += 1; // claim an inference the model didn't produce
    coord.open(&bond("fraud"), Conviction::clamped(0.9))?;
    coord.provision("fraud", BondOutcome::Honored)?;
    let verdict = if forged.verify_inference(&input, SNARK_SOUNDNESS) {
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

    println!("\na constant-size SNARK turns a day-long window into one check — no weights revealed.");
    Ok(())
}

fn hex16(bytes: &[u8; 32]) -> String {
    bytes[..8].iter().map(|b| format!("{b:02x}")).collect()
}
