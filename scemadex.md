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

## Try it in 30 seconds

```bash
cargo run -p scemadex-sdk --example quote            # A+B: intent -> bonded solution -> execute
cargo run -p scemadex-sdk --example conviction_bond  # D+C: honored vs. slashed bonds + honor-rate ledger
cargo run -p scemadex-sdk --example peer_market      # the mesh: trade bonded inferences & experience
cargo run -p scemadex-sdk --example intent_solving   # B: same trade under Price/Speed/Stealth

cargo run --release --bin sdk-dashboard              # live TUI over the bond pipeline (SIM default; --live for Jupiter)
```

---

*All feature behavior described here is exercised by the crate's tests and
examples. The Conviction Routing settlement logic is real and open; only the
on-chain x402 USDC transfer is gated behind the proprietary `scematica` stack.
`--live` paths (Jupiter signing/settlement) are newer and excluded from the
automated test suite — treat them as experimental.*
