/-
  Scematica, formalized.

  See `README.md` for what this package does and — more importantly — what it deliberately
  does not do. In one line: it is not a Rust-to-Lean transpiler. It is a formal model of the
  handful of invariants Scematica's guarantees actually rest on, plus a mechanism for
  checking real Rust output against that model.
-/

import Scema.Term
import Scema.Merkle
import Scema.Canonical
