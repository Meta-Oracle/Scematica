# scema-entitlement

Who may read which world tree.

```rust
let entitlement = Entitlement::from_metadata(token, &token_metadata)?;
match authorise(&oracle, &entitlement, &holder, requested_digest) {
    Decision::Granted { world_commitment } => serve(&world_commitment),
    Decision::Denied { reason }            => refuse(reason.explain()),
    Decision::Undetermined { why }         => retry_later(why),   // NOT a denial
}
```

No chain client, no crypto, no HTTP. Everything that decides what an entitlement *means* is
pure and tested offline; the chain read is an `OwnershipOracle` the caller supplies. Same
split as `alchem_link.omni.world` versus `perceive`, for the same reason — the meaning is the
part worth testing.

## The design, in one line

A `scema-nft` token's metadata already commits to exactly one world, in
`scema.world_commitment`. So **holding the token for world `X` entitles the holder to the
record behind digest `X`** — no tier table, no new identifier, no access list. The grant is
*derived from the artefact*, so the token and the entitlement cannot drift apart.

`Entitlement::from_metadata` returns `None` when a token names no world. That token entitles
its holder to nothing, and an invented empty digest would either match nothing or — much
worse — match a record whose commitment field was also empty.

## Three answers, not two

`Undetermined` is not a denial, and collapsing it into one is the mistake this crate exists
to prevent. An RPC timeout, a rate limit and a reorganised chain all produce it, and none of
them is a fact about the holder. Told "you do not own this", somebody goes and buys a token
they already have.

Access still **fails closed** — `Undetermined` serves nothing. Failing closed and reporting
accurately are independent choices and this makes both.

Every `DenialReason` explains itself in terms the requester can act on. `WrongWorld` names
both digests, because "one token, one world" is surprising the first time you meet it.

## Order of checks is a security property

`authorise` checks request shape, then what the token entitles, then ownership. Consulting
the chain first would leak — by timing and by error message — whether an arbitrary address
holds a token, for a request that could never have been granted. A test with an oracle that
panics on use pins the order.

## Challenges

`challenge.rs` handles proving *control* of an address. Ownership says a token sits at an
address; addresses are public, so a request naming one is a claim, not evidence.

Signature verification is deliberately absent — it needs secp256k1 or ed25519 depending on
the chain. What is here is the part that is easy to get wrong and needs no crypto:

1. **Expiry**, or a signature harvested once works forever.
2. **Binding to the request**, or a signature proving control is replayed to authorise a
   *different* world than the one it was collected for. Everything scoping the grant appears
   in `message()`; anything omitted is unbound, and unbound means replayable.
3. **The verifier stamps the clock.** A requester-supplied `issued_at` lets them mint a
   challenge that never expires, which makes the expiry check ornamental. A challenge dated
   in the future is refused rather than tolerated.

`validate()` returning `Ok` means the challenge is fresh and covers what is being asked. It
says **nothing** about who signed it. The name invites the opposite reading, so there is a
test that says so.

## Three things this is not

**It never grants write.** This answers *what may be read*. `scema_tools::Workspace` answers
*where*, `scema-trust` answers *whether an action may happen*. Merging any two is how a grant
for one silently becomes a grant for another.

**It does not make a sealed record less verifiable.** A record somebody already holds
verifies with no server, no key and no permission — that is the whole point of `scema verify`
and `/omni`. This gates *distribution*, never *truth*. If it ever appears in a verification
path, something has gone wrong.

**It is not a paywall on your own records.** Records under your own `.scema/` are yours. This
exists for a server distributing a corpus to holders.
