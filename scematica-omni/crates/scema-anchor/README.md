# scema-anchor

Batch sealed records under one Merkle root, with a per-record inclusion proof, so a
commitment can be pinned somewhere its author does not control.

```console
$ scema anchor                                   # batch every sealed record
$ scema anchor --proof 8f92a1c4 > proof.json     # one record's proof
$ scema anchor --check proof.json --root-hash 4f21…
PROOF    INCLUDED
```

## What this closes

`scema verify` proves a record was not edited after sealing, and says plainly that it does
**not** prove the record is the original — whoever holds the only copy can seal a different
one and the commitment will be perfectly valid. Every statement of that limit in this
repository ends the same way: *until the root is anchored somewhere the author does not
control.*

This is the batching half. A third party holding one record and one proof can check
membership without the batch, without the other records, and without trusting whoever
produced them.

## Why SHA-256, and why that is not a compromise

`mesh-attest` uses keccak-256, and matching it would let the two share a verifier. That
instinct is wrong here: Omni's commitments are SHA-256, so changing the hash would mean
**every record already sealed on disk stops verifying** — and a verifier that rejects
untouched history is the one failure that teaches a reader to stop believing it.

It costs nothing, because EVM exposes SHA-256 as precompile `0x02`. A Solidity contract can
check one of these proofs directly. The algorithm is recorded in the batch and checked on
verification rather than assumed, so a future keccak batch is a different, clearly-labelled
artefact instead of a silent reinterpretation of this one.

## Two details that are cheap to get wrong

**Leaves and internal nodes are domain-separated** — leaf is `H(0x00 ‖ bytes)`, node is
`H(0x01 ‖ left ‖ right)`. Without the tags an attacker presents an internal node as a leaf
and proves membership of something never submitted.

**An odd node is promoted, never duplicated.** Duplicating to pad a level is the widespread
implementation and it lets two different leaf sets produce one root — the CVE-2012-2459
shape, which here would mean a batch can be presented as covering a record it never covered.

Both are pinned by tests that fail if the property is lost.

## Anchoring is asserted, not verified

`Batch::anchors` is a list because the plan is more than one chain: one whose economics we
control, one with an audience. Each entry is independently checkable, so two anchors are
stronger than one — and **zero is honestly unanchored**, said in those words, rather than a
batch quietly presented as more than it is.

Nothing in this crate talks to a chain. `scema anchor --record` writes down that a root was
published; it does not check, and it says so. Reaching a chain is a network act with a key
behind it, and recording an anchor that was never submitted would be exactly the fabrication
the rest of this runtime exists to refuse. A reader follows the reference and checks for
themselves — which is the point. An anchor you take on the author's word is not an anchor.
