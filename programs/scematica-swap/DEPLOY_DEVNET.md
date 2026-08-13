# Deploying `scematica_swap`

Program ID (declared in `src/lib.rs` and `Anchor.toml`):
`7rRHfgQphASzDTGEyLUEsh9daZ2hRXZA1GP9MPvSxXBh`

> **Keypair rotated 2026-08-13.** The previous program keypair really was committed to
> git history (`git log --all -- programs/scematica-swap/target/deploy/scematica_swap-keypair.json`
> shows it), so both the old ID `7DTvC8pF2QE7bQ74CZn3QDBxwyd5fMbxEU9zMcbPzN8e` and the
> even older `7ycLhn5Wsodc…` this file used to name are dead. The current keypair exists
> only under `target/deploy/`, which `.gitignore` covers via `programs/*/target/`. It is
> the sole authority able to deploy at this address — **back it up**.

Build the artifact with the repo's build script, not `cargo-build-sbf` directly:

```powershell
powershell -ExecutionPolicy Bypass -File tools/build-programs.ps1 -Programs scematica-swap
```

The script exists because `cargo-build-sbf` reports an SBF stack-frame overflow and then
exits 0 anyway, emitting a `.so` that deploys fine and fails at runtime. It greps for
that and fails the build. See `programs/scemadex-vault/Cargo.toml` for a program that
actually hit this.

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
solana program show 7rRHfgQphASzDTGEyLUEsh9daZ2hRXZA1GP9MPvSxXBh --url devnet
```
Explorer: https://explorer.solana.com/address/7rRHfgQphASzDTGEyLUEsh9daZ2hRXZA1GP9MPvSxXBh?cluster=devnet

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

Note the live devnet program ID and hand it to the client/tests.

The `scemadex-settle` settling node (USDC bond movement) is independent of this
program and runs against devnet directly. Once you have a funded agent keypair, a
devnet SPL "USDC" mint, and a beneficiary token account, **one command settles a
slashed bond on-chain and prints the explorer link**:

```bash
cargo run -p scemadex-settle --example devnet_settlement -- \
  --keypair agent.json \
  --usdc-mint <DEVNET_MINT> \
  --beneficiary <CALLER_USDC_TOKEN_ACCOUNT>
# add --mode honor to run the no-transfer (guarantee-met) path
```

The node header documents the one-time `solana` / `spl-token` setup. See
`crates/scemadex-settle/examples/devnet_settlement.rs`.
