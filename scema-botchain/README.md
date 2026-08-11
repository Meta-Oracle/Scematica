# scema-botchain

The BOT Chain (EVM, chain **677**) port of Scematica. **The Solana bot is not going
anywhere** — it stays where the measured edge is, and this tree grows beside it.

```powershell
cargo build
cargo test
cargo run -p botchain-probe -- --blocks 20000
```

## Why this is a separate cargo workspace

Not tidiness — a hard dependency conflict, already documented in the root `Cargo.toml`:

> `reqwest` must stay at 0.11: 0.12 pulls rustls 0.23 which requires `zeroize >= 1.7`,
> conflicting with `curve25519-dalek 3` (used by `solana-sdk`'s ed25519-dalek) which caps
> at `< 1.4`.

Every current EVM stack wants reqwest 0.12 / rustls 0.23. One workspace means one
lockfile means one resolved `zeroize`, and no version satisfies both trees. Two
workspaces and the question never arises. `scema-botchain` is therefore in the root
workspace's `exclude` list, alongside `programs/`.

**The rule that follows:** nothing in here may depend on a crate that pulls `solana-sdk`.
The chain-agnostic crates — `scematica-nn`, `scematica-sentience`, `scemadex-sdk` — are
safe to reach for by path. `scematica-core` and everything rooted on it are not.

## What the chain actually looks like (measured August 2026)

Before porting a sniper, the question is whether there is anything to snipe. There is not,
today:

| Window | V3-style factory | CA factory |
|---|---|---|
| 20,000 blocks (~3.7 h) | 0 | 0 |
| 200,000 blocks (~1.5 d) | 0 | 0 |
| 1,000,000 blocks (~7.7 d) | **2** | 0 |

Two pool creations in roughly eight days. For comparison, the Solana side's edge
(PF 6.50) exists because Raydium produces continuous new-pool flow with real liquidity
and real buyers.

Supporting context, all read from the chain rather than from documentation:

- Network utilization **0.29%**; ~119k tx/day, but a large share is the per-block
  `BOTValidatorSet.deposit` — Parlia system activity, not users. Exclude
  `0x…1000` before drawing conclusions about volume.
- In a 50-transaction sample of live traffic: **2 swaps**.
- Token list is four tokens with real holders (CA 451k, USDT 287k, WBOT 57k, Money 24k)
  followed by a tail of 2-to-6-holder test deployments.
- Consensus is **Parlia PoSA** — block `extraData` carries `geth`/`go1.26.5` with
  `difficulty: 0x2`. BOT Chain is a BSC fork.

**None of this says the port is a bad idea permanently.** It says the sniper has nothing
to do *yet*. `botchain-probe` exists to answer that question repeatably, so the decision
is a measurement rather than an opinion. Re-run it.

## A note on the upstream repo

`BOTChain-bot/BOTCHAIN` contains no BOT Chain code. It is a single-commit copy of
`bnb-chain/bsc`: `go.mod` still declares `module github.com/ethereum/go-ethereum`, the
bootnodes are BSC's, and `params/config.go` defines chains 1, 5, 56, 97, 968, 17000,
11155111 — **677 is absent**. It cannot run a BOT Chain node. Nothing here depends on it.

It does explain where the testnet chain id came from: 968 is BSC's `RialtoChainConfig`.

## Chain-ID collision (testnet)

968 is registered on ChainList as **Datagram**, not BOT Chain testnet, and is also BSC's
Rialto. So **never identify the testnet by chain id alone** — a registry lookup resolves
968 to the wrong chain. Pin the endpoint first, then verify the id against it. That
ordering is what `Client::verify()` enforces.

Mainnet 677 is registered cleanly.

## Endpoints

`Client` walks an ordered list and reports which endpoint answered, because a read served
by the explorer proxy and one served by a node are not interchangeable — the proxy cannot
broadcast a transaction.

| Endpoint | Kind | Note |
|---|---|---|
| `https://rpc.botchain.ai` | node | official; ~615 ms, verified `0x2a5` |
| `https://scan.botchain.ai/api/eth-rpc` | explorer proxy | **reads only**; Cloudflare-fronted, answered when the node RPC briefly did not |

`BOTCHAIN_RPC_URL` puts a private node at the front of the list; the built-ins remain as
fallback. `BOTCHAIN_NETWORK` selects `mainnet` (default) or `testnet`.

## Layout

```
crates/
  botchain-core/    networks, venues, JSON-RPC with endpoint failover
  botchain-probe/   measures reachability and pool-creation flow
```

Venue addresses in `chain.rs` were resolved on-chain by calling `factory()` on routers
found in live transactions — not copied from a docs page. `CASwapRouter` reverts on
`WETH()`, so it is **not** a stock Uniswap-V2 router; do not assume a V2 ABI for it.
