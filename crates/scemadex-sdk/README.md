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
because it depends on a proprietary protocol stack.

## License

MIT
