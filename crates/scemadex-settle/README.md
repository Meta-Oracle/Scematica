# scemadex-settle

**Open devnet reference settler for ScemaDEX Conviction-Routing bonds.**

[`scemadex-sdk`](https://crates.io/crates/scemadex-sdk) ships the bond
*settlement state machine* but moves no money (it has no `solana-sdk`
dependency). This crate closes the loop on **devnet**: it wraps that state
machine and, when a bond is **slashed**, makes a real on-chain SPL-USDC transfer
of the bond to the caller — so you can run the entire Conviction-Routing loop
(quote → bond → execute → settle on-chain) for free, with no proprietary stack.

```rust
use scemadex_settle::DevnetUsdcSettler;
use scemadex_sdk::{BondEngine, demo_intent, ReferenceRoutePolicy, RoutePolicy, Fill, Amount};

let settler = DevnetUsdcSettler::devnet(agent, usdc_mint, beneficiary_token_account);
let solution = ReferenceRoutePolicy.solve(&demo_intent()).await?;
let bond = settler.escrow(&solution).await?;
// an under-delivering fill slashes the bond -> real devnet USDC moves to the caller
let (outcome, signature) = settler.settle_onchain(&bond, &under_fill).await?;
```

See [`examples/devnet_settlement.rs`](examples/devnet_settlement.rs) for a full
runnable walkthrough (with the `solana`/`spl-token` setup commands).

## ⚠️ Devnet / test only

A *reference* settler: it does a plain SPL transfer on slash, with **no** escrow
custody, fee collection, x402 metering, dispute windows, or mainnet safety. Do
not point it at mainnet. The production mainnet rail (x402-settled, fee-abstracted,
trust-networked) is a separate, closed component.

## License

MIT
