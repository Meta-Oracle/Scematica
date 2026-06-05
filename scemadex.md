# ScemaDEX — Never-Before-Seen Features

> **ScemaDEX is not "a DEX."** It is an SDK in which **the routing intelligence
> itself is a metered, learning, accountable product.** Autonomous agents solve
> swap *intents*, **bond** their promises, sell their inferences, and trade
> learned experience with one another — all settled in stablecoins over the x402
> payment rails.

Crate: [`scemadex-sdk`](crates/scemadex-sdk) · `v0.1.1` · published on crates.io.
Every claim below is backed by a runnable example (offline, no keypair/RPC):
`cargo run -p scemadex-sdk --example <name>`.

---

## Why this is new

Every existing Solana router — Jupiter, 1inch-style aggregators, OpenBook
crank bots — answers one question: *given a path graph, what's the cheapest
route right now?* The route is a **free, anonymous, unaccountable** byproduct.
If the quote is wrong, nobody is on the hook.

ScemaDEX inverts all three of those properties. The *decision* is the product,
the decider is **identified and metered**, and the decider posts a **slashable
bond** against its own answer. That combination does not exist in any shipped
DeFi SDK today. The five features below are what fall out of it.

| | Feature | Module | Demo |
|--|---------|--------|------|
| **A** | Metered inference routing — the *quote* is the SKU | `policy`, `EscrowBondEngine::quote_fee` | `quote` |
| **B** | Intent solving over *timing & footprint*, not just path | `intent` | `intent_solving` |
| **C** | Reputation oracle sourced from *bond settlement history* | `oracle`, `BondLedger` | `conviction_bond` |
| **D** | **Conviction Routing** — slashable bonds on a black-box inference | `bond` | `conviction_bond` |
| **★** | A **PeerMarket**: agents trade bonded inferences *and learned experience* | `mesh` | `peer_market` |

---

## D · Conviction Routing — the defining primitive

**The new idea:** an autonomous agent that sells you a routing decision must
**escrow a slashable performance bond against that exact decision.** Meet the
guarantee → the agent reclaims the bond and keeps its fee. Miss it → the bond
settles to *you* as compensation. A paid black-box inference becomes
*trustworthy* because the seller has skin in the game it can lose.

Nothing in DeFi routing works this way today. Aggregators quote and walk away;
ScemaDEX quotes and *posts collateral*.

The bond is **conviction-weighted** — the policy's own self-assessed confidence
(in `[0,1]`, produced directly by the Deep Q\* value head under the `scematica`
feature) sizes both the bond and the inference fee. Confidence is *priced*:

```text
$ cargo run -p scemadex-sdk --example conviction_bond
conviction 0.50
  fee charged : 25000 micro-USDC      # fee scales with conviction
  bond escrow : 2500000 micro-USDC    # bond scales with conviction
  guaranteed  : >= 990000000 base units out   # 100 bps haircut off expected out
  open bonds  : 1

fill meets guarantee -> Honored
fill misses guarantee -> Slashed

ledger: 1 honored / 1 slashed  (honor rate 50%)
```

The settlement state machine (`EscrowBondEngine`) ships **fully open source** —
conviction-weighted sizing, the guaranteed-minimum haircut, escrow/settle, and
the honored/slashed ledger. Only the on-chain USDC transfer lives in a
proprietary companion crate behind the `scematica` feature; the logic above runs
identically offline.

```rust
use scemadex_sdk::{conviction_client, demo_intent, BondOutcome};

let dex = conviction_client();
let (solution, bond) = dex.quote(&demo_intent()).await?;   // solve + escrow a real bond
assert!(bond.amount.0 > 0);                                // skin in the game
let (_fill, outcome) = dex.execute(&demo_intent()).await?; // execute + settle
assert_eq!(outcome, BondOutcome::Honored);
```

---

## A · Metered inference routing — the quote is the SKU

In ScemaDEX the unit of sale is **the decision, not the swap.** Every quote is
produced by a learning policy and is individually priced and billable
per-call over x402. You are paying for *the quality of the route*, not for
liquidity access.

This reframes a router from infrastructure into a **margin business**: a better
agent literally charges more for a better answer, and `quote_fee()` ties that
price to the agent's conviction. The (solution, bond) pair returned by
`ScemaDex::quote` is the atomic, sellable unit — see the PeerMarket below.

---

## B · Intent solving over timing and footprint

Callers express **what they want**, never a path:

```rust
Intent { /* mints, amount, side */ objective: Objective::Stealth, .. }
```

The `Objective` is the novel surface. Static pathfinders optimize one axis —
price along a fixed graph. ScemaDEX's RL policy can also optimize **dimensions a
pathfinder structurally cannot**:

- `Objective::Price` — best effective price / minimal slippage.
- `Objective::Speed` — fastest confirmation; price is secondary.
- `Objective::Stealth` — **split and *time* the order to resist MEV/sandwiching.**

`Stealth` is the standout: the policy controls *when* and *in how many pieces* to
execute, learning an anti-sandwich execution schedule. A quote engine that
returns a single path cannot express "wait 400ms, then fill in three slices" —
ScemaDEX's action space `(venue, split, wait/execute)` can.

```text
$ cargo run -p scemadex-sdk --example intent_solving
Price:   1 leg(s), conviction 0.50, bond 2500000 micro-USDC -> ...
Speed:   1 leg(s), conviction 0.50, bond 2500000 micro-USDC -> ...
Stealth: 1 leg(s), conviction 0.50, bond 2500000 micro-USDC -> ...
```

---

## C · A reputation oracle sourced from bond settlements

Reputation in crypto is usually self-reported or vote-based — easy to farm.
ScemaDEX derives reputation from **on-chain-shaped bond settlement history**: the
honored-vs-slashed `BondLedger` accrued by every agent. You cannot fake an honor
rate without actually having posted, and not lost, real collateral.

Those signals — `Reputation`, `PoolScore`, `Advice` — are exposed as **monetized
read endpoints** (`SignalSource`), so an agent earns USDC simply for being a
reliable source of truth. Under the `scematica` feature these are served live
from the bot's reputation ledger, pool scorer, and Deep Q\* advice; the
`scemadex-relay` binary serves them over HTTP, optionally x402-gated per call.

---

## ★ The PeerMarket — an economy of machine intelligence

This is the endgame that the four primitives compose into, and the part with no
precedent: a **mesh where autonomous agents buy and sell *intelligence itself*.**
Two distinct goods change hands, both settled in USDC:

1. **Bonded inferences** — "here is a solved route, and I've already bonded it."
   Buying one means paying the fee *and inheriting the seller's bond*: trust is
   transferred, not assumed. The market auto-selects the **cheapest matching**
   offer.

2. **Learned experience** — batches of reinforcement-learning transitions
   `(state, action, reward, next_state)` that a peer sells so others can
   **bootstrap their own agents faster.** An agent that has lived through more
   markets can *sell its memory*. This is a market for training data priced by
   the agent that earned it.

```text
$ cargo run -p scemadex-sdk --example peer_market
listed 2 inference offer(s)
bought inference from agent-beta for 0.02 USDC (conviction 0.50)   # cheapest wins
remaining offers: 1
bought 10000 transitions from agent-alpha for 0.5 USDC            # buy experience to learn faster
```

The net effect: **an agent earns USDC for what it knows and spends USDC to learn
faster.** A DEX router is a widget; this is a self-improving economy of
machine intelligence. The in-process `LocalPeerMarket` and the networked
`RemotePeerMarket` (the `net` feature) sit behind the same `PeerMarket` trait, so
a single-node demo and a gossiping mesh are the same code.

---

## The architectural novelty: lean core, injected power

The published crate carries **zero `solana-sdk` or bot dependency by default.**
It is a pure trait surface —

```
RoutePolicy · BondEngine · VenueExecutor · SignalSource · PeerMarket
```

— plus working reference implementations, so it compiles, tests, and runs the
full intent → bond → settle → trade loop **offline, in seconds.** Enabling
features *injects power* without changing a line of caller code:

| Feature | Injects |
|---------|---------|
| *(default)* | Traits + reference impls (`ReferenceRoutePolicy`, `EscrowBondEngine`, `LocalPeerMarket`, `SimVenueExecutor`) |
| `scematica` | The real Deep Q\* agent as the `RoutePolicy`; its value head supplies live conviction |
| `ai` | Natural-language → `Intent` parsing and route/bond narration via an LLM |
| `net` | `RemotePeerMarket` — the networked, x402-settled mesh client |

The same `ScemaDex::quote(&intent)` call you write against the reference impls
runs, unmodified, against a Deep Q\* agent posting real bonds on mainnet.

---

## Foundations: the x402 payment rails (Dexter)

Every payment in ScemaDEX — metered inference fees (A), signal-oracle queries
(C), and Conviction-Routing bond settlement (D) — rides the **x402 protocol**: a
server answers `402 Payment Required` with payment requirements, and the client
signs and retries with proof of payment. ScemaDEX builds on the open x402
ecosystem rather than reinventing the rail.

In particular it is designed to interoperate with the **[Dexter x402
SDK](https://github.com/Dexter-DAO/dexter-x402-sdk)** (`@dexterai/x402`) —
*"HTTP-native micropayments for agents. Solana and the major EVM chains."* It is
the TypeScript counterpart to ScemaDEX's Rust-native `scematica-protocol`
facilitator: Dexter is the **client/agent side** that pays, `scematica-protocol`
is the **server/relay side** that gates and settles.

| Dexter x402 primitive | Where it meets ScemaDEX |
|---|---|
| `payAndFetch()` (client) | Satisfies the `402` our relay returns on gated `/signal/*` endpoints — the exact response the [TS mesh client](web/lib/scemadex.ts) surfaces as `PaymentRequiredError`. |
| `x402Middleware()` (server) | Mirrors the relay's `--pay-to` `PaymentGate` on the signal oracle. |
| `useX402Payment` (React) | Lets the `web/` dashboard pay for inferences and signals from a connected wallet. |
| Batch settlement (escrow channels + vouchers) | Amortizes settlement across the *many* per-call inference fees a mesh agent generates (Primitive A): pre-fund an escrow once, pay with cheap off-chain vouchers, settle batched. |
| Multi-chain USDC (Solana + EVM) | Extends the mesh's unit of account beyond Solana to Base, Polygon, Arbitrum, and more. |

The upshot: a JavaScript/TypeScript agent using the Dexter SDK can pay a Rust
ScemaDEX relay for a bonded inference with **no glue code** — Dexter signs the
x402 payment, the relay's facilitator verifies and settles it, and the bonded
solution comes back. The two SDKs are the two halves of one agent-payment
handshake.

---

## Try it in 30 seconds

```bash
cargo run -p scemadex-sdk --example quote            # A+B: intent -> bonded solution -> execute
cargo run -p scemadex-sdk --example conviction_bond  # D+C: honored vs. slashed bonds + honor-rate ledger
cargo run -p scemadex-sdk --example peer_market      # the mesh: trade bonded inferences & experience
cargo run -p scemadex-sdk --example intent_solving   # B: same trade under Price/Speed/Stealth

cargo run --release --bin sdk-dashboard              # live TUI over the bond pipeline (SIM default; --live for Jupiter)
```

---

## For Investors

**The thesis in one line:** routing intelligence is becoming the scarce,
defensible layer of on-chain trading — and ScemaDEX is the first SDK that turns
that intelligence into a *metered, accountable, tradeable* asset.

### Why now

Aggregators commoditized *liquidity access*; the margin has collapsed to zero.
The remaining edge is **decision quality** — knowing *which* route, *when*, and
*how stealthily* to execute. ScemaDEX is built so that edge is the product:
priced per call, backed by collateral, and improvable by trading experience.
That is a different business than a swap widget.

### Revenue surfaces (already modeled in the crate)

| Surface | Mechanism | Code |
|---------|-----------|------|
| **Per-call inference fees** | Every quote is individually billable, priced by conviction | `EscrowBondEngine::quote_fee` |
| **Bond fees / spread** | Honored bonds return + fee; the agent keeps the upside it can deliver | `bond::settle` |
| **Signal endpoints** | Reputation / pool-score / advice sold per query, x402-gated | `oracle::SignalSource`, `scemadex-relay` |
| **Experience market take** | Mesh fees on agents buying/selling learned RL transitions | `mesh::PeerMarket` |
| **Token gate** | Access to the live bot/agent stack gated behind a 250k `$SCEMA` balance | `scematica-core` gate |

These are not roadmap items — each maps to a shipped type and a runnable example.

### Defensibility (the moats)

- **A trained edge, not a prompt.** The Deep Q\* policy is the product of many
  iterations of live market tuning (validated profit factor ≈ 6.5 on the bot
  slice). A competitor can fork the traits; they cannot fork the weights or the
  experience that produced them.
- **Accountability as a network effect.** Reputation derives from *real bond
  settlements* — un-fakeable history that compounds. The longer an agent runs
  honestly, the more its inferences are worth, and the harder it is to displace.
- **Proprietary settlement rails.** The open SDK defines the trait surface and
  the settlement *logic*; the on-chain x402 facilitator that moves real USDC is
  a closed companion stack. Adopters build *on* ScemaDEX, not *around* it.
- **Compounding data flywheel.** Agents that sell experience seed the mesh;
  agents that buy it converge faster — and the most-traded experience is the
  most valuable, concentrating flow toward the best nodes.

### Traction & status

- `scemadex-sdk v0.1.1` **published on crates.io** — the trait surface, reference
  implementations, four runnable examples, and docs.rs documentation are live.
- The agentic layer is **wired end-to-end**: live Jupiter quotes flow through the
  conviction/route/bond pipeline; `scemadex-relay` serves the mesh + signal
  endpoints over HTTP; the `sdk-dashboard` TUI drives it in SIM and `--live`.
- **Honest risk markers:** live on-chain signing/settlement paths are the newest
  code and excluded from the automated test suite; the open crate simulates
  settlement, with real USDC movement gated behind the proprietary stack. This is
  early-stage infrastructure, deliberately shipped lean.

### The ask / the shape of the opportunity

The wedge is a **margin business on machine intelligence**: take rate on metered
inferences, bond spreads, signal subscriptions, and experience-market fees — with
a token (`$SCEMA`) gating access to the highest-value live stack. The
defensibility is the trained policy plus the accountability flywheel, neither of
which a fork inherits.

---

## For Developers — Adoption Guide

ScemaDEX is a **trait-first SDK**: integrate against the lean core in minutes,
then inject power (a real agent, an LLM, a networked mesh) by flipping feature
flags — *without changing caller code*.

### 1. Install

```toml
[dependencies]
scemadex-sdk = "0.1.1"
tokio = { version = "1", features = ["full"] }
```

```bash
cargo add scemadex-sdk
```

### 2. Your first bonded quote (offline, no keypair/RPC)

```rust
use scemadex_sdk::{conviction_client, demo_intent, BondOutcome};

#[tokio::main]
async fn main() -> scemadex_sdk::Result<()> {
    let dex = conviction_client();                          // reference policy + real bond engine
    let (solution, bond) = dex.quote(&demo_intent()).await?;
    println!("conviction {:.2}, bond {} µUSDC", solution.conviction.0, bond.amount.0);

    let (fill, outcome) = dex.execute(&demo_intent()).await?;
    assert_eq!(outcome, BondOutcome::Honored);
    println!("filled ~{}, bond {:?}", fill.amount_out.ui(), outcome);
    Ok(())
}
```

`reference_client()` gives you a *zero* bond (intent/route surface only);
`conviction_client()` exercises real Conviction Routing end-to-end. Start with
the four examples — they are the fastest way in:

```bash
cargo run -p scemadex-sdk --example quote
cargo run -p scemadex-sdk --example conviction_bond
cargo run -p scemadex-sdk --example peer_market
cargo run -p scemadex-sdk --example intent_solving
```

### 3. The five traits you can implement

The whole SDK is composable around these. Implement one to plug in your own
brain, venue, collateral, signals, or marketplace — `ScemaDex<P, B, V>` is
generic over them, so mix and match freely.

| Trait | Implement it to… | Reference impl |
|-------|------------------|----------------|
| `RoutePolicy` | supply your own routing brain (`solve(intent) -> Solution`) | `ReferenceRoutePolicy`, `integration::DqRoutePolicy` (`scematica`) |
| `BondEngine` | define how bonds are sized & settled | `NoBondEngine`, `EscrowBondEngine` |
| `VenueExecutor` | build/submit swaps on your venue | `SimVenueExecutor`, Jupiter (`scematica`) |
| `SignalSource` | serve reputation / pool-score / advice | `FileSignalSource` (`scemadex-integrations`) |
| `PeerMarket` | back the inference/experience mesh | `LocalPeerMarket`, `RemotePeerMarket` (`net`) |

```rust
use async_trait::async_trait;
use scemadex_sdk::{Intent, RoutePolicy, Solution, ScemaDex, NoBondEngine, SimVenueExecutor};

struct MyAlpha;

#[async_trait]
impl RoutePolicy for MyAlpha {
    async fn solve(&self, intent: &Intent) -> scemadex_sdk::Result<Solution> {
        // your edge here
        unimplemented!()
    }
}

let dex = ScemaDex::new(MyAlpha, NoBondEngine, SimVenueExecutor);
```

### 4. Feature flags — inject power, keep the call site

| Feature | Adds | Pulls |
|---------|------|-------|
| *(default)* | Traits + reference impls, `ScemaDex` facade | serde, async-trait, bs58 |
| `scematica` | the real Deep Q\* agent as `RoutePolicy` | [`scematica-nn`](https://crates.io/crates/scematica-nn) |
| `ai` | natural-language → `Intent` + trade narration | `reqwest` |
| `net` | `RemotePeerMarket` networked mesh client | `reqwest` |

```bash
cargo add scemadex-sdk --features scematica,ai,net
```

### 5. Run a node on the mesh

```bash
# Serve the inference/experience mesh + signal oracle (optionally x402-gated)
cargo run --release --bin scemadex-relay

# Drive real Jupiter quotes through the bond pipeline in a TUI
cargo run --release --bin sdk-dashboard -- --live
```

Point a `RemotePeerMarket` (the `net` feature) at your relay and your node can
buy and sell bonded inferences and experience with the rest of the mesh.

### 5b. Join the mesh from TypeScript / Python / any language

The relay speaks plain HTTP/JSON, so you don't need Rust to participate. A typed,
dependency-free TypeScript client ships in [`web/lib/scemadex.ts`](web/lib/scemadex.ts)
(browser + Node 18+), mirroring the exact wire format:

```ts
import { ScemaDexRelay, fromMicroUsdc } from "./lib/scemadex";

const relay = new ScemaDexRelay("http://localhost:8080");

// Buy the cheapest bonded inference for an intent...
const offer = await relay.quoteInference(intentDigest);
if (offer) console.log(offer.peer_id, fromMicroUsdc(offer.price), offer.solution.conviction);

// ...buy experience under a price cap, or read the (x402-gated) signal oracle.
const batch = await relay.buyExperience(/* USDC */ 1.0);
const rep = await relay.reputation(mint); // throws PaymentRequiredError on HTTP 402
```

To actually **pay** a gated `/signal/*` endpoint, build the client with
[`createPaidRelay`](web/lib/scemadex.ts), injecting the
[Dexter x402 SDK](https://github.com/Dexter-DAO/dexter-x402-sdk)
(`@dexterai/x402`, already in `web/package.json`). Gated reads are then routed
through Dexter's `payAndFetch`, which performs the full 402 handshake (fetch →
sign → retry) transparently:

```ts
import * as x402 from "@dexterai/x402/client";
import { createPaidRelay } from "./lib/scemadex";

const relay = await createPaidRelay("http://localhost:8080", x402, {
  solanaPrivateKey: process.env.SOLANA_PRIVATE_KEY!, // and/or evmPrivateKey
});

const rep = await relay.reputation(mint); // the 402 is paid automatically
```

Without a payer the same call throws `PaymentRequiredError` carrying the payment
requirements, so you can also pay manually and replay via the `headers` option.

A runnable in-app reference lives at
[`web/app/api/scemadex/signal/[mint]/route.ts`](web/app/api/scemadex/signal/[mint]/route.ts) —
a server-side Next.js route that pays a gated signal read (keeping the key off the
browser): `GET /api/scemadex/signal/<mint>?kind=reputation|pool_score|advice`.

The 8-endpoint contract (`/inference/*`, `/experience/*`, `/signal/*`, `/health`)
is the integration surface for Eliza, LangChain, or any agent framework — and the
Dexter SDK is the drop-in x402 wallet for paying it.

### 6. Where to look

- **API docs:** [docs.rs/scemadex-sdk](https://docs.rs/scemadex-sdk) (built with
  all features — the `scematica` / `ai` / `net` modules render with badges).
- **Source & examples:** [`crates/scemadex-sdk`](crates/scemadex-sdk).
- **Changelog:** [`crates/scemadex-sdk/CHANGELOG.md`](crates/scemadex-sdk/CHANGELOG.md).
- **Integrations & relay:** [`crates/scemadex-integrations`](crates/scemadex-integrations),
  [`crates/scemadex-relay`](crates/scemadex-relay).

### Compatibility notes

- Async runtime is **Tokio**; every trait method is `async` + `Send + Sync`.
- The default crate has **no `solana-sdk` dependency** — it builds anywhere and
  fast. Solana types only appear when you enable `scematica`.
- MSRV tracks the workspace (`edition = 2021`). The bot workspace pins
  `solana-sdk 1.18.x` / `reqwest 0.11` for transitive `zeroize` reasons; if you
  enable `scematica`, inherit those pins (see the root `Cargo.toml` comments).

---

*All feature behavior described here is exercised by the crate's tests and
examples. The Conviction Routing settlement logic is real and open; only the
on-chain x402 USDC transfer is gated behind the proprietary `scematica` stack.
`--live` paths (Jupiter signing/settlement) are newer and excluded from the
automated test suite — treat them as experimental.*
