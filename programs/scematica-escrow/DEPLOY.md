# Deploying `scemadex_escrow`

The optimistic bond-escrow program is the **trustless-custody** half of ScemaDEX
Settlement v2: it holds a Conviction-Routing bond (and any challenge stake) in a
program-owned vault through `Escrowed → Provisional → Disputed → Finalized`, and only
moves money at `settle`. It is the on-chain mirror of
`scemadex_sdk::SettlementMachine`; the off-chain driver is
`scemadex_settle::OptimisticUsdcSettler`.

Program ID: `Fu5nDuRRBTTJGNBMcFC1hHvBQybtiCECeNzUBRHmVwLz`

> Assigned 2026-08-13, replacing the placeholder `Esc1DExBondCust0dy111…` — which was
> never a valid pubkey at all (`solana program show` rejects it with `Invalid
> parameters`), so nothing could have been deployed under it. The keypair lives at
> `target/deploy/scemadex_escrow-keypair.json`, gitignored via `programs/*/target/`.

Like `scematica-swap`, this program is **excluded from the cargo workspace** and built
separately with the SBF toolchain:

```powershell
powershell -ExecutionPolicy Bypass -File tools/build-programs.ps1 -Programs scemadex-escrow
```

> **This program had never been compiled** before 2026-08-13. The first real build
> surfaced two `E0716` borrow-lifetime errors in `settle` and `transfer_from_vault`
> (`&[seeds]` inline is a temporary the `CpiContext` outlives), now fixed. Weigh that
> against the audit warning at the bottom of this file: an uncompiled program is also an
> unexecuted one, and none of the lifecycle below has ever run.

---

## Path A — Solana Playground (no local toolchain)

1. Open **https://beta.solpg.io**, set the cluster to **Devnet**, connect a wallet,
   and `solana airdrop 2` in the Playground terminal.
2. New **Anchor (Rust)** project → paste `programs/scemadex-escrow/src/lib.rs` into the
   project's `src/lib.rs`.
3. `build` then `deploy`. Playground manages the program keypair and prints the
   deployed program ID. Use that ID from the client/tests.

> Uses `anchor-lang` 0.29 with `init-if-needed`. If the default Playground Anchor
> version errors, pin it to a 0.29–0.32 line in project settings.

## Path B — Anchor CLI (needs `anchor` + SBF toolchain)

```bash
cd programs/scemadex-escrow
solana-keygen new -o target/deploy/scemadex_escrow-keypair.json --force
anchor keys sync          # rewrites declare_id! to the new pubkey
anchor build
anchor deploy --provider.cluster devnet
```

---

## Instruction lifecycle

| Instruction | Signer | From → to | Money |
|---|---|---|---|
| `escrow` | agent | — → Escrowed | agent → vault (bond) |
| `mark_provisional(honored)` | authority | Escrowed → Provisional | none (opens window) |
| `file_challenge(stake)` | challenger | Provisional → Disputed | challenger → vault (stake) |
| `resolve(challenger_won)` | authority | Disputed → Finalized | none |
| `finalize_timeout` | anyone | Provisional/Escrowed → Finalized | none (window/deadline) |
| `settle` | anyone | Finalized → closed | vault → beneficiaries |

`settle` disburses:

- **Honored** → the bond (plus any forfeited challenge stake) returns to the agent.
- **Slashed** → the bond splits four ways per `SlashRouting`
  (caller / challengers / insurance / lineage, bps summing to 10_000; the caller
  share absorbs rounding dust) and the challenger recovers its stake.

The vault is closed on `settle` and its rent refunded to the agent.

---

## Trust model

Optimistic finality: funds are custodied by the program (not any facilitator) and
only move at `settle`, after `Finalized`. The dispute window is the on-chain challenge
period — the same shape as an optimistic rollup — so a challenger (or a verified
succinct proof resolving the dispute off-chain via `resolve`) can flip a bad
inference's outcome *before* the money is irreversible.

> ⚠️ Reference implementation. Audit before mainnet. The placeholder program ID must
> be rotated to a keypair you control.
