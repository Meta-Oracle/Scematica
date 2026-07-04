# ScemaDEX Settlement v2 — the Optimistic Conviction-Settlement State Machine

**Status:** design (no code yet) · **Author:** design pass 2026-07-04 · **Scope:** `scemadex-sdk` core + downstream crates

## Why this document exists

Three product directions were selected for the SDK's next phase — **close the money
loop on mainnet**, **generalize the rail beyond Solana swaps**, and **deepen the
primitive set**. They look independent. They are not. All three rendezvous on a
single missing piece: a **settlement state machine with a dispute window**.

This doc verifies that claim against the current code, specifies the state machine,
and shows how Counter-Market (E), Scar Market (F), lineage royalties (G), zkML bonds
(I), and a new insurance primitive (J) all attach to it. It is deliberately a design,
not a diff — the next step is a prototype in `settlement.rs` that keeps the current
one-shot path working.

## Current state (verified against source)

| Seam | File | What it does today | Limitation |
|---|---|---|---|
| Settlement | `bond.rs::EscrowBondEngine::settle` | `escrow → settle` in one atomic call; decides `Honored`/`Slashed` from `fill.amount_out.raw >= bond.min_out_raw` | No time dimension, no challenge input, no fraud proof. The outcome is final the instant it's computed. |
| Promise predicate | `bond.rs` + `route.rs::Fill` | Honor test is a swap min-out comparison; `Fill = { amount_out, executed_unix }` | The "did the promise hold?" question is hardcoded to *swap output ≥ threshold*. |
| Intent | `intent.rs::Intent` | `input_mint`, `output_mint`, `amount_in`, `Side::{Buy,Sell}`, `Objective::{Price,Speed,Stealth}` | Every field is swap-specific. A forecast, a classification, or an off-chain decision cannot be expressed. |
| Deadline | `bond.rs::Bond::deadline_unix` | Field exists | Vestigial — never read by `settle`. |
| Counter-Market | `counter.rs::CounterMarket` | Engine-agnostic challenge book; prices inferences via `doubt_spread`; pays out on a `BondOutcome` | **Cannot influence the outcome.** It is a parallel side-bet on an oracle result `settle()` already decided. Challengers can't flip an honored bond. |

**Diagnosis.** The bond lifecycle is a function, not a machine:

```
escrow(solution) -> Bond
settle(bond, fill) -> BondOutcome        // atomic, final, swap-shaped, unchallengeable
```

Everything the three directions need is a lifecycle that has *time* and *contestability*
in the middle.

## The keystone: the settlement state machine

Replace the atomic settle with an explicit lifecycle. Optimistic by default — a fill
provisionally honors immediately (so the happy path stays fast), but the outcome is not
**final** until a dispute window elapses without a successful challenge.

```
                         ┌──────────────── challenge upheld ───────────────┐
                         │                                                  ▼
Escrowed ──fill──▶ Provisional ──window elapses, unchallenged──▶ Finalized(Honored)
    │                    │                                                  
    │                    └──challenge filed──▶ Disputed ──resolve──▶ Finalized(Honored | Slashed)
    │
    └──deadline passes, no fill──▶ Finalized(Slashed)   // failure-to-deliver
```

States:

- **Escrowed** — collateral locked, awaiting a fill (or deadline).
- **Provisional(outcome)** — a fill landed; the naive predicate gives a *provisional*
  outcome. Dispute window `[t_fill, t_fill + W]` is open. Funds do **not** move yet.
- **Disputed** — a challenge was filed during the window. Resolution pending
  (evidence, re-execution, or ground-truth arrival).
- **Finalized(outcome)** — terminal. Now, and only now, do funds settle: bond returns
  to the agent (Honored) or flows to caller + challengers + insurance pool (Slashed).

The window `W` is a `SettlementConfig` knob. `W = 0` collapses the machine back to
today's atomic behavior — that is the backward-compatibility guarantee.

### Proposed core types (sketch)

```rust
// settlement.rs (new)

/// Where a bond is in its lifecycle.
pub enum BondState {
    Escrowed,
    Provisional { outcome: BondOutcome, window_closes_unix: u64 },
    Disputed { provisional: BondOutcome, challenge: ChallengeRef },
    Finalized { outcome: BondOutcome, reason: FinalizeReason },
}

pub enum FinalizeReason {
    WindowElapsed,      // optimistic honor
    DeadlineMissed,     // failure to deliver
    ChallengeUpheld,    // dispute flipped the provisional outcome
    ChallengeRejected,  // dispute failed; provisional stands
    ProofVerified,      // zkML/re-exec resolved it deterministically (Primitive I)
}

pub struct SettlementConfig {
    pub dispute_window_secs: u64,   // W; 0 == legacy atomic settle
    pub challenge_bond_bps: u32,    // stake a challenger must post, as bps of the bond
    pub slash_routing: SlashRouting,
}

/// Where slashed collateral flows. This is THE economic knob that lights up E/G/J.
pub struct SlashRouting {
    pub to_caller_bps: u32,      // compensation to the wronged buyer
    pub to_challengers_bps: u32, // Counter-Market winners (Primitive E)
    pub to_insurance_bps: u32,   // reinsurance pool (Primitive J)
    pub to_lineage_bps: u32,     // upstream experience royalty holders (Primitive G)
}
```

### The generalization seam (Direction 2, nearly free once we're here)

The only swap-specific thing left in settlement is the *promise predicate*. Abstract it
and the rail routes any bonded decision:

```rust
/// The evidence that resolves a bond. Swap fills are one impl.
pub trait Outcome: Send + Sync {
    /// Did the escrowed promise hold, given this realized evidence?
    fn satisfies(&self, promise: &Promise) -> bool;
    /// When the evidence became available (for window/deadline math).
    fn observed_unix(&self) -> u64;
}

// Swap case — today's behavior, now just one impl:
impl Outcome for Fill {
    fn satisfies(&self, p: &Promise) -> bool { self.amount_out.raw >= p.min_out_raw }
    fn observed_unix(&self) -> u64 { self.executed_unix }
}
```

`Intent` gains a generic escape hatch without breaking the swap struct — either a
`Promise` enum (`Swap { min_out_raw } | Forecast { claim, resolves_unix } | Predicate { .. }`)
or a `#[non_exhaustive]` addition. The swap path stays concrete and fast; new domains
add a variant + an `Outcome` impl. **Proof-of-generality demo:** a `ForecastVenueExecutor`
(~1 crate) where an agent bonds a probability, ground truth arrives after the window,
and wrong forecasts slash into the Scar Market — reusing 100% of this machinery.

## How the existing primitives attach

The state machine is not new scope bolted on; it's the missing spine the adversarial
primitives were built to plug into.

- **E · Counter-Market** — *becomes load-bearing.* A challenge filed during the
  **Provisional** window transitions the bond to **Disputed**. If upheld, it *flips*
  the outcome to Slashed at finalization — challengers are no longer betting on an
  oracle result, they are the fraud-proof mechanism. `CounterMarket::settle` already
  computes the correct payouts; we just call it from `Finalized` instead of from a
  unilateral `settle()`. Minimal change to `counter.rs`; big change in meaning.
- **F · Scar Market** — a **Finalized(Slashed, ChallengeUpheld)** transition is the
  cleanest possible trigger for `certify_scar`: the slash is now adversarially proven,
  not self-reported. Wire the finalize hook to emit a `ScarRecord`.
- **G · Lineage royalties** — `SlashRouting.to_lineage_bps` streams a slice of *slashed*
  collateral (not just fees) back up the experience lineage — upstream teachers share
  downstream losses, aligning incentives symmetrically.
- **I · zkML bonds** — a succinct or re-execution proof resolves **Disputed →
  Finalized(ProofVerified)** deterministically, collapsing the window. Today's
  `ReexecutionProofSystem` is the optimistic resolver; a real zk backend (risc0/SP1)
  drops in behind the same `InferenceProofSystem` seam and shrinks `W` toward zero.
- **J · Bond insurance/reinsurance (NEW)** — see below; the state machine's slash-routing
  is its revenue and its liability.

## New primitive J — bond insurance / reinsurance

The Counter-Market is the *speculation* side of a bond (bet it fails). Insurance is the
missing *hedge* side (underwrite that it succeeds). An underwriter (or pool) accepts a
premium up front; on **Finalized(Honored)** it keeps the premium, on
**Finalized(Slashed)** it covers part of the agent's loss from the pool. This:

- gives agents capital efficiency (post less collateral, buy coverage instead);
- creates a yield product for passive USDC (earn premiums underwriting honest agents);
- is the natural **sink** for `SlashRouting.to_insurance_bps` — the reinsurance pool is
  recapitalized by the very slashes it partially covers.

```rust
// insurance.rs (new primitive)
pub struct Policy { pub intent_digest: String, pub coverage: Usdc, pub premium: Usdc }
pub trait Underwriter {
    fn quote(&self, bond: &Bond, reputation: Reputation) -> Result<Usdc>; // premium
    fn bind(&self, policy: Policy) -> Result<()>;
    fn on_finalize(&self, digest: &str, outcome: BondOutcome) -> Result<Payout>;
}
```

Premium pricing keys off the **reputation oracle** (Primitive C) — high honor-rate agents
pay less. That same reputation signal enables **undercollateralized bonds** (a fast
follow): trusted agents post `bond * (1 - reputation_discount)` collateral, closing the
loop between C and D economically.

## Closing the loop on mainnet (Direction 1)

The state machine is chain-agnostic and testable in-process. Mainnet is then a
`BondEngine` impl backed by an on-chain escrow, not a redesign:

- **`scemadex-escrow` Anchor program** (sibling to `programs/scematica-swap`): a PDA
  custodies bond + challenge stakes; instructions mirror the states
  (`escrow`, `mark_provisional`, `file_challenge`, `finalize`). Trustless custody — no
  facilitator holds funds — matching the "cryptographically proven" thesis.
- **Promote `scemadex-settle`** from devnet reference to a mainnet settler wired to the
  program; the existing `X402BondEngine` in `scemadex-integrations` handles metered
  fees over x402.
- **Optimistic finality** means mainnet funds only move at **Finalized**, giving the
  dispute window time to catch a bad inference before money is irreversible — the
  on-chain analog of an optimistic rollup's challenge period.

## Sequencing

Build the spine once (pure Rust, no chain, no money), then fan out. Each step is
independently shippable and testable.

1. **`settlement.rs` state machine + `SettlementConfig`** in `scemadex-sdk`. `W = 0`
   preserves today's behavior; all existing tests pass unchanged. *(keystone)*
2. **`Outcome` trait + `Promise`** generalization; `Fill` becomes one `Outcome` impl.
   Swap path untouched.
3. **Wire E into finality** — challenge during window → Disputed → flip on upheld.
   Emit **F** ScarRecords on adversarial slashes.
4. **Forecast demo executor** — proves domain-generality end-to-end (Direction 2 done).
5. **Insurance primitive J** + reputation-priced premiums + `SlashRouting` sinks
   (Direction 3 core).
6. **`scemadex-escrow` Anchor program + mainnet settler** (Direction 1 — closes the loop).
7. **Real zk backend** behind `InferenceProofSystem` to shrink `W` (Direction 3 stretch).

## Backward compatibility

- `SettlementConfig::default()` ships `dispute_window_secs: 0` → atomic settle → every
  current test and the `conviction_client()` doctest pass byte-for-byte.
- `Intent`'s swap fields are unchanged; generality is additive (new `Promise` variant).
- `EscrowBondEngine::settle` keeps its signature; internally it becomes
  `provisional() → finalize()` with a zero window. The `BondEngine` trait gains
  optional `provisional`/`finalize`/`challenge` methods with default impls that fall
  back to the atomic path, so external `BondEngine` impls keep compiling.

## Open questions (for the build phase)

- **Time source.** The state machine needs "now" for window math, but the SDK core is
  deliberately pure (no wall-clock in workflows/scaffold). Inject a `Clock` trait
  (`unix_now()`) so it stays testable and deterministic.
- **Challenge griefing.** `challenge_bond_bps` must make frivolous disputes costly;
  tune against the doubt-spread economics already in `counter.rs`.
- **Dust & conservation** across the new `SlashRouting` split — reuse the
  last-challenger-absorbs-dust invariant already proven in `counter.rs` tests.
- **Partial fills / partial honor** — is `Outcome::satisfies` boolean forever, or does
  it graduate to a `[0,1]` degree that scales the slash? Boolean first; revisit with J.
