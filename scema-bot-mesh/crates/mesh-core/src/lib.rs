//! `mesh-core` — deterministic neural inference with on-chain-checkable commitments.
//!
//! The foundation of the BOT Chain neural mesh. Three layers, each built on the one below:
//!
//! 1. [`fixed`] — Q16.16 integer arithmetic. No floats anywhere in the inference path.
//! 2. [`net`] — the policy network and its equations, written against that arithmetic.
//! 3. [`commit`] — keccak256 commitments so a claim about an inference can be disputed.
//!
//! # What this is for
//!
//! A game embeds an agent. The agent acts. Somebody else — a player, an opponent, a
//! contract holding a bond — wants to know the agent actually ran the policy it says it
//! ran, and did not simply pick the outcome that suited it.
//!
//! Making that checkable is a determinism problem before it is a cryptography problem.
//! Committing to a hash is easy; the hard part is that the challenger's re-run must
//! produce *the same bits*. That is why the bottom layer is integer arithmetic and not a
//! float tensor library, and why the ordering of a sum and the direction of a rounding tie
//! are specification rather than implementation detail.
//!
//! ```
//! use mesh_core::{fixed::Fx, net::PolicyNet, commit::{InferenceClaim, Verdict}};
//!
//! let net = PolicyNet::new(4, &[8], 3);
//! let state = [Fx::from_f64(0.5), Fx::ZERO, Fx::from_f64(-1.25), Fx::ONE];
//!
//! // The agent acts and commits to what it did.
//! let claim = InferenceClaim::produce(&net, &state).unwrap();
//! let onchain = claim.digest();               // 32 bytes -> BOT Chain
//!
//! // Anyone holding the weights can re-run it and get the same bits.
//! assert_eq!(claim.verify(&net), Verdict::Valid);
//! assert_eq!(onchain, claim.digest());
//! ```
//!
//! # Deliberately not here
//!
//! **Training.** Gradient descent is float-friendly and does not need to be reproducible —
//! only the resulting weights do, and those are committed by hash. Train wherever you
//! like, in whatever framework, then quantise into [`fixed::Fx`] at the boundary via
//! [`fixed::Fx::from_f64`]. Only the *forward* pass must be deterministic.
//!
//! **A network stack.** "Mesh" here means agents referring to each other's commitments,
//! not a peer-to-peer transport. Transport belongs to the host application.

#![forbid(unsafe_code)]

pub mod commit;
pub mod fixed;
pub mod net;

pub use commit::{Digest, InferenceClaim, Verdict};
pub use fixed::Fx;
pub use net::{Activation, Layer, MeshError, PolicyNet};

#[cfg(test)]
mod integration {
    use super::*;

    /// The end-to-end property the whole design exists to provide: an agent's claim is
    /// checkable by a third party who holds only the weights and the claim.
    #[test]
    fn a_third_party_can_adjudicate_without_trusting_the_agent() {
        let mut net = PolicyNet::new(3, &[4], 2);
        for (i, w) in net.trunk[0].weights.iter_mut().enumerate() {
            *w = Fx::from_bits(2_000 * (i as i32 % 5 + 1));
        }
        net.advantage_head.weights[0] = Fx::ONE;
        net.value_head.weights[1] = Fx::ONE;

        let state = [Fx::from_f64(0.25), Fx::from_f64(-0.75), Fx::ONE];
        let honest = InferenceClaim::produce(&net, &state).unwrap();

        // An honest claim survives adjudication by someone who was not there.
        assert!(honest.verify(&net).is_valid());

        // A claim that reports a more convenient action does not.
        let mut lie = honest.clone();
        lie.action = 1 - honest.action;
        assert_eq!(lie.verify(&net), Verdict::ActionMismatch);
        assert!(lie.verify(&net).is_slashable());

        // And tampering is visible in the 32 bytes that went on-chain.
        assert_ne!(lie.digest(), honest.digest());
    }
}
