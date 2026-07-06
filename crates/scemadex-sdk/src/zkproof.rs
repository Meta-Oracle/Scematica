//! **The proof-of-inference role** — the abstract contract every zk backend behind
//! Primitive I fulfils, so the settlement layer never cares *which* proof system
//! collapsed the dispute window.
//!
//! A dispute window is long only because unaided refutation is slow: a challenger
//! must hold the model and re-run the whole forward pass. A proof-of-inference
//! removes that cost — it binds a public `(input, output)` pair to a correct forward
//! pass of a committed model, and a relying party verifies it *without re-executing
//! the network*. A valid proof resolves a bond deterministically via
//! [`crate::coordinator::DisputeCoordinator::resolve_via_oracle`]
//! ([`crate::settlement::FinalizeReason::ProofVerified`]) — which is exactly how any
//! real zk backend shrinks the window `W` toward zero.
//!
//! Two implementations ship, both behind this same trait:
//!
//! - **Transparent** ([`crate::zkbackend::SpotCheckProof`], always compiled) — a
//!   hash-based interactive-oracle proof (STARK-family core, Fiat-Shamir), no trusted
//!   setup, tunable soundness via spot-check count. Succinct *verification*, but the
//!   proof carries per-query openings and reveals the weights it opens.
//! - **SNARK** ([`crate::zksnark::SnarkInferenceProof`], `snark` feature) — arkworks
//!   Groth16 over BN254: a constant-size (~200-byte) proof, cryptographic soundness,
//!   and genuine zero-knowledge — the weights live behind the verifying key and never
//!   appear in the proof. This is the "real SNARK" the transparent backend documents
//!   as the piece that slots in behind this role without touching the coordinator.
//!
//! Because both satisfy [`InferenceProof`], settlement code binds to the trait and a
//! deployment swaps proof systems by swapping the concrete type — nothing downstream
//! of [`verify_inference`](InferenceProof::verify_inference) changes.

/// A backend-agnostic proof that some committed model maps a caller-supplied input to
/// a claimed output, checkable without re-running the model.
///
/// The settlement facilitator only ever calls
/// [`verify_inference`](Self::verify_inference) with its own soundness floor and turns
/// the boolean into a [`BondOutcome`](crate::route::BondOutcome) fed to
/// [`resolve_via_oracle`](crate::coordinator::DisputeCoordinator::resolve_via_oracle).
pub trait InferenceProof {
    /// The output this proof claims the model produced on its input, dequantised to
    /// the caller's units. Pinned to the proof — tampering with it fails verification.
    fn claimed_output(&self) -> &[f64];

    /// This proof's soundness parameter, on a scale comparable to a relying party's
    /// floor. For the transparent backend it is the spot-check query count; for a
    /// cryptographic SNARK it is an effective bit-security (see
    /// [`SNARK_SOUNDNESS`]). Larger is stronger.
    fn soundness(&self) -> usize;

    /// Verify that this proof binds `input` to [`claimed_output`](Self::claimed_output)
    /// as a correct forward pass of the committed model, at or above the caller's
    /// `min_soundness` floor. Returns `false` on any inconsistency, on a wrong
    /// `input`, or when [`soundness`](Self::soundness) `< min_soundness`.
    ///
    /// Crucially this does **not** re-run the network — that is the whole point of a
    /// proof-of-inference and the reason it collapses a dispute window.
    fn verify_inference(&self, input: &[f64], min_soundness: usize) -> bool;
}

/// The effective bit-security a cryptographic SNARK proof reports as its
/// [`soundness`](InferenceProof::soundness). Groth16 over BN254 sits at roughly this
/// level; a relying party's floor is compared against it. It dwarfs any spot-check
/// count a transparent proof can practically carry, which is the honest reflection of
/// "a SNARK's soundness is cryptographic, not sampling-based."
pub const SNARK_SOUNDNESS: usize = 128;

impl InferenceProof for crate::zkbackend::SpotCheckProof {
    fn claimed_output(&self) -> &[f64] {
        &self.output
    }

    fn soundness(&self) -> usize {
        self.num_queries
    }

    fn verify_inference(&self, input: &[f64], min_soundness: usize) -> bool {
        self.verify_with_min(input, min_soundness)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zkbackend::{prove, DenseLayer, SpotCheckConfig, TracedMlp};

    fn net() -> TracedMlp {
        TracedMlp::new(
            2,
            vec![DenseLayer::new(vec![vec![0.5, -0.2], vec![0.1, 0.3]], vec![0.0, 0.1], false)],
        )
    }

    #[test]
    fn transparent_proof_satisfies_the_role() {
        let m = net();
        let input = [0.4, -0.7];
        let proof = prove(&m, &input, SpotCheckConfig { num_queries: 8 });
        // Exercised purely through the trait — the settlement layer's view. The net
        // has 2 computed neurons, so queries clamp to 2 (the max meaningful floor).
        let dyn_proof: &dyn InferenceProof = &proof;
        assert!(dyn_proof.verify_inference(&input, 2));
        assert_eq!(dyn_proof.soundness(), 2); // clamped to the 2 computed neurons
        assert_eq!(dyn_proof.claimed_output().len(), 2);
    }

    #[test]
    fn role_rejects_below_soundness_floor() {
        let m = net();
        let input = [0.4, -0.7];
        let proof = prove(&m, &input, SpotCheckConfig { num_queries: 1 });
        let dyn_proof: &dyn InferenceProof = &proof;
        // Floor above the proof's soundness must be refused, even on the right input.
        assert!(!dyn_proof.verify_inference(&input, 8));
    }
}
