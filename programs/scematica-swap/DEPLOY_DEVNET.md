# Deploying `scematica_swap` to Devnet

Program ID (declared in `src/lib.rs` and `Anchor.toml`):
`7ycLhn5WsodcbYwV9ecQDd3qWQhKgGzgMK5pc4CYXkEc`

A prebuilt artifact already exists at
`target/deploy/scematica_swap.so` (~210 KB), so you can deploy without rebuilding.

> ⚠️ The committed `target/deploy/scematica_swap-keypair.json` was exposed in git
> history — treat it as **devnet-throwaway only**. Do not reuse it on mainnet;
> rotate before any mainnet deploy.

Pick one path. **Path A needs nothing installed locally** and matches testing on
Solana Playground.

---

## Path A — Solana Playground (recommended; no local toolchain)

1. Open **https://beta.solpg.io**.
2. Bottom-left: set the cluster to **Devnet** and connect/create the Playground
   wallet. Airdrop gas in the Playground terminal:
   ```
   solana airdrop 2
   ```
3. Create a new **Anchor (Rust)** project, then paste this repo's
   `programs/scematica-swap/src/lib.rs` into the project's `src/lib.rs`.
4. Build, then deploy:
   ```
   build
   deploy
   ```
   Playground manages the program keypair and prints the **deployed program ID**
   on devnet. Use that ID from the Playground client/tests.
5. Test from the Playground **Test** tab or its built-in client.

> The on-chain program uses `anchor-lang` with `init-if-needed`; Playground
> supports this. If the default Anchor version errors, pin it in the Playground
> project settings to a 0.29–0.32 line.

---

## Path B — Solana CLI, deploy the prebuilt `.so` (fastest if `solana` is installed)

Deploys the existing artifact under the declared program ID on devnet:

```bash
solana config set --url devnet
solana airdrop 2                                   # fund your deployer wallet
solana program deploy \
  programs/scematica-swap/target/deploy/scematica_swap.so \
  --program-id programs/scematica-swap/target/deploy/scematica_swap-keypair.json
```

Verify:
```bash
solana program show 7ycLhn5WsodcbYwV9ecQDd3qWQhKgGzgMK5pc4CYXkEc --url devnet
```
Explorer: https://explorer.solana.com/address/7ycLhn5WsodcbYwV9ecQDd3qWQhKgGzgMK5pc4CYXkEc?cluster=devnet

---

## Path C — Anchor, rebuild + deploy (needs `anchor` + the SBF toolchain)

`Anchor.toml` now has a `[programs.devnet]` entry, so:

```bash
anchor build
anchor deploy --provider.cluster devnet
```

To deploy under a **fresh** program ID (recommended over reusing the exposed
keypair), generate a new keypair and sync it before building:
```bash
solana-keygen new -o target/deploy/scematica_swap-keypair.json --force
anchor keys sync          # rewrites declare_id! + Anchor.toml to the new pubkey
anchor build && anchor deploy --provider.cluster devnet
```

---

## After deploy

Note the live devnet program ID and hand it to the client/tests. The
`scemadex-settle` reference settler (USDC bond movement) is independent of this
program and runs against devnet directly — see
`crates/scemadex-settle/examples/devnet_settlement.rs`.
