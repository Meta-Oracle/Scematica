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

```powershell
solana --version          # 1.18.26 — matches the program's solana-program pin
solana config get         # confirm the cluster you think you're on
```

The `anchor` CLI is **not** required and is not installed here. `anchor build` is
`cargo build-sbf` plus IDL generation, and nothing in this repo consumes the IDL —
`web/lib/escrow/program.ts` decodes the account bytes directly. Skipping Anchor also
skips a version mismatch: `Anchor.toml` and `Cargo.toml` must agree, and they did not.

On Windows, install the toolchain with the Solana release installer:

```powershell
curl.exe -sSL -o solana-install-init.exe `
  https://github.com/solana-labs/solana/releases/download/v1.18.26/solana-install-init-x86_64-pc-windows-msvc.exe
./solana-install-init.exe v1.18.26
[Environment]::SetEnvironmentVariable('HOME', $env:USERPROFILE, 'User')   # required
```

That last line is not optional: `cargo-build-sbf` aborts with `Can't get home directory
path` because Windows sets `USERPROFILE` rather than `HOME`.

---

## 1. Build

```powershell
powershell -ExecutionPolicy Bypass -File tools/build-programs.ps1 -Programs scemadex-vault
```

Use that script rather than calling `cargo-build-sbf` yourself. It checks one thing the
raw tool will not: **`cargo-build-sbf` reports an SBF stack-frame overflow and then exits
0**, emitting a `.so` that deploys cleanly and dies at runtime. This program hit exactly
that — `InitializeVault::try_accounts` came in at 5072 bytes against the hard 4096-byte
limit, meaning no vault could ever have been created. It is now under the limit via two
changes that must not be casually reverted:

- every account in `InitializeVault` is `Box`ed (heap, not stack), and
- `opt-level = 2` is pinned in `Cargo.toml` — the *only* value that fits. See the
  measured table in that file; both 3 and 1 overflow, and `"z"` + LTO is worst of all.

The script prints the sha256 — that is what a third party reproduces to prove the
deployed bytecode matches this source.

## 2. Program ID

Already assigned: **`A7h6khtKFJEu46By7C4hREdMQKkgvnuBCbVyusZRu4YW`**, declared in
`src/lib.rs` and registered in `Anchor.toml`. It replaced `Fg6PaFpo…`, the stock Anchor
example key, which on mainnet is an occupied non-program account.

The keypair is at `target/deploy/scemadex_vault-keypair.json`, gitignored via
`programs/*/target/`. **Back it up.** Until step 5 it is the upgrade authority; lose it
before then and the program can never be fixed, publish it and anyone can rewrite it.

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

```powershell
solana config set --url <your-rpc>
powershell -ExecutionPolicy Bypass -File tools/deploy-programs.ps1 -Programs scemadex-vault
```

which runs, with `--max-len` set to the exact `.so` length:

```bash
solana program deploy target/deploy/scemadex_vault.so \
  --program-id target/deploy/scemadex_vault-keypair.json \
  --max-len <exact .so byte length>
```

**Why exact rather than the default.** `solana program deploy` normally allocates *twice*
the binary size so a future upgrade can grow, and you pay rent on all of it — 4.67 SOL
here instead of 2.34. Exact sizing halves that at the cost of capping every future
upgrade at the current size or smaller. For this program that cost is zero, because
step 5 removes the ability to upgrade at all. Do not copy this flag to a program you
intend to keep upgrading.

> The devnet pass in §3 is **not** optional, and skipping it is a decision with a
> specific consequence: the negative tests are the ones that assert the guarantee, and
> the two defects found while first building this program (a stack overflow that made
> `initialize_vault` unusable, and a sibling program that had never compiled) are both
> the kind a devnet lifecycle run catches in minutes. If you deployed straight to
> mainnet, **do not run step 5 until you have exercised the table in §3 against the live
> program.** Everything is still fixable while you hold the upgrade authority.

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
   `tools/build-programs.ps1 -Programs scemadex-vault` from this commit prints a
   sha256; compare it against the deployed program. Reproducing it requires the same
   inputs — solana-cli **1.18.26** (platform-tools v1.41), the committed `Cargo.lock`,
   and `opt-level = 2`. A different optimisation level produces a different binary, and
   in this program's case an unusable one. A verified build (e.g. `solana-verify`) is
   the stronger form.

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
