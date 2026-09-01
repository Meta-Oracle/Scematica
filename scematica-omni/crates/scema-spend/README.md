# scema-spend

Whether the agent may spend, how much, and on what.

```rust
match authorise(&policy, &ledger, &request) {
    Verdict::Allowed { remaining_after } => hand_off_to_a_settler(),
    Verdict::Refused { refusal }         => refuse(refusal.explain()),
}
```

`scema pay` was left unbuilt for a stated reason — *a runtime that can spend without a spend
policy is a runtime nobody should install.* This is that policy, and it exists **before** `pay`
does rather than alongside it.

## This crate decides. It never settles.

No wallet, no signer, no chain, no HTTP. `authorise` returns a verdict; something else moves
the money.

That is not squeamishness. Scematica Omni **cannot link `scematica-protocol`**, where x402
settlement lives, because it depends on `solana-sdk` — the exact pin the separate workspace
exists to keep out. The split turns out to be right anyway: the decision is pure, total and
testable offline, while settlement is I/O against a counterparty holding a key. A `pay` that
did both would make the interesting half untestable and the dangerous half invisible.

## Nothing is authorised by default

An empty policy authorises **nothing**. Not "no limits configured, so no limits" — that is the
`#[serde(default)]` bool trap, where a missing field silently disables a guard. A policy with
no payees allows no payees, and it gets its own `NoPolicy` refusal rather than reporting a
breached limit, because "you configured nothing" and "you are over budget" send an operator to
completely different places.

Allow-lists are **exact, never prefixes**. A prefix match on `inference.` would authorise every
capability anybody later named under it, including ones that did not exist when the policy was
written.

## Money is integers, and strings on the wire

`u128` in the asset's smallest unit. A float that rounds a limit *up* authorises a spend the
operator did not, and the failure is silent and in the expensive direction.

On the wire they are decimal **strings**. JSON numbers are IEEE-754 doubles to most parsers, so
anything past ~9e15 loses precision — and `serde_json` refuses a bare `u128` on the way back
in, so a record sealed with an integer could be written and never re-read. That is how this was
found: the first sealed record would not deserialise.

## `Settlement::Unknown` is a first-class arm

An authorised spend whose settlement could not be observed is **neither paid nor unpaid**.
Money and network failures overlap badly: a request that timed out may have settled, and
recording it as `Failed` invites a retry that pays twice.

So it exits **3**, and it does **not** consume budget — charging for a spend that may not have
happened lets a flaky counterparty drain an allowance. Neither default is safe, which is why it
is its own arm and why reconciling it is a separate, deliberate step.

## Writing a settler

`scema_spend::settler` is the seam, and `ScriptedSettler` is the double that makes the loop
testable before any code that can spend exists. It is *scripted* rather than
always-succeeding on purpose: a stub that always settles tests the one path least likely to go
wrong. `Script::Silence` is the case to build against — returning nothing is what produces
`Settlement::Unknown`, and a caller that treats it as failure and retries **pays twice**.

`answers()` is separate from `Receipt::validate` because a settler working a queue can reply to
the wrong item with a perfectly well-formed document.

Shared conformance vectors live in `vectors/receipts.json`; omni runs them in
`tests/conformance.rs` and a settler author runs the same file. One vector is
`fabricated-but-well-formed` and it is **accepted** — this contract checks shape, never truth.

Full guide: `scematica-omni/docs/SETTLERS.md`.

## What a settler is never given

The policy, the ledger, and the ability to authorise. Those decisions are made before it is
called. A settler that could re-decide would be a second, undocumented spend policy living
wherever somebody happened to put the network code — pinned by a test asserting
`SettlementRequest` carries no caps, no allow-lists and no balance.
