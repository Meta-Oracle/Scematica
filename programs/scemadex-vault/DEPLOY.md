# Deploying the Escrow Market vault

The custody guarantee this program makes is **not** established by its source code. It
is established by three facts about the deployed artifact, only one of which is visible
in `lib.rs`. This document is how you produce the other two — and, more importantly, how
a stranger checks all three without trusting you.

> **The order matters.** Deploy to devnet, exercise the full lifecycle, and only then go
> to mainnet. Finalizing an upgrade authority is irreversible: a bug shipped to a
> finalized program can never be patched, and the funds in it can never be rescued.
> That is the deal — it is what makes the vault trustworthy, and it is why the devnet
> pass is not optional.

---

## 0. Prerequisites

```bash
solana --version          # 1.18.x, matching the program's solana-program pin
anchor --version          # 0.29.x, matching Cargo.toml
solana config get         # confirm the cluster you think you're on
```

Neither tool ships with this repo. On Windows, install via WSL or use the Solana
release installer; `anchor` comes from `avm`.

---

## 1. Build

```bash
cd programs/scemadex-vault
anchor build
```

Record the build hash — it is what a third party reproduces to prove the deployed
bytecode matches this source:

```bash
sha256sum target/deploy/scemadex_vault.so
```

## 2. Set the program ID

The `declare_id!` in `lib.rs` is a placeholder and **will not work as-is**. Generate the
real keypair, put its pubkey in the source, and rebuild:

```bash
solana-keygen new -o target/deploy/scemadex_vault-keypair.json
solana address -k target/deploy/scemadex_vault-keypair.json     # -> <PROGRAM_ID>
# paste <PROGRAM_ID> into declare_id!(...) in src/lib.rs, then:
anchor build
```

Add it to the workspace `Anchor.toml` under `[programs.devnet]` / `[programs.mainnet]`.

## 3. Devnet deploy and full lifecycle test

```bash
solana config set --url devnet
solana airdrop 5
anchor deploy --provider.cluster devnet
```

Exercise every path before you even consider mainnet. **Do not skip the negative
tests** — they are the ones asserting the guarantee:

| # | Test | Expected |
|---|---|---|
| 1 | `initialize_vault` for a devnet token pair | vault + two PDA token accounts created |
| 2 | `deposit` with `backing_amount = 0` | fails `ZeroBacking` |
| 3 | `deposit` with `lock_secs = 3600` | fails `LockOutOfRange` |
| 4 | `deposit` with `token_amount = 0`, backing > 0 | succeeds (pure-reserve deposit) |
| 5 | `withdraw` before `unlock_unix` | fails `StillLocked` |
| 6 | `withdraw` signed by a **different** wallet | fails `NotDepositor` |
| 7 | `extend_lock` to an **earlier** time | fails `LockNotExtended` |
| 8 | `withdraw` after unlock, correct depositor | returns both legs exactly; position closed |
| 9 | replay the same `withdraw` transaction | fails — position account no longer exists |
| 10 | two depositors in one vault; A withdraws | B's funds untouched, totals correct |

Test 10 is the one that catches the worst class of bug: `withdraw` transfers the
*recorded* amounts, never the vault balance, precisely so one position cannot reach
another's funds. Verify the balances rather than trusting the return code.

## 4. Mainnet deploy

```bash
solana config set --url mainnet-beta
anchor deploy --provider.cluster mainnet
```

## 5. Finalize — the step that makes the guarantee real

Everything above this line is a normal, fully custodial program. Whoever holds the
upgrade authority can redeploy it with a `drain_everything` instruction and take every
lamport in every vault. **A PDA vault under an upgradeable program is not
non-custodial.** This one command is what changes that, and it cannot be undone:

```bash
solana program set-upgrade-authority <PROGRAM_ID> --final
```

Verify immediately:

```bash
solana program show <PROGRAM_ID>
```

```text
Authority: none          # <- required. Anything else means it is still custodial.
```

If that line names any address at all — including yours — the vault is not yet what it
claims to be, and you should not point anyone at it.

---

## 6. Publish the proof-of-reserve page (scematica.org/escrow)

The page is already part of the Next app in `web/` and ships with every deploy of the
site — there is no separate service to stand up. What it needs is configuration, and the
**order matters**: the program must exist on-chain first, or the page correctly reports
that it does not.

```bash
# Vercel → the scematica.org project → Settings → Environment Variables
NEXT_PUBLIC_ESCROW_PROGRAM_ID = <PROGRAM_ID from step 2>
RPC_ENDPOINT                  = https://mainnet.helius-rpc.com/?api-key=...
```

Two things about those two variables:

- `NEXT_PUBLIC_ESCROW_PROGRAM_ID` is inlined into the client bundle at build time, so
  **changing it requires a redeploy**, not just a settings save. It is public by nature —
  a program ID is not a secret.
- `RPC_ENDPOINT` has **no** `NEXT_PUBLIC_` prefix and is read only by
  `web/lib/escrow/rpc.ts`, which throws if it is ever imported into a browser bundle. It
  may safely carry a real key. Do not reuse `NEXT_PUBLIC_RPC_ENDPOINT` here: that one is
  served to every visitor.

Unset, the page is still correct — `/api/escrow/vault` returns `503 not_configured` and
the UI reads "Not deployed". That is the designed behaviour for a program that does not
exist yet, and it is why the program ID is not defaulted to the source placeholder.

Verify against the live domain once deployed:

```bash
# Before the program exists — expect 503 not_configured
curl -s "https://scematica.org/api/escrow/vault?token=<MINT>&backing=<MINT>" | jq .

# After: a real vault. `measuredAt.slot` is what makes the figure checkable, and
# `rpc.authenticated` tells you whether it came from your keyed node or the public
# fallback.
curl -s "https://scematica.org/api/escrow/vault?token=<TOKEN_MINT>&backing=<WBTC_MINT>" | jq .
```

A vault that has never been initialised returns `502 read_failed` with `no vault at
<pda>`. That is distinct from `not_configured` (no program) and from a zero balance (a
real, empty vault) — the page keeps the three apart on purpose, because "could not read
the reserve" and "the reserve is zero" are different claims and only one of them is an
accusation.

---

## What a stranger should check before depositing

Publish this list; it is the actual product. None of it requires trusting the operator.

1. **Upgrade authority is gone.**
   `solana program show <PROGRAM_ID>` → `Authority: none`.
   Without this, nothing below matters.

2. **The bytecode matches this source.**
   `anchor build` from this commit, then compare `sha256sum` against the deployed
   program. A verified build (e.g. `solana-verify`) is the stronger form.

3. **There is no admin instruction.**
   Read the IDL: `initialize_vault`, `deposit`, `extend_lock`, `withdraw`. That is the
   complete set. No pause, no `set_authority`, no emergency path, no fee recipient.

4. **The vaults are PDA-owned.**
   `spl-token account-info <TOKEN_VAULT>` → owner is the vault PDA, not a wallet.

5. **The reserve is really there.**
   Read `total_backing_locked` on the vault account and compare it to the backing
   vault's actual token balance. The balance must be **greater than or equal to** the
   recorded total — not exactly equal.

   The inequality is not slack, it is a fact about SPL tokens: anyone may transfer into
   any token account at any time, so a stranger can donate into the vault and push the
   balance above the sum of live positions. Donations are **permanently stuck** — no
   instruction in this program can move unrecorded funds, and adding a sweeper would
   mean adding the privileged role the whole design exists to avoid. Stuck donations are
   the strictly better failure.

   `balance < total_backing_locked` is the condition that should alarm you. It would
   mean the accounting and the tokens disagree, and `withdraw` will start failing with
   `AccountingUnderflow` for somebody.

   Any USD or "percent backed" figure is yours to compute — this program deliberately
   stores no price and consults no oracle, so there is no feed to manipulate and no
   number to argue with.

---

## Choosing the backing mint

BTC and ETH do not exist natively on Solana, so the reserve is always a wrapped
representation and **the vault is only as good as that bridge**. This is a risk-transfer
decision, not an engineering one, and it deserves a deliberate answer rather than
whichever asset has the deepest Jupiter liquidity:

| Asset | Bridge / issuer | The thing to weigh |
|---|---|---|
| wBTC / wETH | Wormhole (Portal) | Deepest liquidity on Solana; bridge is the trust root |
| tBTC | Threshold | Decentralised custody; thinner liquidity |
| cbBTC | Coinbase | Centralised issuer, strong balance sheet, issuer freeze risk |

Whichever you pick, name it in the vault's public description. "Backed by BTC" and
"backed by a Wormhole-wrapped claim on BTC" are different statements, and only the
second one is true.
