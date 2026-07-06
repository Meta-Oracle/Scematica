//! **A real zk-SNARK proof-of-inference backend** — arkworks Groth16 over BN254,
//! slotting in behind the same [`InferenceProof`](crate::zkproof::InferenceProof) role
//! as the transparent [`zkbackend`](crate::zkbackend), and finishing the job that
//! module's header names: *"what a SNARK backend adds behind this same role."*
//!
//! ## What it is
//!
//! The prover commits to a quantised MLP as an **R1CS circuit** and produces a
//! Groth16 proof that a public `(input, output)` pair is a correct forward pass. The
//! model's **weights are circuit constants baked into the proving/verifying key at
//! setup** — they are *the statement*, not part of the proof — so:
//!
//! - **Zero-knowledge.** The proof is three BN254 group elements (~200 bytes). It
//!   reveals nothing about the weights; a verifier holding only the [`ProvenModel`]'s
//!   verifying key learns that *this* model maps input→output and no more. The
//!   transparent backend, by contrast, opens (reveals) every weight a spot-check
//!   touches.
//! - **Constant size, cryptographic soundness.** Proof size and verifier work are
//!   independent of the network's parameter count, and soundness is Groth16's
//!   (≈128-bit, [`SNARK_SOUNDNESS`](crate::zkproof::SNARK_SOUNDNESS)) rather than a
//!   `(1 − 1/N)^q` sampling bound you dial up with more openings.
//! - **Model identity = verifying key.** [`ProvenModel::model_id`] /
//!   [`SnarkInferenceProof::model_id`] hash the VK, giving the "registered model
//!   commitment" the transparent backend can't: a proof is bound to exactly the
//!   weights its VK was set up over.
//!
//! ## Honest caveats
//!
//! - **Trusted setup.** Groth16 needs a per-circuit trusted setup; [`ProvenModel::setup`]
//!   runs it in-process for tests/demos with a caller-supplied RNG. A production
//!   deployment replaces that single-party setup with a ceremony (or swaps in a
//!   transparent SNARK — halo2/STARK — behind this identical trait; only this module
//!   changes). The *transparent* backend needs no setup at all, which is why it stays
//!   the default.
//! - **Fixed-point, not float.** The circuit proves an exact integer forward pass over
//!   a fixed-point quantisation ([`SnarkConfig::scale`]); it is a *different, exact*
//!   semantics from the transparent backend's per-layer float requantisation, not a
//!   bit-for-bit re-encoding of it. Both are faithful proofs of *a* forward pass — they
//!   just quantise differently.
//! - **Quantisation bound.** Every neuron's integer pre-activation must fit in
//!   [`SnarkConfig::acc_bits`] bits (the ReLU range-check width). Pick `scale` so that
//!   holds for the model's activation range; the default pair is sized for small
//!   policy heads. Proving fails loudly if the bound is exceeded.
//!
//! ## How it collapses a dispute window
//!
//! Identical to the transparent path: a valid proof turns into
//! [`BondOutcome::Honored`](crate::route::BondOutcome) and resolves via
//! [`DisputeCoordinator::resolve_via_oracle`](crate::coordinator::DisputeCoordinator::resolve_via_oracle)
//! ([`FinalizeReason::ProofVerified`](crate::settlement::FinalizeReason)); a forged one
//! fails and slashes into the Scar Market. See `examples/snark_settlement.rs`.

use ark_bn254::{Bn254, Fr};
use ark_ff::PrimeField;
use ark_groth16::{Groth16, Proof, ProvingKey, VerifyingKey};
use ark_r1cs_std::{
    alloc::AllocVar,
    boolean::Boolean,
    eq::EqGadget,
    fields::fp::FpVar,
    fields::FieldVar,
    R1CSVar,
};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use ark_std::rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::ScemaDexError;
use crate::zkbackend::TracedMlp;
use crate::zkproof::{InferenceProof, SNARK_SOUNDNESS};

/// How to quantise the model for the circuit.
#[derive(Clone, Copy, Debug)]
pub struct SnarkConfig {
    /// Fixed-point scale: inputs and weights are rounded to `round(x · scale)`. Layer
    /// pre-activations accumulate scale multiplicatively (see the module docs), so the
    /// output is at `scale^(num_layers + 1)`. Larger `scale` = more precision but
    /// larger integers — keep every pre-activation under `2^acc_bits`.
    pub scale: f64,
    /// Bit-width of the ReLU range check. Each hidden neuron's `|pre-activation|` must
    /// fit here. Wider is sounder-by-construction but adds constraints; 64 comfortably
    /// covers a small quantised policy head.
    pub acc_bits: usize,
}

impl Default for SnarkConfig {
    fn default() -> Self {
        Self { scale: 1024.0, acc_bits: 64 }
    }
}

fn quantize_input(input: &[f64], scale: f64) -> Vec<i64> {
    input.iter().map(|&x| (x * scale).round() as i64).collect()
}

/// Quantised model: `(weights[layer][out][in], biases[layer][out], relu[layer],
/// output_scale)`. Weights are scale `s`; biases and pre-activations are scale
/// `s^(layer_index + 2)`; the output lands at `output_scale = s^(num_layers + 1)`.
type QuantizedModel = (Vec<Vec<Vec<i64>>>, Vec<Vec<i128>>, Vec<bool>, f64);

/// Quantise a float MLP into integer weights (scale `s`) and biases (scale
/// `s^(layer_index + 2)`, matching the pre-activation scale), plus the ReLU flags and
/// the final output scale `s^(num_layers + 1)`.
fn quantize_model(model: &TracedMlp, scale: f64) -> QuantizedModel {
    let mut weights_q = Vec::with_capacity(model.layers.len());
    let mut biases_q = Vec::with_capacity(model.layers.len());
    let mut relu = Vec::with_capacity(model.layers.len());
    let mut in_scale = scale; // input activation scale s_0
    for layer in &model.layers {
        let w: Vec<Vec<i64>> = layer
            .weights
            .iter()
            .map(|row| row.iter().map(|&w| (w * scale).round() as i64).collect())
            .collect();
        let out_scale = scale * in_scale; // pre-activation / bias scale for this layer
        let b: Vec<i128> = layer.biases.iter().map(|&bb| (bb * out_scale).round() as i128).collect();
        weights_q.push(w);
        biases_q.push(b);
        relu.push(layer.relu);
        in_scale = out_scale;
    }
    (weights_q, biases_q, relu, in_scale)
}

/// The exact integer forward pass the circuit enforces — run on the host to obtain the
/// public output. Panics only on a structurally invalid model (a programming error).
fn forward_i128(
    weights_q: &[Vec<Vec<i64>>],
    biases_q: &[Vec<i128>],
    relu: &[bool],
    input_q: &[i64],
) -> Vec<i128> {
    let mut prev: Vec<i128> = input_q.iter().map(|&x| x as i128).collect();
    for l in 0..weights_q.len() {
        let mut acts = Vec::with_capacity(biases_q[l].len());
        for o in 0..weights_q[l].len() {
            let mut acc = biases_q[l][o];
            for (i, &w) in weights_q[l][o].iter().enumerate() {
                acc += w as i128 * prev[i];
            }
            if relu[l] && acc < 0 {
                acc = 0;
            }
            acts.push(acc);
        }
        prev = acts;
    }
    prev
}

fn fr_from_i128(n: i128) -> Fr {
    if n >= 0 {
        Fr::from(n as u128)
    } else {
        -Fr::from((-n) as u128)
    }
}

/// `Some(v)` iff the field element is a small non-negative integer `v < 2^128`.
fn fr_to_u128(f: Fr) -> Option<u128> {
    let limbs = f.into_bigint().0; // BN254 Fr → BigInt<4> = [u64; 4]
    if limbs[2] != 0 || limbs[3] != 0 {
        return None;
    }
    Some((limbs[0] as u128) | ((limbs[1] as u128) << 64))
}

/// Interpret a field element as a signed integer whose magnitude fits `i128` (true for
/// every honest value in this circuit — pre-activations are `acc_bits`-bounded).
fn fr_to_i128(f: Fr) -> i128 {
    if let Some(v) = fr_to_u128(f) {
        return v as i128;
    }
    if let Some(v) = fr_to_u128(-f) {
        return -(v as i128);
    }
    panic!("field element out of i128 range — quantisation bound exceeded");
}

/// Enforce `var ∈ [0, 2^bits)` by decomposing it into `bits` boolean witnesses and
/// re-summing. Doubles as the non-negativity proof the ReLU gadget relies on.
fn enforce_range(
    cs: ConstraintSystemRef<Fr>,
    var: &FpVar<Fr>,
    bits: usize,
) -> Result<(), SynthesisError> {
    let val = var.value().ok().map(|f| fr_to_u128(f).expect("range value fits u128"));
    let mut bit_vars = Vec::with_capacity(bits);
    for i in 0..bits {
        let b = val.map(|v| (v >> i) & 1 == 1);
        bit_vars.push(Boolean::new_witness(cs.clone(), || {
            b.ok_or(SynthesisError::AssignmentMissing)
        })?);
    }
    let recon = Boolean::le_bits_to_fp_var(&bit_vars)?;
    recon.enforce_equal(var)
}

/// ReLU as an R1CS gadget: given `acc`, prove `y = max(0, acc)`.
///
/// Introduces `y, neg ≥ 0` (range-checked to `bits`) with `acc = y − neg` and the
/// complementarity `y · neg = 0`. With no field wraparound (guaranteed while
/// `2^bits < (p−1)/2`), exactly one of `y, neg` is zero, forcing `y = max(0, acc)`.
fn relu_gadget(
    cs: ConstraintSystemRef<Fr>,
    acc: &FpVar<Fr>,
    bits: usize,
) -> Result<FpVar<Fr>, SynthesisError> {
    let (y_val, neg_val) = match acc.value() {
        Ok(a) => {
            let s = fr_to_i128(a);
            (Some(fr_from_i128(s.max(0))), Some(fr_from_i128((-s).max(0))))
        }
        Err(_) => (None, None),
    };
    let y = FpVar::new_witness(cs.clone(), || y_val.ok_or(SynthesisError::AssignmentMissing))?;
    let neg = FpVar::new_witness(cs.clone(), || neg_val.ok_or(SynthesisError::AssignmentMissing))?;
    enforce_range(cs.clone(), &y, bits)?;
    enforce_range(cs.clone(), &neg, bits)?;
    acc.enforce_equal(&(&y - &neg))?;
    (&y * &neg).enforce_equal(&FpVar::zero())?;
    Ok(y)
}

/// The R1CS statement: "`output` is the forward pass of the committed (constant)
/// weights on public `input`." Inputs and outputs are public; every activation and
/// ReLU auxiliary is a private witness.
#[derive(Clone)]
struct MlpCircuit {
    weights_q: Vec<Vec<Vec<i64>>>,
    biases_q: Vec<Vec<i128>>,
    relu: Vec<bool>,
    input_dim: usize,
    acc_bits: usize,
    /// Present at proving; `None` at setup (structure only).
    input_q: Option<Vec<i64>>,
    output_q: Option<Vec<i128>>,
}

impl ConstraintSynthesizer<Fr> for MlpCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // Public inputs #1: the input vector (allocated first → lowest public indices).
        let mut prev: Vec<FpVar<Fr>> = Vec::with_capacity(self.input_dim);
        for i in 0..self.input_dim {
            let x = FpVar::new_input(cs.clone(), || {
                self.input_q
                    .as_ref()
                    .map(|v| fr_from_i128(v[i] as i128))
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;
            prev.push(x);
        }

        // Affine + ReLU layers. Constant weights ⇒ each pre-activation is a linear
        // combination (no witness); only ReLU introduces witnesses.
        for l in 0..self.weights_q.len() {
            let mut acts = Vec::with_capacity(self.biases_q[l].len());
            for o in 0..self.weights_q[l].len() {
                let mut acc = FpVar::constant(fr_from_i128(self.biases_q[l][o]));
                for (i, &w) in self.weights_q[l][o].iter().enumerate() {
                    acc += &FpVar::constant(fr_from_i128(w as i128)) * &prev[i];
                }
                if self.relu[l] {
                    acts.push(relu_gadget(cs.clone(), &acc, self.acc_bits)?);
                } else {
                    acts.push(acc);
                }
            }
            prev = acts;
        }

        // Public inputs #2: the claimed output, pinned to the final activations.
        for (o, act) in prev.iter().enumerate() {
            let y = FpVar::new_input(cs.clone(), || {
                self.output_q
                    .as_ref()
                    .map(|v| fr_from_i128(v[o]))
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;
            act.enforce_equal(&y)?;
        }
        Ok(())
    }
}

/// A model whose forward pass can be proven — holds the Groth16 proving key (weights
/// baked in) and the quantisation needed to prove and dequantise. The verifying key,
/// hashed by [`model_id`](Self::model_id), is the model's on-rail identity.
pub struct ProvenModel {
    pk: ProvingKey<Bn254>,
    vk_bytes: Vec<u8>,
    weights_q: Vec<Vec<Vec<i64>>>,
    biases_q: Vec<Vec<i128>>,
    relu: Vec<bool>,
    input_dim: usize,
    acc_bits: usize,
    scale: f64,
    output_scale: f64,
}

impl ProvenModel {
    /// Run the (per-model, in-process) Groth16 trusted setup over `model`'s quantised
    /// circuit. See the module docs on replacing this with a ceremony in production.
    pub fn setup<R: RngCore + CryptoRng>(
        model: &TracedMlp,
        cfg: SnarkConfig,
        rng: &mut R,
    ) -> Result<Self, ScemaDexError> {
        let (weights_q, biases_q, relu, output_scale) = quantize_model(model, cfg.scale);
        let circuit = MlpCircuit {
            weights_q: weights_q.clone(),
            biases_q: biases_q.clone(),
            relu: relu.clone(),
            input_dim: model.input_dim,
            acc_bits: cfg.acc_bits,
            input_q: None,
            output_q: None,
        };
        let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit, rng)
            .map_err(|e| ScemaDexError::Other(format!("groth16 setup: {e}")))?;
        let mut vk_bytes = Vec::new();
        vk.serialize_compressed(&mut vk_bytes)
            .map_err(|e| ScemaDexError::Other(format!("vk serialize: {e}")))?;
        Ok(Self {
            pk,
            vk_bytes,
            weights_q,
            biases_q,
            relu,
            input_dim: model.input_dim,
            acc_bits: cfg.acc_bits,
            scale: cfg.scale,
            output_scale,
        })
    }

    /// Prove that this model maps `input` to its forward output, succinctly and in
    /// zero-knowledge over the weights.
    pub fn prove<R: RngCore + CryptoRng>(
        &self,
        input: &[f64],
        rng: &mut R,
    ) -> Result<SnarkInferenceProof, ScemaDexError> {
        if input.len() != self.input_dim {
            return Err(ScemaDexError::Other(format!(
                "input width {} != model {}",
                input.len(),
                self.input_dim
            )));
        }
        let input_q = quantize_input(input, self.scale);
        let output_i128 = forward_i128(&self.weights_q, &self.biases_q, &self.relu, &input_q);
        // Public outputs are stored as i64 for a serde-friendly proof; the honest
        // range (bounded by acc_bits and the output scale) fits comfortably.
        let output_q: Vec<i64> = output_i128
            .iter()
            .map(|&y| {
                i64::try_from(y)
                    .map_err(|_| ScemaDexError::Other("output exceeds i64 — lower scale".into()))
            })
            .collect::<Result<_, _>>()?;
        let circuit = MlpCircuit {
            weights_q: self.weights_q.clone(),
            biases_q: self.biases_q.clone(),
            relu: self.relu.clone(),
            input_dim: self.input_dim,
            acc_bits: self.acc_bits,
            input_q: Some(input_q),
            output_q: Some(output_i128),
        };
        let proof = Groth16::<Bn254>::prove(&self.pk, circuit, rng)
            .map_err(|e| ScemaDexError::Other(format!("groth16 prove: {e}")))?;
        let mut proof_bytes = Vec::new();
        proof
            .serialize_compressed(&mut proof_bytes)
            .map_err(|e| ScemaDexError::Other(format!("proof serialize: {e}")))?;
        let output: Vec<f64> = output_q.iter().map(|&y| y as f64 / self.output_scale).collect();
        Ok(SnarkInferenceProof {
            proof_bytes,
            vk_bytes: self.vk_bytes.clone(),
            scale: self.scale,
            input_dim: self.input_dim,
            output_q,
            output,
        })
    }

    /// The model's identity: SHA-256 of its verifying key. A relying party pins this to
    /// know a proof concerns *this* model's exact weights.
    pub fn model_id(&self) -> [u8; 32] {
        Sha256::digest(&self.vk_bytes).into()
    }
}

/// A constant-size Groth16 proof that a committed model produced [`output`](Self::output)
/// on some input, verifiable without the weights. Implements
/// [`InferenceProof`](crate::zkproof::InferenceProof).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnarkInferenceProof {
    /// Groth16 proof (compressed BN254 group elements).
    pub proof_bytes: Vec<u8>,
    /// Verifying key the proof checks against; its hash is the model id.
    pub vk_bytes: Vec<u8>,
    /// Fixed-point scale used to quantise the input (verifier re-applies it).
    pub scale: f64,
    /// Expected input width.
    pub input_dim: usize,
    /// Quantised public output (the circuit's public inputs, output half).
    pub output_q: Vec<i64>,
    /// Dequantised claimed output.
    pub output: Vec<f64>,
}

impl SnarkInferenceProof {
    /// The committed model's identity (SHA-256 of the verifying key). Pin this to bind
    /// a proof to a registered model.
    pub fn model_id(&self) -> [u8; 32] {
        Sha256::digest(&self.vk_bytes).into()
    }

    fn verify_groth16(&self, input: &[f64]) -> bool {
        if input.len() != self.input_dim {
            return false;
        }
        let vk = match VerifyingKey::<Bn254>::deserialize_compressed(&self.vk_bytes[..]) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let proof = match Proof::<Bn254>::deserialize_compressed(&self.proof_bytes[..]) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let input_q = quantize_input(input, self.scale);
        let mut public = Vec::with_capacity(self.input_dim + self.output_q.len());
        for &x in &input_q {
            public.push(fr_from_i128(x as i128));
        }
        for &y in &self.output_q {
            public.push(fr_from_i128(y as i128));
        }
        Groth16::<Bn254>::verify(&vk, &public, &proof).unwrap_or(false)
    }
}

impl InferenceProof for SnarkInferenceProof {
    fn claimed_output(&self) -> &[f64] {
        &self.output
    }

    fn soundness(&self) -> usize {
        SNARK_SOUNDNESS
    }

    fn verify_inference(&self, input: &[f64], min_soundness: usize) -> bool {
        min_soundness <= SNARK_SOUNDNESS && self.verify_groth16(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zkbackend::DenseLayer;
    use ark_std::rand::{rngs::StdRng, SeedableRng};

    /// A deterministic CSPRNG for the in-process trusted setup (StdRng is CryptoRng).
    fn test_rng() -> StdRng {
        StdRng::seed_from_u64(0x5CE_11A)
    }

    /// 3 → 4 ReLU → 2 linear: a toy Q-head, same shape as the transparent backend's.
    fn net() -> TracedMlp {
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

    #[test]
    fn honest_proof_verifies() {
        let mut rng = test_rng();
        let pm = ProvenModel::setup(&net(), SnarkConfig::default(), &mut rng).unwrap();
        let input = [0.7, -1.2, 0.4];
        let proof = pm.prove(&input, &mut rng).unwrap();
        assert!(proof.verify_inference(&input, SNARK_SOUNDNESS));
        assert_eq!(proof.output.len(), 2);
        // Constant-size proof: three BN254 group elements, ~200 bytes, regardless of net.
        assert!(proof.proof_bytes.len() < 256);
    }

    #[test]
    fn dequantised_output_tracks_a_float_forward_pass() {
        let mut rng = test_rng();
        let pm = ProvenModel::setup(&net(), SnarkConfig::default(), &mut rng).unwrap();
        let input = [0.7, -1.2, 0.4];
        let proof = pm.prove(&input, &mut rng).unwrap();

        // Reference float forward pass (ReLU hidden, linear head).
        let m = net();
        let mut prev = input.to_vec();
        for layer in &m.layers {
            let mut acts = Vec::new();
            for (o, row) in layer.weights.iter().enumerate() {
                let mut acc = layer.biases[o];
                for (i, &w) in row.iter().enumerate() {
                    acc += w * prev[i];
                }
                if layer.relu && acc < 0.0 {
                    acc = 0.0;
                }
                acts.push(acc);
            }
            prev = acts;
        }
        for (got, want) in proof.output.iter().zip(prev.iter()) {
            assert!((got - want).abs() < 1e-2, "snark {got} vs float {want}");
        }
    }

    #[test]
    fn wrong_input_is_rejected() {
        let mut rng = test_rng();
        let pm = ProvenModel::setup(&net(), SnarkConfig::default(), &mut rng).unwrap();
        let proof = pm.prove(&[0.7, -1.2, 0.4], &mut rng).unwrap();
        assert!(!proof.verify_inference(&[0.7, -1.2, 0.5], SNARK_SOUNDNESS));
    }

    #[test]
    fn tampered_output_is_rejected() {
        let mut rng = test_rng();
        let pm = ProvenModel::setup(&net(), SnarkConfig::default(), &mut rng).unwrap();
        let input = [0.7, -1.2, 0.4];
        let mut proof = pm.prove(&input, &mut rng).unwrap();
        proof.output_q[0] += 1; // claim a different inference than the proof attests
        assert!(!proof.verify_inference(&input, SNARK_SOUNDNESS));
    }

    #[test]
    fn soundness_floor_above_snark_is_refused() {
        let mut rng = test_rng();
        let pm = ProvenModel::setup(&net(), SnarkConfig::default(), &mut rng).unwrap();
        let input = [0.7, -1.2, 0.4];
        let proof = pm.prove(&input, &mut rng).unwrap();
        assert!(proof.verify_inference(&input, SNARK_SOUNDNESS));
        assert!(!proof.verify_inference(&input, SNARK_SOUNDNESS + 1));
    }

    #[test]
    fn model_id_is_the_vk_hash_and_stable() {
        let mut rng = test_rng();
        let pm = ProvenModel::setup(&net(), SnarkConfig::default(), &mut rng).unwrap();
        let input = [0.1, 0.2, 0.3];
        let proof = pm.prove(&input, &mut rng).unwrap();
        assert_eq!(pm.model_id(), proof.model_id());
    }

    #[test]
    fn proof_is_usable_through_the_trait_object() {
        let mut rng = test_rng();
        let pm = ProvenModel::setup(&net(), SnarkConfig::default(), &mut rng).unwrap();
        let input = [0.2, 0.9, -0.3];
        let proof = pm.prove(&input, &mut rng).unwrap();
        let dyn_proof: &dyn InferenceProof = &proof;
        assert!(dyn_proof.verify_inference(&input, 64));
        assert_eq!(dyn_proof.soundness(), SNARK_SOUNDNESS);
        assert_eq!(dyn_proof.claimed_output().len(), 2);
    }
}
