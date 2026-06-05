# Decision: the open/proprietary boundary for bond settlement

**Status:** proposed (awaiting owner sign-off)
**Date:** 2026-06-05
**Owner:** Scematica core

## Context

Conviction Routing (Primitive D) is ScemaDEX's defining feature: a routing agent
escrows a **slashable performance bond** against its own quote. The published
`scemadex-sdk` ships the full settlement *state machine* in the open —
`EscrowBondEngine` sizes a conviction-weighted bond, sets a guaranteed-minimum
output, escrows it, and records honored/slashed outcomes into a `BondLedger`.

What it does **not** ship is the **on-chain USDC movement**. The actual transfer
of a slashed bond (or an inference fee) lives in `scemadex-integrations`
(`X402BondEngine`), which is `publish = false` because it depends on the
proprietary `scematica-protocol` x402 facilitator.

**Consequence:** an external developer can fully *simulate* Conviction Routing
but cannot *settle* a real bond. The headline trust primitive is, for outsiders,
a no-op. This is the single biggest cap on SDK adoption.

## The question

How much of the settlement rail do we open-source, and on which network?

## Options

### Option 1 — Keep it fully proprietary (status quo)
The open crate simulates; real settlement requires a commercial/licensed
relationship for the `scematica-protocol` stack.

- ➕ Maximum monetization leverage; the moat (real money movement) stays closed.
- ➕ Zero new attack surface or support burden on a public settlement path.
- ➖ Adopters can't prove the headline feature works end-to-end; "trustless"
  routing reads as marketing until they pay.
- ➖ Slows the mesh flywheel — peers won't transact real value without it.

### Option 2 — Open a **devnet** USDC reference settler  ⭐ recommended
Ship a minimal, open `VenueExecutor`/`BondEngine` wiring that moves **devnet**
USDC via a standard SPL transfer (no proprietary facilitator), gated behind a
feature like `settle-devnet`.

- ➕ Outsiders run the *whole* loop — quote → bond → execute → real on-chain
  settle — with zero financial risk and no license.
- ➕ Proves the primitive is real; converts "interesting" to "I shipped it."
- ➕ The mainnet facilitator (fee abstraction, x402 metering, batching, the
  trust/relay network) stays proprietary — the moat is the *production* rail,
  not the mechanism.
- ➖ Some implementation + maintenance cost; must be clearly labelled devnet-only.

### Option 3 — Open a **mainnet** reference settler
Same as Option 2 but on mainnet with real USDC.

- ➕ Nothing left to prove.
- ➖ Gives away the core monetizable rail; invites forks of the exact thing we
  sell; real-funds support burden and liability. Hard to walk back.

## Recommendation

**Adopt Option 2.** It removes the adoption ceiling (the primitive becomes
demonstrably real, end-to-end, for free) while keeping the actually-defensible
asset — the **production mainnet facilitator + the trained policy + the
accountability/relay network** — closed. The mechanism is not the moat; the
liquidity, fee abstraction, and trust network are.

### Proposed scope if approved
- New optional feature on a published wiring crate (e.g. `scemadex-settle`):
  `settle-devnet` → an `SplUsdcBondSettler` doing plain devnet SPL transfers.
- A `examples/devnet_settlement.rs` requiring a funded devnet keypair env var,
  excluded from CI (same pattern as the `live-tests` feature).
- Docs: a "Real settlement on devnet" section in `scemadex.md`, and an explicit
  table row distinguishing *simulated* / *devnet* / *mainnet (proprietary)*.
- Keep `X402BondEngine` and the mainnet facilitator `publish = false`.

### Explicitly out of scope
- Any mainnet settlement in open source.
- Exposing the x402 facilitator internals, fee-abstraction, or relay trust graph.

## Open questions for sign-off
1. Is devnet-real settlement acceptable to expose, or must even the mechanism
   stay closed for now?
2. Should the reference settler use the x402 *message format* (so devnet code is
   shaped like mainnet) or a plain SPL transfer (simpler, less reveal)?
3. Naming/packaging: fold into `scemadex-sdk` behind a feature, or a separate
   `scemadex-settle` crate?
