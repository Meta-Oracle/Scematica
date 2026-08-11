//! Commitments: turning a policy and an inference into something a contract can check.
//!
//! This is what separates a mesh from a folder of models. Weights are far too large to
//! live on-chain — but their *hash* is 32 bytes, and so is the hash of an inference. So
//! the split is:
//!
//! * **Off-chain**: the weights, the forward pass, the game loop. Fast, free, private.
//! * **On-chain**: `weights_hash`, and a claim of the form "given this input, this policy
//!   produced this action".
//!
//! A challenger who has the weights can re-run the forward pass and compare 32 bytes. If
//! they differ, the claim was false and the bond behind it can be slashed — which is what
//! [`ScemaBondEscrow`](../../../scema-botchain/contracts/src/ScemaBondEscrow.sol) is for.
//!
//! **keccak256, not SHA-256**, because that is what Solidity's `keccak256` computes. A
//! commitment hashed with anything else cannot be checked by a contract without shipping a
//! hash implementation in Solidity, which would cost more gas than the dispute is worth.
//!
//! # Why this is only worth doing with the fixed-point core
//!
//! The whole scheme rests on the challenger reproducing the result exactly. With float
//! inference the challenger's re-run differs in the last bits, the hashes differ, and an
//! honest claim looks fraudulent. Bit-exact integer arithmetic is the precondition, not an
//! optimisation — see [`crate::fixed`].

use tiny_keccak::{Hasher, Keccak};

use crate::fixed::Fx;
use crate::net::PolicyNet;

/// A 32-byte keccak256 digest, ABI-compatible with Solidity's `bytes32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Digest(pub [u8; 32]);

impl Digest {
    pub fn to_hex(self) -> String {
        let mut s = String::with_capacity(66);
        s.push_str("0x");
        for b in self.0 {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }
}

fn keccak(parts: &[&[u8]]) -> Digest {
    let mut hasher = Keccak::v256();
    for p in parts {
        hasher.update(p);
    }
    let mut out = [0u8; 32];
    hasher.finalize(&mut out);
    Digest(out)
}

/// Encode fixed-point values as big-endian `int32`.
///
/// Big-endian because that is how the EVM lays out words; a little-endian encoding would
/// force the verifying contract to byte-swap every value.
fn encode(values: &[Fx]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(values.len() * 4);
    for v in values {
        buf.extend_from_slice(&v.to_bits().to_be_bytes());
    }
    buf
}

/// Hash of a policy's parameters.
///
/// Domain-separated with a version tag. Without separation, a weights hash and an
/// inference hash could collide across contexts, and a challenger could substitute one
/// for the other. The version means a future change to the encoding or to `FRAC_BITS`
/// produces a visibly different commitment instead of a silently incompatible one.
pub fn weights_hash(net: &PolicyNet) -> Digest {
    let shape = [
        net.state_dim() as u32,
        net.trunk.len() as u32,
        net.action_count() as u32,
    ];
    let mut shape_bytes = Vec::with_capacity(12);
    for s in shape {
        shape_bytes.extend_from_slice(&s.to_be_bytes());
    }
    // Shape is hashed alongside the values so two nets with identical parameter vectors
    // but different layer widths cannot share a commitment.
    keccak(&[b"scema-bot-mesh/weights/v1", &shape_bytes, &encode(&net.parameters())])
}

/// A claim that a policy produced an action for an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceClaim {
    pub weights: Digest,
    pub input: Vec<Fx>,
    pub q_values: Vec<Fx>,
    pub action: usize,
}

impl InferenceClaim {
    /// Run the policy and record what it produced.
    pub fn produce(net: &PolicyNet, input: &[Fx]) -> Result<Self, crate::net::MeshError> {
        let q_values = net.q_values(input)?;
        let action = net.act(input)?;
        Ok(InferenceClaim { weights: weights_hash(net), input: input.to_vec(), q_values, action })
    }

    /// The 32 bytes that go on-chain.
    pub fn digest(&self) -> Digest {
        keccak(&[
            b"scema-bot-mesh/inference/v1",
            &self.weights.0,
            &encode(&self.input),
            &encode(&self.q_values),
            &(self.action as u32).to_be_bytes(),
        ])
    }

    /// Re-run the claim against a policy and report whether it holds.
    ///
    /// Returns the specific way it failed rather than a bool: "the weights are not the
    /// ones committed to" and "the weights are right but the output is wrong" are
    /// different accusations, and a dispute needs to say which.
    pub fn verify(&self, net: &PolicyNet) -> Verdict {
        if weights_hash(net) != self.weights {
            return Verdict::WrongWeights;
        }
        let q = match net.q_values(&self.input) {
            Ok(q) => q,
            Err(_) => return Verdict::Unrunnable,
        };
        if q != self.q_values {
            return Verdict::OutputMismatch;
        }
        match net.act(&self.input) {
            Ok(a) if a == self.action => Verdict::Valid,
            Ok(_) => Verdict::ActionMismatch,
            Err(_) => Verdict::Unrunnable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Valid,
    /// The policy supplied is not the one the claim committed to.
    WrongWeights,
    /// Right policy, different Q-values — the claimed inference did not happen.
    OutputMismatch,
    /// Q-values match but the chosen action does not follow from them.
    ActionMismatch,
    /// The input does not fit the network. A malformed claim, not a dishonest one.
    Unrunnable,
}

impl Verdict {
    pub fn is_valid(self) -> bool {
        matches!(self, Verdict::Valid)
    }
    /// Whether this verdict justifies slashing. A malformed claim is a bug, not fraud.
    pub fn is_slashable(self) -> bool {
        matches!(self, Verdict::WrongWeights | Verdict::OutputMismatch | Verdict::ActionMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::PolicyNet;

    fn net() -> PolicyNet {
        let mut n = PolicyNet::new(2, &[3], 2);
        for (i, w) in n.trunk[0].weights.iter_mut().enumerate() {
            *w = Fx::from_bits(1000 * (i as i32 + 1));
        }
        for (i, w) in n.advantage_head.weights.iter_mut().enumerate() {
            *w = Fx::from_bits(700 * (i as i32 + 1));
        }
        n.value_head.weights = vec![Fx::ONE, Fx::ONE, Fx::ONE];
        n
    }

    #[test]
    fn an_honest_claim_verifies() {
        let n = net();
        let claim = InferenceClaim::produce(&n, &[Fx::from_int(2), Fx::from_int(3)]).unwrap();
        assert_eq!(claim.verify(&n), Verdict::Valid);
        assert!(!claim.verify(&n).is_slashable());
    }

    #[test]
    fn a_tampered_output_is_caught_and_is_slashable() {
        let n = net();
        let mut claim = InferenceClaim::produce(&n, &[Fx::from_int(2), Fx::from_int(3)]).unwrap();
        claim.q_values[0] = claim.q_values[0].add(Fx::ONE);
        assert_eq!(claim.verify(&n), Verdict::OutputMismatch);
        assert!(claim.verify(&n).is_slashable());
    }

    #[test]
    fn substituting_different_weights_is_caught() {
        let n = net();
        let claim = InferenceClaim::produce(&n, &[Fx::from_int(1), Fx::from_int(1)]).unwrap();
        let mut other = n.clone();
        other.value_head.biases[0] = Fx::ONE;
        assert_eq!(claim.verify(&other), Verdict::WrongWeights);
    }

    #[test]
    fn a_malformed_claim_is_not_treated_as_fraud() {
        // Slashing someone for a shape bug would make honest integrators afraid to bond.
        let n = net();
        let claim = InferenceClaim { weights: weights_hash(&n), input: vec![Fx::ONE], q_values: vec![], action: 0 };
        assert_eq!(claim.verify(&n), Verdict::Unrunnable);
        assert!(!claim.verify(&n).is_slashable());
    }

    #[test]
    fn a_single_weight_change_moves_the_hash() {
        let a = net();
        let mut b = a.clone();
        b.trunk[0].weights[0] = b.trunk[0].weights[0].add(Fx::from_bits(1));
        assert_ne!(weights_hash(&a), weights_hash(&b));
    }

    #[test]
    fn shape_is_part_of_the_commitment() {
        // Two nets whose parameter vectors are both all-zero but whose shapes differ must
        // not share a commitment, or one could be substituted for the other.
        let a = PolicyNet::new(4, &[], 2); // 4*1+1 + 4*2+2 params
        let b = PolicyNet::new(2, &[], 4);
        assert_eq!(a.parameter_count(), b.parameter_count());
        assert_eq!(a.parameters(), b.parameters());
        assert_ne!(weights_hash(&a), weights_hash(&b), "shape must be bound into the hash");
    }

    #[test]
    fn domains_are_separated() {
        // A weights digest must never be usable as an inference digest.
        let n = net();
        let claim = InferenceClaim::produce(&n, &[Fx::ONE, Fx::ONE]).unwrap();
        assert_ne!(claim.digest(), claim.weights);
    }

    #[test]
    fn hashing_is_stable_across_runs() {
        let n = net();
        let h = weights_hash(&n);
        for _ in 0..32 {
            assert_eq!(weights_hash(&n), h);
        }
        assert_eq!(h.to_hex().len(), 66);
    }
}
