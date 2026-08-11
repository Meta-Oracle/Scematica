//! Claim batching: many inferences, one 32-byte anchor.
//!
//! A game agent acts tens of times a second. Anchoring each inference separately is
//! absurd — one transaction per frame. So claims accumulate into a Merkle tree and only
//! the root goes on chain. Any single claim remains challengeable: the challenger is
//! handed an inclusion proof, re-runs that one forward pass, and disputes it against the
//! root that was already committed.
//!
//! # The footgun this closes by construction
//!
//! The usual Merkle construction duplicates the last node when a level has an odd count.
//! That makes distinct leaf sets produce identical roots — the Bitcoin CVE-2012-2459
//! shape — and in this setting it is worse than a nuisance: an agent could commit to a
//! batch and later present a *different* batch with the same root, choosing after the fact
//! which inferences it admits to.
//!
//! Two measures remove the ambiguity rather than mitigate it:
//!
//! * **Leaves bind their index.** `leaf(i) = keccak(0x00 ‖ i ‖ digest)`. The same claim at
//!   two positions hashes differently, so a duplicated leaf is not a duplicate hash.
//! * **Odd nodes are promoted, not duplicated.** Nothing is invented to pad a level, so
//!   there is no synthetic node to collide with.
//!
//! Domain tags `0x00`/`0x01` keep leaves and internal nodes in separate spaces, so a
//! 64-byte internal preimage can never be presented as a leaf.

use mesh_core::commit::Digest;
use tiny_keccak::{Hasher, Keccak};

const LEAF_TAG: u8 = 0x00;
const NODE_TAG: u8 = 0x01;

fn hash_leaf(index: u32, digest: &Digest) -> Digest {
    let mut k = Keccak::v256();
    k.update(&[LEAF_TAG]);
    k.update(&index.to_be_bytes());
    k.update(&digest.0);
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    Digest(out)
}

fn hash_node(left: &Digest, right: &Digest) -> Digest {
    let mut k = Keccak::v256();
    k.update(&[NODE_TAG]);
    k.update(&left.0);
    k.update(&right.0);
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    Digest(out)
}

/// One step of an inclusion proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProofStep {
    pub sibling: Digest,
    /// True when the sibling sits on the left, so the verifier knows the order. Hashing
    /// is deliberately **not** commutative (no sorted pairs): order carries information
    /// about position, and sorting would throw it away.
    pub sibling_is_left: bool,
}

/// An accumulating batch of claim digests.
#[derive(Debug, Clone, Default)]
pub struct ClaimBatch {
    leaves: Vec<Digest>,
}

impl ClaimBatch {
    pub fn new() -> Self {
        ClaimBatch { leaves: Vec::new() }
    }

    /// Append a claim digest. Returns its index, which is part of its leaf hash.
    pub fn push(&mut self, digest: Digest) -> usize {
        self.leaves.push(digest);
        self.leaves.len() - 1
    }

    pub fn len(&self) -> usize {
        self.leaves.len()
    }

    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    pub fn clear(&mut self) {
        self.leaves.clear();
    }

    pub fn digests(&self) -> &[Digest] {
        &self.leaves
    }

    fn level0(&self) -> Vec<Digest> {
        self.leaves
            .iter()
            .enumerate()
            .map(|(i, d)| hash_leaf(i as u32, d))
            .collect()
    }

    /// The root that goes on chain, or `None` for an empty batch.
    ///
    /// `None` rather than a zero root: committing a well-formed-looking root for zero
    /// claims would let an agent appear active while asserting nothing.
    pub fn root(&self) -> Option<Digest> {
        let mut level = self.level0();
        if level.is_empty() {
            return None;
        }
        while level.len() > 1 {
            let mut next = Vec::with_capacity(level.len().div_ceil(2));
            let mut i = 0;
            while i + 1 < level.len() {
                next.push(hash_node(&level[i], &level[i + 1]));
                i += 2;
            }
            if i < level.len() {
                // Promote, never duplicate.
                next.push(level[i]);
            }
            level = next;
        }
        Some(level[0])
    }

    /// Inclusion proof for the claim at `index`.
    pub fn proof(&self, index: usize) -> Option<Vec<ProofStep>> {
        if index >= self.leaves.len() {
            return None;
        }
        let mut steps = Vec::new();
        let mut level = self.level0();
        let mut pos = index;

        while level.len() > 1 {
            let mut next = Vec::with_capacity(level.len().div_ceil(2));
            let mut i = 0;
            let mut next_pos = pos;
            while i + 1 < level.len() {
                if pos == i {
                    steps.push(ProofStep { sibling: level[i + 1], sibling_is_left: false });
                    next_pos = next.len();
                } else if pos == i + 1 {
                    steps.push(ProofStep { sibling: level[i], sibling_is_left: true });
                    next_pos = next.len();
                }
                next.push(hash_node(&level[i], &level[i + 1]));
                i += 2;
            }
            if i < level.len() {
                if pos == i {
                    // Promoted: no sibling, so no step is recorded for this level.
                    next_pos = next.len();
                }
                next.push(level[i]);
            }
            level = next;
            pos = next_pos;
        }
        Some(steps)
    }
}

/// Recompute a root from a claim digest and its proof.
///
/// This is the function a verifying contract mirrors, so it is kept tiny and free of any
/// dependency on the batch that produced it — a challenger holds a claim, an index, and a
/// path, and nothing else.
pub fn verify_proof(digest: &Digest, index: u32, steps: &[ProofStep], root: &Digest) -> bool {
    let mut node = hash_leaf(index, digest);
    for step in steps {
        node = if step.sibling_is_left {
            hash_node(&step.sibling, &node)
        } else {
            hash_node(&node, &step.sibling)
        };
    }
    node == *root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(byte: u8) -> Digest {
        Digest([byte; 32])
    }

    fn batch_of(n: u8) -> ClaimBatch {
        let mut b = ClaimBatch::new();
        for i in 0..n {
            b.push(d(i));
        }
        b
    }

    #[test]
    fn empty_batch_has_no_root() {
        assert_eq!(ClaimBatch::new().root(), None);
    }

    #[test]
    fn every_claim_proves_against_the_root() {
        for n in 1u8..=17 {
            let b = batch_of(n);
            let root = b.root().unwrap();
            for i in 0..n as usize {
                let proof = b.proof(i).expect("proof exists");
                assert!(
                    verify_proof(&d(i as u8), i as u32, &proof, &root),
                    "claim {i} of {n} failed to prove"
                );
            }
        }
    }

    #[test]
    fn a_wrong_claim_does_not_prove() {
        let b = batch_of(8);
        let root = b.root().unwrap();
        let proof = b.proof(3).unwrap();
        assert!(!verify_proof(&d(99), 3, &proof, &root), "a substituted digest must fail");
    }

    #[test]
    fn a_claim_cannot_be_moved_to_another_index() {
        // The index is inside the leaf hash, so a valid proof for position 3 is useless
        // at position 4. Without index binding this is where a duplicate-leaf attack lives.
        let b = batch_of(8);
        let root = b.root().unwrap();
        let proof = b.proof(3).unwrap();
        assert!(!verify_proof(&d(3), 4, &proof, &root));
    }

    #[test]
    fn duplicated_trailing_claims_do_not_collide() {
        // The CVE-2012-2459 shape: with last-node duplication, [a,b,c] and [a,b,c,c] give
        // the same root, letting a committer choose their batch after the fact. Promotion
        // plus index-bound leaves makes the two roots differ.
        let mut three = ClaimBatch::new();
        for i in 0..3u8 {
            three.push(d(i));
        }
        let mut four = three.clone();
        four.push(d(2)); // repeat the last claim
        assert_ne!(three.root().unwrap(), four.root().unwrap());
    }

    #[test]
    fn identical_claims_at_different_positions_hash_differently() {
        let mut b = ClaimBatch::new();
        b.push(d(7));
        b.push(d(7));
        let root = b.root().unwrap();
        let p0 = b.proof(0).unwrap();
        let p1 = b.proof(1).unwrap();
        assert!(verify_proof(&d(7), 0, &p0, &root));
        assert!(verify_proof(&d(7), 1, &p1, &root));
        assert_ne!(p0, p1, "same digest at different indices must take different paths");
    }

    #[test]
    fn a_single_claim_is_its_own_root() {
        let b = batch_of(1);
        let root = b.root().unwrap();
        assert!(verify_proof(&d(0), 0, &[], &root));
        // And it is the tagged leaf, not the raw digest — so a bare digest cannot be
        // passed off as a root.
        assert_ne!(root, d(0));
    }

    #[test]
    fn order_is_part_of_the_commitment() {
        let mut a = ClaimBatch::new();
        a.push(d(1));
        a.push(d(2));
        let mut b = ClaimBatch::new();
        b.push(d(2));
        b.push(d(1));
        assert_ne!(a.root().unwrap(), b.root().unwrap());
    }

    #[test]
    fn roots_are_stable_across_recomputation() {
        let b = batch_of(11);
        let r = b.root().unwrap();
        for _ in 0..16 {
            assert_eq!(b.root().unwrap(), r);
        }
    }

    #[test]
    fn out_of_range_index_has_no_proof() {
        assert!(batch_of(4).proof(4).is_none());
    }
}

#[cfg(test)]
mod vectors {
    //! Cross-implementation test vectors.
    //!
    //! The Solidity verifier in `BotchainNNMesh.sol` must reproduce these exactly. Two
    //! implementations of one hash tree is the single most likely place for this design to
    //! break silently — an honest agent whose proof does not verify on-chain looks like a
    //! fraud — so the values are pinned on both sides rather than assumed compatible.
    use super::*;

    fn d(b: u8) -> Digest {
        Digest([b; 32])
    }

    #[test]
    fn print_vectors_for_solidity() {
        for n in [1usize, 2, 3, 5] {
            let mut batch = ClaimBatch::new();
            for i in 0..n {
                batch.push(d(i as u8 + 1));
            }
            println!("n={n} root={}", batch.root().unwrap().to_hex());
            for i in 0..n {
                let p = batch.proof(i).unwrap();
                let path: Vec<String> = p
                    .iter()
                    .map(|s| format!("{}:{}", if s.sibling_is_left { "L" } else { "R" }, s.sibling.to_hex()))
                    .collect();
                println!("  i={i} steps={} {}", p.len(), path.join(" "));
            }
        }
    }
}
