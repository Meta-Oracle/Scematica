//! Batching sealed records into one root, so a decision can be pinned somewhere its author
//! does not control.
//!
//! `scema verify` proves a record was not edited after sealing. It says so, and it says what
//! it does *not* prove: that this is the original record. Tamper-**evident**, not
//! tamper-proof — because somebody holding the only copy can seal a different one and the
//! commitment will be perfectly valid. The missing ingredient has always been the same:
//! *until the root is anchored somewhere the author does not control.*
//!
//! This is the batching half of that. It takes the roots of sealed records, builds one
//! Merkle root over them, and issues an [`InclusionProof`] per record. A third party holding
//! one record and one proof can check membership without the batch, without the other
//! records, and without trusting whoever produced them.
//!
//! ## Why SHA-256, and why that is not a compromise
//!
//! `mesh-attest` in the BOT Chain workspace uses keccak-256, and the obvious instinct is to
//! match it so the two can share a verifier. That instinct is wrong here and the cost of
//! following it would be enormous: Omni's commitments are SHA-256, so changing the hash
//! would mean **every record already sealed on disk stops verifying**. A verifier that
//! rejects untouched history is the one failure that teaches a reader to stop believing it.
//!
//! The reason it is not a compromise is that EVM exposes SHA-256 as precompile `0x02`. A
//! Solidity contract can verify one of these proofs directly, cheaply, without Omni changing
//! anything. The hash is recorded in the batch rather than assumed, so a future batch on a
//! different algorithm is a different, clearly-labelled artefact instead of a silent
//! reinterpretation of this one.
//!
//! ## Two details that are easy to get wrong and expensive to get wrong
//!
//! **Leaves and internal nodes are domain-separated.** A leaf is `H(0x00 ‖ bytes)` and an
//! internal node is `H(0x01 ‖ left ‖ right)`. Without the tags an attacker can present an
//! internal node as if it were a leaf, and prove membership of something that was never
//! submitted.
//!
//! **An odd node is promoted, never duplicated.** Duplicating the last node to pad a level
//! is the widespread implementation and it lets two different leaf sets produce the same
//! root — the Bitcoin CVE-2012-2459 shape. Promotion carries the odd node up untouched, and
//! the tree stays injective.
//!
//! ## What an anchor is, and what an empty list means
//!
//! [`Batch::anchors`] is a list, because the plan is to anchor to more than one chain: one
//! whose economics we control and one with an audience. Each entry is independently
//! checkable, so a batch anchored twice is stronger than one anchored once and a batch
//! anchored zero times is **honestly unanchored** rather than quietly presented as sealed.
//!
//! Nothing in this crate talks to a chain. Submitting the root is a network act with a key
//! behind it; recording an anchor that was never submitted would be exactly the fabrication
//! the rest of this runtime exists to refuse.

use scema_verify::canonical::Digest;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Tag prepended to a leaf preimage.
const LEAF_TAG: u8 = 0x00;
/// Tag prepended to an internal-node preimage.
const NODE_TAG: u8 = 0x01;

/// The hash this build produces trees with.
pub const ALGORITHM: &str = "sha256";

fn hash_leaf(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([LEAF_TAG]);
    h.update(bytes);
    h.finalize().into()
}

fn hash_node(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([NODE_TAG]);
    h.update(left);
    h.update(right);
    h.finalize().into()
}

/// Which side a sibling sits on.
///
/// Recorded rather than derived from an index, because a verifier is given the proof and the
/// leaf and nothing else — it does not know where in the tree the leaf sat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
}

/// One step up the tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Hex of the sibling hash.
    pub sibling: String,
    /// Which side the *sibling* is on.
    pub side: Side,
}

/// Everything needed to prove one leaf is in one root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InclusionProof {
    /// The record root this proves membership of, as hex.
    pub leaf: String,
    pub steps: Vec<Step>,
    /// The algorithm the tree was built with. Checked, never assumed.
    pub algorithm: String,
}

fn from_hex(s: &str) -> Option<[u8; 32]> {
    let s = s.trim();
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn to_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A Merkle tree over record roots.
#[derive(Clone, Debug)]
pub struct Tree {
    /// Level 0 is the hashed leaves; the last level is the single root.
    levels: Vec<Vec<[u8; 32]>>,
    leaves: Vec<String>,
}

impl Tree {
    /// Build a tree over record roots, given as hex.
    ///
    /// The order is the caller's and is preserved — a proof names an index implicitly by its
    /// sibling path, so re-sorting the leaves would invalidate every proof already issued.
    ///
    /// `None` for an empty set. A tree with no leaves has no root, and inventing one (the
    /// hash of nothing, say) would produce an artefact that looks anchorable and attests to
    /// nothing.
    pub fn build(leaves: &[String]) -> Option<Tree> {
        if leaves.is_empty() {
            return None;
        }
        let mut level: Vec<[u8; 32]> = Vec::with_capacity(leaves.len());
        for l in leaves {
            // A leaf that is not a 32-byte hex digest is refused rather than coerced: this
            // tree's whole value is that its leaves are commitments somebody else can
            // recompute.
            let raw = from_hex(l)?;
            level.push(hash_leaf(&raw));
        }

        let mut levels = vec![level];
        while levels.last()?.len() > 1 {
            let prev = levels.last()?;
            let mut next = Vec::with_capacity(prev.len().div_ceil(2));
            let mut i = 0;
            while i + 1 < prev.len() {
                next.push(hash_node(&prev[i], &prev[i + 1]));
                i += 2;
            }
            if i < prev.len() {
                // Promoted, not duplicated. Duplicating the last node lets two different
                // leaf sets produce one root — the CVE-2012-2459 shape.
                next.push(prev[i]);
            }
            levels.push(next);
        }
        Some(Tree { levels, leaves: leaves.to_vec() })
    }

    pub fn root(&self) -> String {
        to_hex(&self.levels[self.levels.len() - 1][0])
    }

    pub fn len(&self) -> usize {
        self.leaves.len()
    }

    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    /// A proof for the leaf at `index`.
    pub fn proof(&self, index: usize) -> Option<InclusionProof> {
        if index >= self.leaves.len() {
            return None;
        }
        let mut steps = Vec::new();
        let mut idx = index;
        for level in &self.levels[..self.levels.len() - 1] {
            // An odd node promoted to the next level has no sibling at this one, and
            // therefore contributes no step. Emitting one would make the proof fail.
            if idx ^ 1 < level.len() {
                let sibling = level[idx ^ 1];
                steps.push(Step {
                    sibling: to_hex(&sibling),
                    side: if idx.is_multiple_of(2) { Side::Right } else { Side::Left },
                });
            }
            idx /= 2;
        }
        Some(InclusionProof {
            leaf: self.leaves[index].clone(),
            steps,
            algorithm: ALGORITHM.to_string(),
        })
    }

    /// A proof for a leaf given by its hex value.
    pub fn proof_for(&self, leaf: &str) -> Option<InclusionProof> {
        let idx = self.leaves.iter().position(|l| l.eq_ignore_ascii_case(leaf))?;
        self.proof(idx)
    }
}

/// Check a proof against a root.
///
/// Refuses an unrecognised algorithm rather than assuming this build's. A proof produced by
/// a future keccak batch would otherwise verify against nothing and report a tampered
/// record, which is the one failure that teaches a reader to stop believing the verifier.
pub fn verify_inclusion(proof: &InclusionProof, root: &str) -> bool {
    if proof.algorithm != ALGORITHM {
        return false;
    }
    let Some(raw) = from_hex(&proof.leaf) else { return false };
    let mut acc = hash_leaf(&raw);
    for step in &proof.steps {
        let Some(sib) = from_hex(&step.sibling) else { return false };
        acc = match step.side {
            Side::Right => hash_node(&acc, &sib),
            Side::Left => hash_node(&sib, &acc),
        };
    }
    match from_hex(root) {
        Some(r) => acc == r,
        None => false,
    }
}

/// Where a root was published.
///
/// `reference` is deliberately opaque — a transaction hash, a block explorer URL, an IPFS
/// CID. This crate does not know what chains exist and should not acquire opinions about
/// them; a reader follows the reference and checks for themselves, which is the entire
/// point of anchoring.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    /// A name a reader can act on: `bot-chain`, `base`, `ethereum`.
    pub chain: String,
    /// How to find it there.
    pub reference: String,
    /// Unix seconds the anchor was recorded.
    pub at: i64,
}

/// A set of record roots, batched under one Merkle root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batch {
    pub root: String,
    pub algorithm: String,
    /// The record roots covered, in tree order.
    pub leaves: Vec<String>,
    /// Unix seconds the batch was built.
    pub at: i64,
    /// Where this root has been published.
    ///
    /// Empty means **not anchored**, which is a true and useful statement — not a defect to
    /// be hidden. A batch that has been built but not published proves only what
    /// `scema verify` already proved.
    #[serde(default)]
    pub anchors: Vec<Anchor>,
}

impl Batch {
    pub fn build(leaves: &[String], at: i64) -> Option<Batch> {
        let tree = Tree::build(leaves)?;
        Some(Batch {
            root: tree.root(),
            algorithm: ALGORITHM.to_string(),
            leaves: leaves.to_vec(),
            at,
            anchors: Vec::new(),
        })
    }

    /// Has this been published anywhere?
    ///
    /// The question every reader of a batch actually has, and the one a boolean `anchored`
    /// field would answer badly — the interesting part is *where*, and how many.
    pub fn is_anchored(&self) -> bool {
        !self.anchors.is_empty()
    }

    /// Rebuild the tree to issue proofs.
    pub fn tree(&self) -> Option<Tree> {
        Tree::build(&self.leaves)
    }

    /// Does the stored root still match the stored leaves?
    ///
    /// A batch is a file somebody can edit. Adding a leaf without recomputing the root would
    /// otherwise let a record be claimed as covered by an anchor that never included it.
    pub fn root_matches_leaves(&self) -> bool {
        match Tree::build(&self.leaves) {
            Some(t) => t.root() == self.root && self.algorithm == ALGORITHM,
            None => false,
        }
    }
}

/// Convert a `scema-verify` digest to the hex form used as a leaf.
pub fn leaf_of(digest: Digest) -> String {
    digest.to_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaves(n: usize) -> Vec<String> {
        (0..n).map(|i| to_hex(&hash_leaf(&[i as u8]))).collect()
    }

    #[test]
    fn an_empty_set_has_no_root_rather_than_a_root_of_nothing() {
        // Inventing one would produce an artefact that looks anchorable and attests to
        // nothing.
        assert!(Tree::build(&[]).is_none());
        assert!(Batch::build(&[], 0).is_none());
    }

    #[test]
    fn every_leaf_proves_against_the_root() {
        for n in [1usize, 2, 3, 4, 5, 7, 8, 9, 16, 33] {
            let ls = leaves(n);
            let t = Tree::build(&ls).unwrap();
            let root = t.root();
            for i in 0..n {
                let p = t.proof(i).unwrap();
                assert!(verify_inclusion(&p, &root), "n={n} i={i}");
            }
        }
    }

    #[test]
    fn a_leaf_that_is_not_in_the_tree_does_not_prove() {
        let t = Tree::build(&leaves(8)).unwrap();
        let mut p = t.proof(3).unwrap();
        p.leaf = to_hex(&hash_leaf(b"not submitted"));
        assert!(!verify_inclusion(&p, &t.root()));
    }

    #[test]
    fn an_altered_step_does_not_prove() {
        let t = Tree::build(&leaves(8)).unwrap();
        let mut p = t.proof(3).unwrap();
        p.steps[0].sibling = to_hex(&[0u8; 32]);
        assert!(!verify_inclusion(&p, &t.root()));
    }

    #[test]
    fn flipping_a_side_does_not_prove() {
        // Side is recorded rather than derived, so it is data a forger controls — and
        // hashing is order-sensitive precisely so that flipping it fails.
        let t = Tree::build(&leaves(8)).unwrap();
        let mut p = t.proof(3).unwrap();
        p.steps[0].side = match p.steps[0].side {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        };
        assert!(!verify_inclusion(&p, &t.root()));
    }

    #[test]
    fn an_internal_node_cannot_be_passed_off_as_a_leaf() {
        // The reason leaves and nodes are domain-separated. Without the tags, an internal
        // node is a valid leaf preimage and membership can be proven for something that was
        // never submitted.
        let ls = leaves(4);
        let t = Tree::build(&ls).unwrap();
        let internal = to_hex(&t.levels[1][0]);
        let forged = InclusionProof {
            leaf: internal,
            steps: vec![Step { sibling: to_hex(&t.levels[1][1]), side: Side::Right }],
            algorithm: ALGORITHM.into(),
        };
        assert!(!verify_inclusion(&forged, &t.root()));
    }

    #[test]
    fn an_odd_node_is_promoted_so_two_leaf_sets_cannot_share_a_root() {
        // Duplicating the last node to pad a level is the widespread implementation and it
        // is the CVE-2012-2459 shape: [a,b,c] and [a,b,c,c] then produce the same root, so
        // a batch can be presented as covering a record it never covered.
        let three = leaves(3);
        let mut four = three.clone();
        four.push(three[2].clone());
        let a = Tree::build(&three).unwrap().root();
        let b = Tree::build(&four).unwrap().root();
        assert_ne!(a, b, "padding by duplication would make these equal");
    }

    #[test]
    fn leaf_order_is_preserved_because_proofs_depend_on_it() {
        let ls = leaves(4);
        let mut reordered = ls.clone();
        reordered.swap(0, 3);
        assert_ne!(Tree::build(&ls).unwrap().root(), Tree::build(&reordered).unwrap().root());
    }

    #[test]
    fn a_proof_from_another_algorithm_is_refused_rather_than_failed_open() {
        let t = Tree::build(&leaves(4)).unwrap();
        let mut p = t.proof(0).unwrap();
        p.algorithm = "keccak256".into();
        assert!(!verify_inclusion(&p, &t.root()));
    }

    #[test]
    fn a_malformed_leaf_is_refused_rather_than_coerced() {
        assert!(Tree::build(&["not hex".to_string()]).is_none());
        assert!(Tree::build(&["abcd".to_string()]).is_none());
    }

    #[test]
    fn an_unanchored_batch_says_so_rather_than_hiding_it() {
        // Empty is a true statement. A batch built but not published proves only what
        // `scema verify` already proved.
        let b = Batch::build(&leaves(3), 0).unwrap();
        assert!(!b.is_anchored());
        assert!(b.anchors.is_empty());
    }

    #[test]
    fn a_batch_can_carry_more_than_one_anchor() {
        // The plan is a chain whose economics we control and one with an audience. Each is
        // independently checkable, so two is stronger than one.
        let mut b = Batch::build(&leaves(3), 0).unwrap();
        b.anchors.push(Anchor {
            chain: "bot-chain".into(),
            reference: "0xaaa".into(),
            at: 1,
        });
        b.anchors.push(Anchor { chain: "base".into(), reference: "0xbbb".into(), at: 2 });
        assert!(b.is_anchored());
        assert_eq!(b.anchors.len(), 2);
    }

    #[test]
    fn adding_a_leaf_without_recomputing_the_root_is_caught() {
        // A batch is a file somebody can edit, and this is the edit that would let a record
        // be claimed as covered by an anchor that never included it.
        let mut b = Batch::build(&leaves(3), 0).unwrap();
        assert!(b.root_matches_leaves());
        b.leaves.push(to_hex(&hash_leaf(b"snuck in")));
        assert!(!b.root_matches_leaves());
    }

    #[test]
    fn a_batch_issues_a_proof_for_a_leaf_by_value() {
        let ls = leaves(5);
        let b = Batch::build(&ls, 0).unwrap();
        let t = b.tree().unwrap();
        let p = t.proof_for(&ls[2]).unwrap();
        assert!(verify_inclusion(&p, &b.root));
        assert!(t.proof_for("00").is_none());
    }

    #[test]
    fn a_single_leaf_tree_has_an_empty_proof_that_still_verifies() {
        let ls = leaves(1);
        let t = Tree::build(&ls).unwrap();
        let p = t.proof(0).unwrap();
        assert!(p.steps.is_empty());
        assert!(verify_inclusion(&p, &t.root()));
    }
}
