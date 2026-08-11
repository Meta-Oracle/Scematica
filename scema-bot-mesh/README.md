# scema-bot-mesh

A modular neural mesh for BOT Chain: agents whose decisions can be **checked by someone
who did not run them**, including by a contract on chain 677.

```powershell
cargo test
cargo clippy --all-targets
```

## The idea in one paragraph

A game embeds an agent. The agent acts. A player, an opponent, or a contract holding a
bond wants to know the agent really ran the policy it claims, rather than picking whatever
outcome suited it. Weights are far too large for on-chain storage — but a keccak256 hash of
them is 32 bytes, and so is a hash of an inference. So the agent commits 32 bytes on chain,
and any challenger holding the weights re-runs the forward pass and compares. Disagreement
is provable, and a bond behind the claim can be slashed via
[`ScemaBondEscrow`](../scema-botchain/contracts/src/ScemaBondEscrow.sol).

## Why this is a determinism problem before it is a cryptography problem

Commit-and-challenge is old. The reason it is rarely applied to neural inference is that
**the challenger's re-run has to produce the same bits**, and floating point does not
cooperate:

- **Solidity has no floating point at all.** A contract adjudicating a dispute cannot even
  represent an `f32`. This alone settles the question — a float net can never be settled
  on-chain.
- **Transcendentals are libm implementations, not IEEE operations.** Two conforming
  machines legitimately disagree on `exp` in the last bits.
- **JavaScript has no `f32`**, so a browser verifier diverges on rounding alone.

So the foundation is Q16.16 integer arithmetic, and the parts people normally treat as
implementation detail are promoted to specification:

| Decision | Why it is in the spec |
|---|---|
| Round-half-**away-from-zero** | Symmetric: `(-x)·y == -(x·y)` exactly, so a sign flip cannot change a magnitude |
| Division, **not `>>`** | An arithmetic shift floors toward −∞ and breaks that symmetry — this was a real bug here, caught by `multiplication_is_symmetric_under_sign` |
| Fixed summation order, widened accumulator | A SIMD reimplementation that reassociates a sum is a consensus break, not a speedup |
| Ties → lowest index | An unspecified tie is a divergent action, and a divergent action is a divergent game state |
| Saturate, never wrap | A wrapped activation silently becomes its own negation |

`FRAC_BITS`, the parameter ordering, and the domain tags are all bound into the hash, so a
future change produces a visibly different commitment rather than a silently incompatible
one.

## The equations

Built on that arithmetic, in `net.rs`:

```
layer      y      = ReLU(Wx + b)
dueling    Q(s,a) = V(s) + A(s,a) − mean_a A(s,a)
policy     a*     = argmax_a Q(s,a)          ties → lowest index
bellman    y      = r + γ · max_a' Q(s',a')  0 past terminal
td error   δ      = y − Q(s,a)
```

The mean-subtraction in the dueling head is load-bearing rather than cosmetic: without it
`V` and `A` are unidentifiable — add a constant to `V`, subtract it from every `A`, and `Q`
is unchanged — so the split never converges anywhere meaningful. Centring the advantages
pins it, which is what makes `V(s)` separately interpretable. Same formulation as
`scematica-nn`'s Deep Q\*, restated in integers.

`bellman_target` and `td_error` are exported because they are what a verifier needs to check
a *training* claim. Inference commitments prove a policy ran; these make "I learned this
from that transition" checkable too.

## Layout

```
crates/
  mesh-core/     fixed-point arithmetic, policy net, keccak256 commitments
    src/fixed.rs    Q16.16 — the foundation
    src/net.rs      the equations above
    src/commit.rs   weights_hash, InferenceClaim, Verdict
```

One dependency: `tiny-keccak`. It earns its place by matching Solidity's `keccak256`
exactly, so a commitment made here needs no hash implementation on-chain. **Nothing else in
the inference path may add a dependency** — every one is something a reimplementer has to
match bit for bit.

## Deliberately out of scope

**Training.** Gradients are float-friendly and need not be reproducible; only the resulting
weights do, and those are committed by hash. Train in any framework, then quantise at the
boundary with `Fx::from_f64`. Only the forward pass must be deterministic.

**A network transport.** "Mesh" here means agents referencing each other's commitments, not
a peer-to-peer protocol. Transport belongs to the host application.

**Its own workspace**, like `scema-botchain`, for the same lockfile reason — and for a
second one: this tree is a specification other people are expected to reimplement, so it
stays dependency-minimal on purpose.

## Status

`mesh-core` is complete and tested: 28 tests covering the arithmetic laws, the identifiability
of the dueling decomposition, tie-breaking, replay stability, domain separation, and the
adjudication path end to end.

Not yet built: the game-facing integration crate, the on-chain registry contract that anchors
`weights_hash`, and the wiring from a losing `Verdict` to a slash.
