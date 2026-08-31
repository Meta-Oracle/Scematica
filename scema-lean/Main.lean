/-
  `formalize` — the runnable half.

  Building the library proves the theorems. This executable reports what was proved and
  re-runs the decidable checks as computations, so the package is useful to somebody who
  wants an answer without reading Lean.
-/

import Scema

open Scema

/-- One line of the report. -/
def check (name : String) (ok : Bool) : IO Bool := do
  IO.println s!"  {if ok then "ok  " else "FAIL"}  {name}"
  pure ok

def main : IO UInt32 := do
  IO.println "SCEMA · formal model"
  IO.println ""
  IO.println "TERM — the unmeasured/measured-zero distinction"
  let r1 ← check "an unmeasured term contributes the additive neutral element"
              (Term.absent.contribution == 0)
  let r2 ← check "a measured zero contributes zero too — the arithmetic collapses them"
              ((Term.counted 0).contribution == Term.absent.contribution)
  let r3 ← check "...but they are different observations"
              (Term.counted 0 != Term.absent)
  let r4 ← check "...so the rendering is where the distinction survives"
              (render (Term.counted 0) != render Term.absent)
  let r5 ← check "an unmeasured term renders as an em dash whatever it carries"
              (render ⟨42, false⟩ == "—")

  IO.println ""
  IO.println "COVERAGE — a count, never a ratio"
  let r6 ← check "an empty aggregate is 0/0, which is undefined rather than zero percent"
              (Coverage.of [] == ⟨0, 0⟩)
  let r7 ← check "2/5 and 4/10 are different claims"
              (Coverage.of [Term.counted 1, Term.counted 1, Term.absent, Term.absent, Term.absent]
                 != Coverage.of (List.replicate 4 (Term.counted 1) ++ List.replicate 6 Term.absent))

  IO.println ""
  IO.println "MERKLE — CVE-2012-2459"
  let r8 ← check "the duplicating construction collides on 3 vs 4 leaves"
              (MTree.rootDuplicating [.leaf 1, .leaf 2, .leaf 3]
                 == MTree.rootDuplicating [.leaf 1, .leaf 2, .leaf 3, .leaf 3])
  let r9 ← check "the promoting construction separates them"
              (MTree.root [.leaf 1, .leaf 2, .leaf 3]
                 != MTree.root [.leaf 1, .leaf 2, .leaf 3, .leaf 3])
  let r10 ← check "an empty batch has no root, not a zero root"
              (MTree.root [] == none)
  let r11 ← check "order is part of the claim"
              (MTree.root [.leaf 1, .leaf 2] != MTree.root [.leaf 2, .leaf 1])

  IO.println ""
  IO.println "CANONICAL — why a commitment binds"
  let r12 ← check "a float zero and an integer zero do not encode alike"
              (encode (.float 0) != encode (.int 0))
  let r13 ← check "object key order does not affect the encoding"
              (encode (.obj [("b", .int 2), ("a", .int 1)])
                 == encode (.obj [("a", .int 1), ("b", .int 2)]))
  let r14 ← check "nesting is not flattened"
              (encode (.arr [.arr [.int 1], .arr [.int 2]]) != encode (.arr [.int 1, .int 2]))
  let r15 ← check "there is exactly one zero"
              (encode (.float 0) == encode (.float (-0)))

  let all := [r1, r2, r3, r4, r5, r6, r7, r8, r9, r10, r11, r12, r13, r14, r15]
  let passed := all.countP id
  IO.println ""
  IO.println s!"{passed}/{all.length} computable checks agree with the proofs"
  IO.println ""
  IO.println "The proofs themselves were discharged at build time. This binary re-runs the"
  IO.println "decidable ones as computations, so a reader who does not read Lean still gets"
  IO.println "an answer — and so a `decide` that was quietly weakened would show up here."
  if passed == all.length then pure 0 else pure 1
