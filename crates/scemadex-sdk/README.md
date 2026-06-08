# scemadex-sdk

**An agentic liquidity layer for Solana — not "a DEX."** In ScemaDEX the routing
*intelligence itself* is a metered, learning, accountable product: autonomous
agents solve swap **intents**, **bond** their promises, sell their inferences,
and trade learned experience with one another.

```rust
use scemadex_sdk::{reference_client, demo_intent};

# async fn run() -> scemadex_sdk::Result<()> {
let dex = reference_client();
let (solution, bond) = dex.quote(&demo_intent()).await?;   // solve + escrow bond
assert!(solution.route.splits_valid());
let _ = bond;
# Ok(()) }
```

## Live viewer

Install the crate and run the bundled `scemadex` viewer — a terminal dashboard
that drives the whole pipeline (`intent → solve → conviction bond → settle →
peer market`) live, fully offline:

```bash
cargo install scemadex-sdk
scemadex            # space = pause/resume · s = step · q = quit
```

The viewer ships behind a default `cli` feature. Depending on the crate as a
**library** and want only the lean trait surface? Opt out of the TUI stack:

```toml
scemadex-sdk = { version = "0.1", default-features = false }
```

## The four composing primitives

| | Primitive | Where |
|--|--|--|
| **A** | **Metered inference routing** — quotes produced by a learning policy, sold per-call | `policy`, `EscrowBondEngine::quote_fee` |
| **B** | **Intent solving** — express *what* you want (`Objective::{Price,Speed,Stealth}`), not a path | `intent` |
| **C** | **Signal / reputation oracle** — reputation, pool scores, advice as paid endpoints | `oracle`, `BondLedger` |
| **D** | **Conviction Routing** *(defining)* — the policy escrows a **slashable bond** against its own promise | `bond` |

**Headline:** a `PeerMarket` mesh where agents **sell bonded inferences and
learned experience, and buy better ones from peers** — an economy of machine
intelligence, settled in stablecoins.

## Lean core, injected power

The published crate has **no `solana-sdk` dependency by default**. It defines the
trait surface — `RoutePolicy`, `BondEngine`, `VenueExecutor`, `SignalSource`,
`PeerMarket` — plus working reference implementations (`ReferenceRoutePolicy`,
`EscrowBondEngine`, `LocalPeerMarket`, `SimVenueExecutor`).

### Feature flags

| Feature | Adds | Pulls |
|---------|------|-------|
| *(default)* | Traits, reference impls, `ScemaDex` facade | serde, async-trait, bs58 |
| `scematica` | `integration::DqRoutePolicy` — the real Deep Q\* agent as policy | [`scematica-nn`](https://crates.io/crates/scematica-nn) |
| `ai` | NL → `Intent` parsing + trade narration | `reqwest` |
| `net` | `RemotePeerMarket` — networked mesh client | `reqwest` |

The x402-settled bond engine lives in a separate, unpublished companion crate
because it depends on a proprietary protocol stack. The conviction-weighted
settlement *state machine*, however, ships in the open as `EscrowBondEngine` —
only the on-chain USDC transfer is gated.

## Runnable examples

All run offline — no keypair, RPC, or `solana-sdk`:

```bash
cargo run -p scemadex-sdk --example quote            # A+B: intent -> bonded solution -> execute
cargo run -p scemadex-sdk --example conviction_bond  # D+C: honored vs. slashed bonds + honor-rate ledger
cargo run -p scemadex-sdk --example peer_market      # the mesh: trade bonded inferences & experience
cargo run -p scemadex-sdk --example intent_solving   # B: same trade under Price/Speed/Stealth
```

`reference_client()` wires a *zero* bond (`NoBondEngine`) for the bare
intent/route surface; use **`conviction_client()`** to exercise real
Conviction Routing (conviction-sized, slashable bonds with a ledger) end-to-end.

## License

MIT
