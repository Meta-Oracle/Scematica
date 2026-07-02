//! zkML-verified conviction bonds (Primitive I) — the capstone of the
//! Adversarial Layer.
//!
//! A [`crate::bond::Bond`] makes a *paid black-box inference* trustworthy by
//! giving the agent slashable skin in the game. But it still trusts the agent's
//! word that the quote came from the model it claims. This module removes that
//! last trust assumption: it binds a bond to a **cryptographic commitment that a
//! specific model produced a specific inference**, so a lie is *provable* and the
//! bond is slashed on proof — not on reputation.
//!
//! ## Trust model
//!
//! The Scematica Deep Q* net is pure-Rust and **deterministic** (no
//! ML-framework nondeterminism), which is exactly what makes verifiable
//! inference tractable here:
//!
//! 1. **Commit** — the model is committed to as a SHA-256 Merkle root over its
//!    quantised parameters (`ModelCommitment`). Publishing the root fixes the
//!    weights without revealing them up front.
//! 2. **Attest** — for an inference, the agent publishes an
//!    [`InferenceAttestation`] binding `model_root · hash(input) · output`.
//! 3. **Verify** — any challenger holding the model + input **re-executes** the
//!    forward pass and checks the binding. A mismatch is a fraud proof; the bond
//!    slashes regardless of the realised fill ([`VerifiedBond::settle_verified`]).
//!
//! This is *optimistic* verification (trustless-by-refutation), the same shape as
//! an optimistic rollup. [`InferenceProofSystem`] is the seam to drop in a
//! **succinct** zk backend (risc0 / SP1 / halo2) later without changing the bond
//! API: `ReexecutionProofSystem` is the reference implementation shipping today.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::bond::{Bond, BondOutcome};
use crate::route::Fill;

/// Fixed-point scale for deterministic float→integer quantisation. Two f64s that
/// agree to ~6 decimals commit to the same bytes, sidestepping float-formatting
/// ambiguity across platforms.
const FIXED_SCALE: f64 = 1_000_000.0;

/// Domain-separation tags so leaf / node / input / output hashes can never collide.
const DOMAIN_LEAF: &[u8] = b"scemadex.zkbond.v1.leaf";
const DOMAIN_NODE: &[u8] = b"scemadex.zkbond.v1.node";
const DOMAIN_INPUT: &[u8] = b"scemadex.zkbond.v1.input";
const DOMAIN_OUTPUT: &[u8] = b"scemadex.zkbond.v1.output";

fn quantise(x: f64) -> i64 {
    (x * FIXED_SCALE).round() as i64
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// Hash a quantised vector under a domain tag (used for input & output binding).
fn hash_vec(domain: &[u8], v: &[f64]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(domain);
    h.update((v.len() as u64).to_le_bytes());
    for &x in v {
        h.update(quantise(x).to_le_bytes());
    }
    h.finalize().into()
}

/// Merkle root over quantised parameters. Leaves are `H(LEAF · i · q(w_i))`;
/// internal nodes are `H(NODE · left · right)`, duplicating the last node on odd
/// levels. Empty models commit to `H(LEAF · "empty")`.
fn merkle_root(params: &[f64]) -> [u8; 32] {
    if params.is_empty() {
        return sha256(&[DOMAIN_LEAF, b"empty"]);
    }
    let mut level: Vec<[u8; 32]> = params
        .iter()
        .enumerate()
        .map(|(i, &w)| sha256(&[DOMAIN_LEAF, &(i as u64).to_le_bytes(), &quantise(w).to_le_bytes()]))
        .collect();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i < level.len() {
            let left = level[i];
            let right = if i + 1 < level.len() { level[i + 1] } else { level[i] };
            next.push(sha256(&[DOMAIN_NODE, &left, &right]));
            i += 2;
        }
        level = next;
    }
    level[0]
}

/// A model whose inference can be committed to and re-executed deterministically.
///
/// The Scematica agent implements this behind the `scematica` feature (see
/// [`nn_models`]); tests and third-party models can implement it directly.
pub trait CommittableModel {
    /// Canonical, stable-ordered flattening of every parameter (weights + biases).
    fn parameters(&self) -> Vec<f64>;
    /// Deterministic forward pass producing the public output (e.g. Q-values).
    fn infer(&self, input: &[f64]) -> Vec<f64>;
    /// Architecture identifier bound into the commitment (guards against a model
    /// with coincidentally-equal weights but a different shape).
    fn architecture_tag(&self) -> String;
}

/// A commitment to a model: a Merkle root over its quantised parameters plus its
/// architecture tag and parameter count.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCommitment {
    pub root: String,
    pub num_params: usize,
    pub architecture: String,
}

impl ModelCommitment {
    pub fn of<M: CommittableModel + ?Sized>(model: &M) -> Self {
        let params = model.parameters();
        let arch = model.architecture_tag();
        // Fold the arch tag into the committed root so shape is bound too.
        let param_root = merkle_root(&params);
        let root = sha256(&[DOMAIN_NODE, &param_root, arch.as_bytes()]);
        Self {
            root: to_hex(&root),
            num_params: params.len(),
            architecture: arch,
        }
    }
}

/// A binding proof that the committed model produced `output` for a specific
/// input. `commitment` = `H(OUTPUT · model_root · input_hash · q(output) · nonce)`.
// No `Eq`: `output: Vec<f64>` isn't `Eq`. Equality is by the committed hash
// fields anyway (see `PartialEq`), which is exact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InferenceAttestation {
    pub model: ModelCommitment,
    pub input_hash: String,
    pub output: Vec<f64>,
    pub commitment: String,
    /// Anti-replay / freshness nonce (e.g. the intent digest folded to a u64).
    pub nonce: u64,
}

impl InferenceAttestation {
    /// Produce an attestation for `model.infer(input)`.
    pub fn create<M: CommittableModel + ?Sized>(model: &M, input: &[f64], nonce: u64) -> Self {
        let commitment = ModelCommitment::of(model);
        let output = model.infer(input);
        let input_hash = to_hex(&hash_vec(DOMAIN_INPUT, input));
        let commitment_hash = Self::bind(&commitment.root, &input_hash, &output, nonce);
        Self {
            model: commitment,
            input_hash,
            output,
            commitment: commitment_hash,
            nonce,
        }
    }

    fn bind(model_root: &str, input_hash: &str, output: &[f64], nonce: u64) -> String {
        let out_hash = hash_vec(DOMAIN_OUTPUT, output);
        to_hex(&sha256(&[
            DOMAIN_OUTPUT,
            model_root.as_bytes(),
            input_hash.as_bytes(),
            &out_hash,
            &nonce.to_le_bytes(),
        ]))
    }

    /// Recompute the binding from the attestation's own fields and check internal
    /// consistency (cheap; does not prove the *model* — see
    /// [`verify_by_reexecution`]).
    pub fn is_internally_consistent(&self) -> bool {
        let expected = Self::bind(&self.model.root, &self.input_hash, &self.output, self.nonce);
        expected == self.commitment
    }
}

/// The fraud proof: re-execute `model` on `input` and check the attestation binds
/// to that exact `(model, input, output)`. Any honest party holding the model and
/// input can call this to refute a false attestation.
pub fn verify_by_reexecution<M: CommittableModel + ?Sized>(
    att: &InferenceAttestation,
    model: &M,
    input: &[f64],
) -> bool {
    // 1. The committed model must be the one we hold.
    let commitment = ModelCommitment::of(model);
    if commitment != att.model {
        return false;
    }
    // 2. The input must be the one attested.
    if to_hex(&hash_vec(DOMAIN_INPUT, input)) != att.input_hash {
        return false;
    }
    // 3. Re-execution must reproduce the attested output (quantised equality).
    let recomputed = model.infer(input);
    if recomputed.len() != att.output.len()
        || recomputed
            .iter()
            .zip(&att.output)
            .any(|(a, b)| quantise(*a) != quantise(*b))
    {
        return false;
    }
    // 4. And the published binding must be correct.
    att.is_internally_consistent()
}

/// A conviction bond bound to a verifiable inference. Settlement honours the bond
/// only if the inference is proven *and* the fill meets the guarantee; a
/// fraudulent attestation is slashed regardless of the fill.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifiedBond {
    pub bond: Bond,
    pub attestation: InferenceAttestation,
}

impl VerifiedBond {
    pub fn new(bond: Bond, attestation: InferenceAttestation) -> Self {
        Self { bond, attestation }
    }

    /// Re-execute and verify the bound inference.
    pub fn verify<M: CommittableModel + ?Sized>(&self, model: &M, input: &[f64]) -> bool {
        verify_by_reexecution(&self.attestation, model, input)
    }

    /// Settle with verification: a proven-false inference slashes unconditionally;
    /// otherwise the usual guaranteed-min-output rule applies.
    pub fn settle_verified<M: CommittableModel + ?Sized>(
        &self,
        model: &M,
        input: &[f64],
        fill: &Fill,
    ) -> BondOutcome {
        if !self.verify(model, input) {
            return BondOutcome::Slashed;
        }
        if fill.amount_out.raw >= self.bond.min_out_raw {
            BondOutcome::Honored
        } else {
            BondOutcome::Slashed
        }
    }
}

/// Seam for pluggable inference-proof backends. The reference implementation
/// ([`ReexecutionProofSystem`]) proves by deterministic re-execution; a future
/// implementation can produce a succinct zk-SNARK over the same attestation
/// without changing [`VerifiedBond`].
pub trait InferenceProofSystem {
    /// Opaque proof artefact.
    type Proof;
    /// Produce a proof that `model` yields its inference on `input`.
    fn prove<M: CommittableModel + ?Sized>(&self, model: &M, input: &[f64], nonce: u64) -> Self::Proof;
    /// Verify a proof binds to `expected`.
    fn verify(&self, proof: &Self::Proof, expected: &InferenceAttestation) -> bool;
}

/// Reference proof system: the "proof" is the attestation itself, verified by
/// re-execution at settlement time. Trustless-by-refutation, zero extra deps.
pub struct ReexecutionProofSystem;

impl InferenceProofSystem for ReexecutionProofSystem {
    type Proof = InferenceAttestation;

    fn prove<M: CommittableModel + ?Sized>(
        &self,
        model: &M,
        input: &[f64],
        nonce: u64,
    ) -> Self::Proof {
        InferenceAttestation::create(model, input, nonce)
    }

    fn verify(&self, proof: &Self::Proof, expected: &InferenceAttestation) -> bool {
        proof == expected && proof.is_internally_consistent()
    }
}

/// `CommittableModel` implementations for the real Scematica Deep Q* networks.
/// Only compiled with the `scematica` feature.
#[cfg(feature = "scematica")]
#[cfg_attr(docsrs, doc(cfg(feature = "scematica")))]
pub mod nn_models {
    use super::CommittableModel;
    use scematica_nn::network::QNetwork;
    use scematica_nn::QuantileNetwork;

    fn linear_params(l: &scematica_nn::network::Linear, out: &mut Vec<f64>) {
        for row in &l.weights {
            out.extend_from_slice(row);
        }
        out.extend_from_slice(&l.biases);
    }

    impl CommittableModel for QNetwork {
        fn parameters(&self) -> Vec<f64> {
            let mut p = Vec::new();
            for l in &self.layers {
                linear_params(l, &mut p);
            }
            if let Some(v) = &self.value_head {
                linear_params(v, &mut p);
            }
            if let Some(a) = &self.advantage_head {
                linear_params(a, &mut p);
            }
            p
        }
        fn infer(&self, input: &[f64]) -> Vec<f64> {
            self.forward(input)
        }
        fn architecture_tag(&self) -> String {
            format!("scematica-qnet:{:?}", self.layer_sizes)
        }
    }

    impl CommittableModel for QuantileNetwork {
        fn parameters(&self) -> Vec<f64> {
            let mut p = Vec::new();
            for l in &self.layers {
                linear_params(l, &mut p);
            }
            linear_params(&self.value_head, &mut p);
            linear_params(&self.advantage_head, &mut p);
            p
        }
        fn infer(&self, input: &[f64]) -> Vec<f64> {
            // Public output is the mean-of-quantiles Q-vector.
            self.q_values(input)
        }
        fn architecture_tag(&self) -> String {
            format!(
                "scematica-qrdqn:{:?}:q{}:a{}",
                self.layer_sizes, self.n_quantiles, self.action_dim
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bond::Bond;
    use crate::primitives::{Amount, Usdc};

    /// A tiny deterministic model for exercising the commitment/attestation logic
    /// without the `scematica` feature.
    struct MockModel {
        params: Vec<f64>,
    }
    impl CommittableModel for MockModel {
        fn parameters(&self) -> Vec<f64> {
            self.params.clone()
        }
        fn infer(&self, input: &[f64]) -> Vec<f64> {
            let bias: f64 = self.params.iter().sum();
            vec![input.iter().sum::<f64>() + bias, bias]
        }
        fn architecture_tag(&self) -> String {
            "mock-1".into()
        }
    }

    fn model() -> MockModel {
        MockModel { params: vec![0.1, -0.2, 0.3, 0.4] }
    }

    #[test]
    fn attestation_verifies_against_same_model_and_input() {
        let m = model();
        let input = [1.0, 2.0, 3.0];
        let att = InferenceAttestation::create(&m, &input, 42);
        assert!(att.is_internally_consistent());
        assert!(verify_by_reexecution(&att, &m, &input));
    }

    #[test]
    fn verify_fails_if_model_weights_differ() {
        let att = InferenceAttestation::create(&model(), &[1.0, 2.0, 3.0], 1);
        let tampered = MockModel { params: vec![0.1, -0.2, 0.3, 0.5] }; // last weight changed
        assert!(!verify_by_reexecution(&att, &tampered, &[1.0, 2.0, 3.0]));
    }

    #[test]
    fn verify_fails_if_input_differs() {
        let m = model();
        let att = InferenceAttestation::create(&m, &[1.0, 2.0, 3.0], 1);
        assert!(!verify_by_reexecution(&att, &m, &[1.0, 2.0, 3.5]));
    }

    #[test]
    fn verify_fails_if_output_is_tampered() {
        let m = model();
        let mut att = InferenceAttestation::create(&m, &[1.0, 2.0, 3.0], 1);
        att.output[0] += 1.0; // claim a different inference
        // Internal consistency now broken AND re-execution disagrees.
        assert!(!att.is_internally_consistent());
        assert!(!verify_by_reexecution(&att, &m, &[1.0, 2.0, 3.0]));
    }

    #[test]
    fn model_commitment_is_deterministic() {
        let a = ModelCommitment::of(&model());
        let b = ModelCommitment::of(&model());
        assert_eq!(a, b);
        assert_eq!(a.num_params, 4);
    }

    #[test]
    fn fraudulent_inference_slashes_regardless_of_fill() {
        let m = model();
        let input = [1.0, 2.0, 3.0];
        let att = InferenceAttestation::create(&m, &input, 7);
        let bond = Bond {
            intent_digest: "d".into(),
            amount: Usdc::from_usdc(5.0),
            min_out_raw: 1_000_000,
            deadline_unix: 0,
        };
        let vbond = VerifiedBond::new(bond, att);
        let good_fill = Fill { amount_out: Amount::new(1_000_000, 6), executed_unix: 0 };

        // Honest model + meeting fill → honored.
        assert_eq!(vbond.settle_verified(&m, &input, &good_fill), BondOutcome::Honored);

        // A different model can't satisfy the commitment → slashed even though the
        // fill met the guarantee (the inference provenance is what failed).
        let liar = MockModel { params: vec![9.9, 9.9, 9.9, 9.9] };
        assert_eq!(vbond.settle_verified(&liar, &input, &good_fill), BondOutcome::Slashed);
    }

    #[test]
    fn reexecution_proof_system_roundtrips() {
        let m = model();
        let input = [0.5, 0.5];
        let ps = ReexecutionProofSystem;
        let proof = ps.prove(&m, &input, 3);
        let expected = InferenceAttestation::create(&m, &input, 3);
        assert!(ps.verify(&proof, &expected));
    }
}
