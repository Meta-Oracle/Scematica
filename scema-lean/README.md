# scema-lean — Scematica's invariants, as machine-checked proofs

```console
$ lake build          # discharges every proof
$ lake exe formalize  # re-runs the decidable ones as computations
15/15 computable checks agree with the proofs
```

Lean 4.18.0, no Mathlib, no network. Builds in about a minute from cold.

## What this is not

**It is not a Rust-to-Lean transpiler.** Translating arbitrary Rust into a proof assistant is
a live research problem — [Aeneas](https://github.com/AeneasVerif/aeneas) (Inria, via LLBC)
and [hax](https://github.com/hacspec/hax) (Cryspen, to F\*/Coq) have both spent years on
*subsets* of the language, and both are careful about which subset. Anything claiming to do
it in general is a demo that breaks on the second real file.

So this package does not read `.rs` files and it never will. What it does is narrower and
more useful.

## What it is

Scematica's guarantees rest on a small number of pure, total functions. Not on the sniper,
not on the agent, not on any of the hundreds of thousands of lines around them — on maybe
three hundred lines that decide whether a commitment binds, whether an anchor identifies what
it claims, and whether an unmeasured quantity can masquerade as a measurement.

Those are the ones formalized here. Three modules:

| Module | What it settles |
|---|---|
| `Scema/Term.lean` | why an unmeasured term must render as `—` and not `0.00` |
| `Scema/Merkle.lean` | why odd nodes are promoted rather than duplicated |
| `Scema/Canonical.lean` | why the encoding is tagged, sorted, and fixed-point |

Everywhere else in this repository those rules are enforced by convention, comments and
tests. Here they are theorems.

## The result worth reading

`Term.lean` proves two things that are better together than apart:

```lean
theorem contribution_collapses :
    (Term.counted 0).contribution = Term.absent.contribution := by decide

theorem render_distinguishes :
    render (Term.counted 0) ≠ render Term.absent := by decide
```

The first says the **arithmetic provably cannot** tell a measured zero from an unmeasured
term. That is not a deficiency in the implementation — it is a property of summation, and no
amount of care in `scema-policy` can recover it. The second says the **string is the only
place the distinction survives.**

Which is the argument for the em dash, stated properly. Before writing it down, "print `—`
rather than `0.00`" was a house style anyone could reasonably argue with. After, it is the
last surviving copy of an observation, and a renderer that drops it destroys information
nothing downstream can reconstruct.

`Merkle.lean` earns its place differently. The Rust test asserts that `[a,b,c]` and
`[a,b,c,c]` have different roots — true, and checked against real SHA-256. Lean also proves
the thing Rust *cannot* test, because Rust does not implement the broken version:

```lean
theorem duplicating_construction_collides :
    rootDuplicating [leaf 1, leaf 2, leaf 3]
      = rootDuplicating [leaf 1, leaf 2, leaf 3, leaf 3] := by decide
```

The vulnerability is exhibited, not just avoided. That is the difference between "our tests
pass" and "here is why the other construction is wrong."

## Why the hash is an inductive type

`MTree` has two constructors, `leaf` and `node`, and *is* the digest. There is no SHA-256.

This is deliberate and it is the standard idealisation: the property at stake is structural,
holding for any collision-resistant hash and failing for any duplicating construction
regardless of the hash. Modelling the digest as the free term algebra is what "assume the
hash is injective" means, and it makes the argument decidable instead of assumed. Domain
separation is not a `0x00`/`0x01` prefix here — it is constructor disjointness, which is
what that prefix buys.

## Why values are `Int`

Not an approximation. `scema-verify` already hashes floats as `round(v * 1e9)` in `i64`,
because `serde_json`'s parser is not correctly rounded for every 17-digit input and a
commitment over raw IEEE-754 bits fails the moment a record crosses JSON — a record sealed in
the daemon and re-read over `GET` reported INVALID on a byte nobody touched.

So the *committed* value of every term in Scematica is already an integer in units of 1e-9.
`Int` is faithful to what is actually signed, and it makes every statement here decidable.

## Where the gap is, and it is real

The honest weakness of any formalization: **this is a model of the Rust, not the Rust.** The
theorems hold of `Scema.encode`. Whether `scema_verify::canonical::encode` agrees with it is
not proved, and cannot be without exactly the transpiler this package declines to be.

Two things narrow the gap without pretending to close it:

1. **The model was written against the implementation and its stated reasons**, not against a
   guess. Every doc comment here names the Rust it mirrors and the failure that motivated it.
2. **The properties are shared with real tests.** `scema-anchor`'s
   `an_odd_node_is_promoted_so_two_leaf_sets_cannot_share_a_root` uses the same `[a,b,c]` /
   `[a,b,c,c]` witness as `promoting_construction_separates`. `check:omni` pins the float/int
   tag distinction the same way `float_zero_ne_int_zero` does.

That is a correspondence maintained by hand, and it will drift if nobody maintains it. Saying
so is the point — the alternative is a proof that is true about a model nobody kept in sync,
which is worse than no proof because it is believed.

**The next step that would actually close some of it** is a Rust binary emitting Lean source
from real artifacts: take a sealed record, emit its canonical encoding as a `Val` literal plus
`example : encode ‹that› = ‹those bytes› := by decide`, and let `lake build` check the bytes
Rust really produced against the model. That turns hand-maintained correspondence into a
build failure. It is not built yet, and this README will say so until it is.

## Layout

```
lean-toolchain        pinned to v4.18.0 — no toolchain fetch, so this builds offline
lakefile.toml         no dependencies, deliberately (see the comment in it)
Scema.lean            root
Scema/Term.lean       Term, Coverage, the utility equation
Scema/Merkle.lean     the anchor tree and CVE-2012-2459
Scema/Canonical.lean  tags, key order, fixed-point
Main.lean             `formalize` — the checks as a runnable report
```

## Notes for anyone extending it

- **No Mathlib.** Every proof must be reachable by `decide`, `simp`, `omega` or a short term.
  If a claim needs measure theory it is not a claim about a wire format. This also keeps the
  build fast enough that people actually re-run it.
- **`partial def` does not reduce in the kernel.** The first version of `Canonical.lean` sorted
  keys inside the recursive encoder, which forced `partial`, and then every `decide` got stuck
  — the proofs did not fail, they could not run. Splitting `canon` (normalise) from `enc`
  (serialise) made both structurally recursive and is better design anyway.
- **`Val` cannot derive `DecidableEq`** — it is a nested inductive. It does not need to: every
  theorem compares `List Nat`, which is correct, because a commitment is only ever taken over
  bytes.
