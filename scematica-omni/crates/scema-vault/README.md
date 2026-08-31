# scema-vault

Serves sealed world trees to the holders of the tokens that commit to them.

```console
$ scema-vault --records .scema/decisions --entitlements ./entitlements.json
scema-vault on http://127.0.0.1:7843
  no ownership oracle: every request answers 503 undetermined, not 403
```

Three GETs, no write path:

| Route | Gate |
|---|---|
| `GET /health` | none — and it does **not** report the record count |
| `GET /worlds` | none — a buyer must see what a token would buy |
| `GET /world/<commitment>` | holder of the token committing to that world |

## Why this is a separate binary

`scema-omnid` binds loopback and that bind is **deliberately not configurable** — its own
note says the one thing that reliably happens to a `--bind` flag is somebody setting it to
`0.0.0.0`. It emits no CORS headers and handles no `OPTIONS`.

A distribution service is the opposite: it is *supposed* to be reachable. Putting both
postures in one process and one config file means somebody eventually enables the wrong one.
So this is a different binary with a different default, and the daemon keeps its guarantee.

It still defaults to loopback, so starting it by accident exposes nothing.

## The guarantee this must never damage

A record served here **verifies offline afterwards** — `scema verify --file`, or the `/omni`
page, both with no server and no permission. The response says so in `X-Scema-Verify`, not
just in this file.

This gates **distribution, never truth**. A holder who fetches a record once owes the service
nothing. If that ever stops being true, the design has gone wrong.

Which is why `store.rs` returns the bytes **verbatim**. Re-serialising through `serde_json`
would collapse Rust's `0.0` to `0`, moving it from the FLOAT tag to the INTEGER tag in the
canonical encoding and changing the digest — so a holder would fetch a record that fails its
own verification. That is the worst failure available here, because it teaches people the
verifier is broken. Pinned by a test, and checked end to end: the served bytes are
byte-identical to disk and `scema verify --file` reports VALID on them.

## Status codes carry the distinction

| Code | Meaning |
|---|---|
| `401` | no `X-Scema-Holder` — checked **before** any chain read |
| `400` | the path is not 64 lowercase hex characters |
| `403` | a fact about the holder: they do not hold the token, or it commits to another world |
| `404` | no such world here, **or** entitled but not stored — the second says the gap is the vault's |
| `405` | anything that is not a GET. There is no write path at all |
| `503` | **undetermined** — the chain could not be read. `retry: true` |

**`503`, never `403`, when ownership is unknown.** An RPC timeout is not a fact about the
holder, and a `403` sends somebody to buy a token they may already own. It still fails
closed — nothing is served — because failing closed and reporting accurately are different
choices and this makes both.

## Ownership

`OwnershipOracle` is a trait; this binary ships two implementations and neither reads a chain:

- **`NoChain`** (default) answers `Unknown` for everything, so an unconfigured vault serves
  nothing and says exactly why. It does not answer `NotHeld`, because it genuinely cannot
  tell and that would be a claim with no basis.
- **`GrantAll`** behind `--insecure-grant-all`, which prints an unsilenceable warning on every
  start. The failure mode is leaving it on, and a service that quietly grants everything looks
  exactly like one that is working.

Wire a real oracle by implementing the trait against an indexer or a node.

## Entitlements

A JSON array the operator writes:

```json
[{ "chain": "eip155:1", "contract": "0x…", "token_id": "1", "world_commitment": "12c7bd…" }]
```

This process never mints and never guesses which token commits to which world — the mapping
is an input so it can be reviewed. Malformed commitments are rejected at load, not at request
time.

## TLS

There is none, on purpose. Put a TLS-terminating reverse proxy in front. The alternative is a
TLS stack in the omni workspace, which is the dependency the whole workspace split exists to
avoid.
