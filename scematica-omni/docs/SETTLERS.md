# Writing a settler

**Contract: `scema.receipt/1`**

`scema pay` decides whether a spend may happen and seals a record. It does not move money.
Something else does, and that something else is a **settler**.

The split is not squeamishness. Omni cannot link `scematica-protocol` — x402 settlement
depends on `solana-sdk`, and that pin is precisely what the separate workspace exists to keep
out. So the boundary between deciding and paying is a JSON document, chosen the same way the
`WorldState` producer contract was: **a format, not a trait**, because the two sides cannot
link and no compiler will ever stand between them.

It turns out to be the right shape anyway. The decision is pure, total and testable offline;
settlement is I/O against a counterparty holding a key. A `pay` that did both would make the
interesting half untestable and the dangerous half invisible.

## The loop

```console
$ scema pay --capability inference.rank --to agent-b --units 250000 \
      --policy policy.json --commit
  sealed .scema/spends/ed88e60aa16c4a1d.json
  UNKNOWN — authorised and handed off; this runtime does not settle

SETTLEMENT REQUEST — hand this to something that can pay:
{ "capability": "inference.rank", "payee": "agent-b",
  "amount": { "asset": "lamports", "units": "250000" },
  "intent": null, "spend_record": "ed88e60aa16c4a1d" }

$ my-settler < request.json > receipt.json      # you write this part

$ scema reconcile receipt.json --ledger ledger.json
  settled — 5xQabc...signature
  budget  250000 spent across 1 settlement(s)
```

## What you emit

```json
{
  "spend_record": "ed88e60aa16c4a1d",
  "outcome": "settled",
  "reference": "5xQabc...signature",
  "settler": "my-settler/1.0.0"
}
```

or

```json
{
  "spend_record": "ed88e60aa16c4a1d",
  "outcome": "failed",
  "detail": "counterparty returned 402 with requirements this settler cannot meet",
  "settler": "my-settler/1.0.0"
}
```

That is the whole format. Two outcomes.

## The four rules, and why each one exists

**1. A settlement must carry a reference.** It is the entire difference between a claim
somebody can check and one they must take on trust. Nothing in omni can verify that money
moved; the reference exists so a human or a chain lookup can. A settler that cannot produce
one has not settled — say `failed` instead.

**2. A failure must say why.** Retry or do not retry is the only decision a failure informs,
and an operator cannot make it from the word "failed".

**3. There is no `unknown` outcome.** This is the rule most likely to surprise you, and it is
deliberate. A settler reporting "I do not know" is reporting the state the record is *already*
in — accepting it would let reconciliation appear to make progress while changing nothing.

If you cannot tell whether the payment went through, **emit nothing at all.** The spend stays
`Unknown` and stays visibly unresolved, which is exactly where a human should be looking.
Silence is the honest signal, and it is the one case where saying nothing is better than
saying something.

**4. Name yourself.** `settler` is optional so an operator can reconcile by hand from a block
explorer, but a program should fill it in. A reconciliation records whose word it is.

## Testing yours

Run the same vectors omni runs:

```console
$ cargo test -p scema-spend --test conformance     # omni's side
```

`crates/scema-spend/vectors/receipts.json` is the shared artefact. Point your emitter at it
and check you agree on every case. Both sides are then checked against **one document** rather
than against each other's prose — three hand-written `WorldState` producers drifted exactly
this way until `scema-tools/fixtures/` pinned them.

One vector deserves attention before you write anything:

```json
{ "name": "fabricated-but-well-formed", "accepted": true }
```

A made-up reference is **accepted**. This contract checks shape, never truth. If you read a
successful reconciliation as proof of payment, you have misread it.

## Testing the loop without a chain

`scema_spend::settler` gives you the seam and a double:

```rust
use scema_spend::{ScriptedSettler, Script, Settler, answers};

let s = ScriptedSettler::new("flaky", vec![
    Script::Silence,                                   // first attempt: no answer
    Script::Settle { reference: "sig-2".into() },      // second: settles
]);
```

`Script::Silence` is the one to build against. A caller that treats the first attempt's
silence as a failure and retries will **pay twice**, and that is the failure this whole design
is arranged around.

`answers(&request, &receipt)` checks the receipt is about the spend you asked about. Separate
from `Receipt::validate`, which only inspects the document — a settler working a queue can
reply to the wrong item, and a perfectly well-formed receipt for *another* spend would
otherwise resolve this one.

## What a settler is never given

The policy, the ledger, and the ability to authorise. Those decisions are made before you are
called and are not yours to revisit. A settler that could re-decide would be a second,
undocumented spend policy living wherever somebody happened to put the network code — pinned
by a test asserting `SettlementRequest` carries no caps, no allow-lists and no balance.

## Reconciliation, from your side

You do not write it. `scema reconcile` does, and two of its properties matter to you:

- **Reconciling twice is a no-op.** `Ledger.settled_ids` records what has been counted, so if
  your settler retries or your pipeline runs again, the budget is not charged twice. You do
  not need to be careful about this on omni's behalf.
- **The spend record is never edited.** Reconciliation appends its own sealed record; the
  original still reads `UNKNOWN`, because that was the state of knowledge when it was sealed.
  Do not expect the file you were pointed at to change.

## Where to put it

The bot workspace, next to `scematica-protocol`, which already has the x402 client
(`build_payment_payload`, `encode_payment_header`) and the `solana-sdk` pin it needs. Not in
this workspace — see the top of this document.
