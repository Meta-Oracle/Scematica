/-
  Canonical encoding — why a commitment means anything.

  `scema-verify` hashes a *canonical* encoding rather than `serde_json` output: sorted keys,
  tagged types, normalised `-0.0` and NaN, floats as fixed-point at 1e-9. Every one of those
  is load-bearing, and the property they exist to buy is **injectivity**: distinct values must
  produce distinct encodings, or a commitment binds to more than one world and proves nothing.

  What is modelled here is the *encoding*, not SHA-256. Injectivity of the encoding is the
  part that is a design decision and can be got wrong; collision resistance of the hash is an
  assumption imported from elsewhere. Conflating the two is how a review concludes that a
  broken encoding is fine because the hash is strong.

  ## Why this is split into `canon` then `enc`

  The first version wrote one recursive `encode` that sorted object keys inline. It had to be
  `partial`, because Lean cannot see that recursing under `sortPairs ps` terminates — and a
  `partial def` does not reduce in the kernel, so every `decide` below got stuck. The proofs
  did not fail; they could not run at all.

  Splitting normalisation from serialisation fixes that and is better design anyway:
  `canon` sorts, `enc` writes bytes, both structurally recursive and therefore total. It also
  names the thing the Rust is really doing — there is a canonical *form*, and the bytes are a
  function of it.
-/

namespace Scema

/-- A value in the subset the canonical encoder accepts. -/
inductive Val where
  | int   : Int → Val
  | float : Int → Val          -- already fixed-point: round(v * 1e9)
  | str   : String → Val
  | bool  : Bool → Val
  | null  : Val
  | arr   : List Val → Val
  | obj   : List (String × Val) → Val
deriving Repr
-- No `deriving DecidableEq`: `Val` is a *nested* inductive (it contains `List Val`), and the
-- deriving handler does not cover those. It is not needed either — every theorem below
-- compares `List Nat`, which is the point: the claims are about the **bytes**, not about the
-- values, and a commitment is only ever taken over bytes.

/-- Type tags, mirroring the discriminants in `canonical.rs`. -/
def tag : Val → Nat
  | .null    => 0
  | .bool  _ => 1
  | .int   _ => 2
  | .float _ => 3
  | .str   _ => 4
  | .arr   _ => 5
  | .obj   _ => 6

/-- Insertion sort on keys — byte-wise, matching the Rust. -/
def insertPair (p : String × Val) : List (String × Val) → List (String × Val)
  | [] => [p]
  | q :: rest => if p.1 ≤ q.1 then p :: q :: rest else q :: insertPair p rest

def sortPairs : List (String × Val) → List (String × Val)
  | [] => []
  | p :: rest => insertPair p (sortPairs rest)

/-! ## Normal form -/

mutual

/-- The canonical *form*: the same value with every object's keys in byte order. -/
def canon : Val → Val
  | .arr xs => .arr (canonList xs)
  | .obj ps => .obj (sortPairs (canonPairs ps))
  | v => v

def canonList : List Val → List Val
  | [] => []
  | v :: rest => canon v :: canonList rest

def canonPairs : List (String × Val) → List (String × Val)
  | [] => []
  | (k, v) :: rest => (k, canon v) :: canonPairs rest

end

/-! ## Bytes -/

/--
  Sign then magnitude.

  Two bytes rather than one so a negative number cannot be mistaken for a larger positive
  one, and so there is exactly one encoding of zero — see `only_zero_encodes_as_zero`.
-/
def encodeInt (i : Int) : List Nat :=
  if i < 0 then [1, (-i).toNat] else [0, i.toNat]

mutual

/--
  The canonical byte encoding of an already-normalised value.

  Every case emits its tag first. That single decision is what makes the encoding injective
  *across* types, and it is the one an implementer is most tempted to skip because it costs
  bytes on values that are already unambiguous *within* a type.

  Every variable-length case emits a length before its contents, which is what stops
  `[[1],[2]]` and `[1,2]` colliding.
-/
def enc : Val → List Nat
  | .null    => [0]
  | .bool b  => [1, if b then 1 else 0]
  | .int i   => 2 :: encodeInt i
  | .float f => 3 :: encodeInt f
  | .str s   => 4 :: s.length :: s.data.map (·.toNat)
  | .arr xs  => 5 :: xs.length :: encList xs
  | .obj ps  => 6 :: ps.length :: encPairs ps

def encList : List Val → List Nat
  | [] => []
  | v :: rest => enc v ++ encList rest

def encPairs : List (String × Val) → List Nat
  | [] => []
  | (k, v) :: rest => (k.length :: k.data.map (·.toNat)) ++ enc v ++ encPairs rest

end

/-- Normalise, then write. The digest is taken over this. -/
def encode (v : Val) : List Nat := enc (canon v)

/-! ## Tag separation -/

/--
  **A float zero and an integer zero do not encode alike.**

  The `0.0` versus `0` hazard, as a decidable fact. `JSON.stringify` writing `0` where Rust
  wrote `0.0` moves the value across this boundary and the digest changes — which is correct:
  information the encoding depends on was destroyed by the round trip. Untagged, the round
  trip would be undetectable instead of merely annoying.
-/
theorem float_zero_ne_int_zero : encode (.float 0) ≠ encode (.int 0) := by
  decide

/-- Every type carries a distinct tag, so no two constructors can be confused. -/
theorem tags_are_distinct :
    tag .null ≠ tag (.bool true) ∧
    tag (.int 0) ≠ tag (.float 0) ∧
    tag (.str "") ≠ tag (.arr []) ∧
    tag (.arr []) ≠ tag (.obj []) := by
  decide

/--
  An empty string, an empty array and an empty object are all distinguishable.

  Untagged, all three are plausibly "nothing", and an encoder that emitted a length of zero
  for each would map them to one digest.
-/
theorem empty_containers_are_distinct :
    encode (.str "") ≠ encode (.arr []) ∧
    encode (.arr []) ≠ encode (.obj []) ∧
    encode (.str "") ≠ encode (.obj []) := by
  decide

/-! ## Sign, and the single zero -/

/-- Negative and positive one do not collide, despite equal magnitude. -/
theorem sign_is_encoded : encode (.int 1) ≠ encode (.int (-1)) := by
  decide

/--
  **There is exactly one zero.**

  `-0.0` and `0.0` are distinct IEEE-754 bit patterns and the same number, and a commitment
  that distinguished them would report tamper on a value nobody changed. Two things remove
  the hazard here, and it is worth being precise about which does the work:

  * The fixed-point representation eliminates it at the source — `round(-0.0 * 1e9)` and
    `round(0.0 * 1e9)` are both the integer `0`, and `Int` has one zero. So `-0` is not
    merely normalised, it is unrepresentable.
  * The sign-then-magnitude encoding does not *reintroduce* it, which is the real risk: an
    encoder emitting a sign byte could easily produce both `[0,0]` and `[1,0]`.

  This states the second, since the first is true by construction and proves nothing about
  the encoder.
-/
theorem only_zero_encodes_as_zero (i : Int) (h : encodeInt i = encodeInt 0) : i = 0 := by
  simp [encodeInt] at h
  by_cases hneg : i < 0
  · simp [hneg] at h
  · simp [hneg] at h
    omega

/-! ## Key order -/

/--
  **Object key order does not affect the encoding.**

  The property that lets a record survive any JSON library. Without sorting, a digest is a
  function of whoever serialised the document rather than of the document — and `/omni`
  re-reading a record the daemon wrote would fail to verify a byte nobody touched.
-/
theorem key_order_is_irrelevant :
    encode (.obj [("b", .int 2), ("a", .int 1)])
      = encode (.obj [("a", .int 1), ("b", .int 2)]) := by
  decide

/-- ...but the *values* under those keys still matter. -/
theorem values_still_matter :
    encode (.obj [("a", .int 1)]) ≠ encode (.obj [("a", .int 2)]) := by
  decide

/-- ...and so do the keys themselves. -/
theorem keys_still_matter :
    encode (.obj [("a", .int 1)]) ≠ encode (.obj [("b", .int 1)]) := by
  decide

/-- Sorting reaches nested objects, not just the top level. -/
theorem nested_keys_are_sorted_too :
    encode (.obj [("x", .obj [("b", .int 2), ("a", .int 1)])])
      = encode (.obj [("x", .obj [("a", .int 1), ("b", .int 2)])]) := by
  decide

/-! ## Structure is not flattened -/

/--
  A nested array does not encode like a flattened one.

  An encoder that emitted elements without a length prefix would make `[[1],[2]]` and
  `[1,2]` identical — the classic shape for a naively concatenating encoder.
-/
theorem nesting_is_preserved :
    encode (.arr [.arr [.int 1], .arr [.int 2]]) ≠ encode (.arr [.int 1, .int 2]) := by
  decide

/-! ## Resolution of the fixed-point binding -/

/--
  The commitment binds values to 1e-9, and this is what that means concretely: a change of
  one unit at that scale is visible. An edit below it is not caught — and cannot move any
  gate in `scema-policy`, which is the reason the resolution is acceptable.
-/
theorem one_ulp_at_1e_9_is_visible : encode (.float 1) ≠ encode (.float 0) := by
  decide

end Scema
