/-
  The anchor tree, and the shape of CVE-2012-2459.

  `scema-anchor` batches sealed record roots into one Merkle root with per-record inclusion
  proofs. Two decisions in that implementation are security properties rather than taste, and
  both are stated here as theorems:

  1. **Leaves and nodes are domain-separated** — `H(0x00 ‖ bytes)` for a leaf, `H(0x01 ‖ L ‖ R)`
     for a node. Without it an attacker can present an internal node as if it were a leaf.
  2. **An odd node is promoted, never duplicated.** Duplicating the last element is the
     Bitcoin construction, and it is why two different transaction lists can produce the same
     root — CVE-2012-2459.

  ## Why the hash is an inductive type

  There is no SHA-256 here and there should not be. The property at stake is *structural*:
  it holds for any collision-resistant hash and fails for any construction that duplicates,
  regardless of the hash. Modelling the digest as the free term algebra — `leaf`/`node` as
  constructors — gives exactly the idealisation cryptographers mean by "assume the hash is
  injective", and makes the argument decidable rather than assumed.

  So `MTree` *is* the hash. Domain separation is not a byte prefix here; it is the fact that
  `leaf` and `node` are different constructors, which is what the byte prefix is *for*.
-/

namespace Scema

/-- An idealised digest: the hash tree itself, with leaves and nodes distinguishable. -/
inductive MTree where
  | leaf : Nat → MTree
  | node : MTree → MTree → MTree
deriving DecidableEq, Repr

namespace MTree

/--
  One layer of pairing, promoting an odd element.

  The `[x]` case is the whole point: `x` is carried up **unchanged**. The alternative —
  `pair [x] = [node x x]` — is the duplicating construction.
-/
def pairUp : List MTree → List MTree
  | [] => []
  | [x] => [x]
  | x :: y :: rest => node x y :: pairUp rest

/-- The duplicating construction, present only so the theorem below can talk about it. -/
def pairUpDuplicating : List MTree → List MTree
  | [] => []
  | [x] => [node x x]
  | x :: y :: rest => node x y :: pairUpDuplicating rest

/-- Fold layers until one root remains. `fuel` bounds the recursion for totality. -/
def rootAux : Nat → List MTree → Option MTree
  | _, [] => none
  | _, [x] => some x
  | 0, _ => none
  | fuel + 1, xs => rootAux fuel (pairUp xs)

/-- The root of a batch, or `none` for an empty batch — never a zero digest. -/
def root (xs : List MTree) : Option MTree := rootAux (xs.length + 1) xs

/-- The same, built by duplicating odd elements. -/
def rootDupAux : Nat → List MTree → Option MTree
  | _, [] => none
  | _, [x] => some x
  | 0, _ => none
  | fuel + 1, xs => rootDupAux fuel (pairUpDuplicating xs)

def rootDuplicating (xs : List MTree) : Option MTree := rootDupAux (xs.length + 1) xs

end MTree

open MTree

/-! ## Domain separation -/

/--
  A leaf is never equal to a node.

  In the implementation this is the `0x00` / `0x01` prefix; here it is constructor
  disjointness, which is what that prefix buys. Without it, an internal node could be
  presented as a leaf and an inclusion proof would verify for data that was never a record.
-/
theorem leaf_ne_node (n : Nat) (l r : MTree) : leaf n ≠ node l r := by
  intro h; cases h

/-! ## The empty batch -/

/--
  An empty batch has no root, rather than a root of zero.

  A zero digest is a value some other batch could legitimately hash to, and "nothing was
  anchored" would then be indistinguishable from "this specific thing was anchored".
-/
theorem root_of_empty : root [] = none := by
  decide

/-- A single record is its own root; no padding, no synthetic sibling. -/
theorem root_of_singleton (x : MTree) : root [x] = some x := by
  simp [root, rootAux]

/-! ## CVE-2012-2459: promotion versus duplication -/

/--
  **The duplicating construction collides.**

  Three leaves and four leaves — the fourth being a copy of the third — produce *the same
  root*. An attacker who can append a duplicate of the last element produces a different
  batch with an identical anchor, so the root no longer identifies what was anchored.

  This is stated as a concrete counterexample rather than a general lemma on purpose: a
  single witnessed collision is what makes the vulnerability real, and it is checkable by
  `decide` in milliseconds.
-/
theorem duplicating_construction_collides :
    rootDuplicating [leaf 1, leaf 2, leaf 3]
      = rootDuplicating [leaf 1, leaf 2, leaf 3, leaf 3] := by
  decide

/--
  **The promoting construction does not.**

  The same two batches, built the way `scema-anchor` builds them, have different roots. This
  is the theorem the implementation comment is asserting, and the reason the `[x] => [x]`
  case is written the way it is.
-/
theorem promoting_construction_separates :
    root [leaf 1, leaf 2, leaf 3] ≠ root [leaf 1, leaf 2, leaf 3, leaf 3] := by
  decide

/-- The same at the next odd size, so the result is not an artefact of three. -/
theorem promoting_separates_at_five :
    root [leaf 1, leaf 2, leaf 3, leaf 4, leaf 5]
      ≠ root [leaf 1, leaf 2, leaf 3, leaf 4, leaf 5, leaf 5] := by
  decide

/--
  Promotion is not merely different from duplication — it disagrees with it on the first
  odd batch, which is why swapping one for the other silently changes every anchored root.
-/
theorem promotion_differs_from_duplication :
    root [leaf 1, leaf 2, leaf 3] ≠ rootDuplicating [leaf 1, leaf 2, leaf 3] := by
  decide

/-! ## Order is part of the claim -/

/--
  Reordering a batch changes its root.

  Worth pinning: an implementation that sorted leaves for determinism would make two
  different anchoring orders indistinguishable, and the inclusion proofs would stop
  identifying a position.
-/
theorem root_is_order_sensitive :
    root [leaf 1, leaf 2] ≠ root [leaf 2, leaf 1] := by
  decide

end Scema
