# Scematica — security audit

**Date:** 2026-09-05  **Revision audited:** working tree at `3cc54c0`
**Companion artefact:** the `Audit` Lean library (`scema-lean/Audit.lean`,
`scema-lean/Audit/`), which builds with `cd scema-lean ; lake build` and contains a
machine-checked model of everything asserted below.

**Remediation status:** every finding except EF-01 (a scope statement, not a defect) has been
fixed in the working tree since this report was written. Section 6 lists what changed and
where. The findings below are stated in the present tense as they were audited; read them
as a description of the code *before* those commits.

---

## 1. Scope and method

The repository is large (≈64 kLOC of Rust in `crates/`, ≈29 kLOC in `scematica-omni/`, three
Anchor programs, a web front end and a great deal of prose). An audit that spreads itself
evenly over that is a reading exercise, not an audit. This one is concentrated on the code
where a defect costs money, keys, or control of a running system:

| Reviewed in depth | Why |
|---|---|
| `programs/scematica-escrow` | holds a bond in a program-owned vault and disburses it |
| `programs/scematica-vault` | holds third-party reserves under a time lock |
| `programs/scematica-swap` | guards the arbitrage bundle |
| `crates/scematica-protocol` | takes payment (x402 / SVM `exact`) |
| `crates/scemadex-sdk/src/settlement.rs` | the off-chain twin of the escrow lifecycle |
| `crates/scematica-api` | HTTP control surface of a live trading bot |
| `scematica-omni/crates/scema-daemon/src/auth.rs` | bearer token, loopback, DNS-rebinding |
| `scematica-omni/crates/scema-entitlement` | token-gated record distribution |
| `scematica-omni/crates/scema-effect` + `scema-cli execute` | the agent's only write path |
| `scematica-omni/crates/scema-anchor` | Merkle batching of sealed records |

Reviewed more briefly, and not the subject of any finding below: the sniper, the arbitrage
engine, the DQ\* agent, the TUI dashboards, the mesh crates, the web front end, and the
`alchem-link` Python package. **Nothing here should be read as a clean bill of health for
those.**

Method: manual review of the security-relevant paths, cross-checked against the design
intent stated in the code's own documentation, followed by formalization of each conclusion
as a Lean theorem. Where a finding is a behaviour of the code, the Lean statement *exhibits*
that behaviour; where it is a guarantee, the Lean statement proves it. No dynamic testing,
no fuzzing, no on-chain deployment, and no review of the deployed bytecode was performed.

### Reading intent correctly

Several patterns in this codebase look like defects to a reviewer skimming for them and are
not. They were checked and found to be deliberate, coherent, and — in most cases — better
than the alternative a checklist would recommend. They are listed here so that this audit
cannot be mistaken for one that flagged them.

| Pattern | Why it is not a finding |
|---|---|
| `scema-vault` (the service) serves over plain HTTP | Documented: TLS terminates at a reverse proxy; adding a TLS stack is precisely the dependency the workspace split avoids. |
| `scematica-vault` (the program) has no admin, no pause, no sweeper, no redemption | This is the product. Every one of those would reintroduce the trusted party the program exists to remove; donated tokens being permanently stuck is the accepted, documented cost. Proved: `Audit.Vault.excess_never_decreases`. |
| Arbitrage runs "program-less" by default | Deliberate: atomic min-out profit-or-revert without an on-chain deploy. The swap program is opt-in via `SWAP_PROGRAM_ID`. |
| `execute` is a dry run by default and needs two flags | Deliberate and correct; the dry run still runs the gates but never prompts. Proved: `Audit.Effect.dry_run_never_attempts`, `dry_run_never_prompts`, `cli_defaults_never_attempt`. |
| `scema-effect` shells out via `std::process::Command` | No shell is used — argv is passed directly — and nothing reaches the runner without confinement plus a trust decision. Proved: `Audit.Effect.attempted_implies_both_gates`. |
| The omni daemon binds loopback with no `--bind` flag, emits no CORS and handles no `OPTIONS` | Deliberate layering, documented in `scema-daemon`: loopback, bearer token, `Host` check, no CORS. Fails closed on a missing `Host`: `Audit.Access.absent_host_is_rejected`. |
| `.scema/omnid.token` exists in the working tree | It is a locally generated secret, `chmod 0600`, and excluded from version control by a `.scema/.gitignore` the daemon writes itself. Confirmed not tracked (`git ls-files`). |
| The Merkle tree promotes an odd node instead of duplicating it | This is the CVE-2012-2459 defence, and it is already proved in the pre-existing `scema-lean` package (`Scema.Merkle`). |
| Unmeasured quantities render as `—` rather than `0.00` | Deliberate and load-bearing; already proved in `Scema.Term`. |
| The zk-SNARK backend needs a per-circuit trusted setup | Stated as a caveat in the module's own header, with the transparent backend kept as the default. Honest, not hidden. |
| Deposits credit the measured balance delta rather than the requested amount | Required for Token-2022 fee-bearing mints, and load-bearing for solvency: `Audit.Vault.reachable_solvent`. |
| Two token-program accounts per vault instead of one | Needed for mixed legacy-SPL / Token-2022 pairs; a wrong program cannot create a bad vault because `initialize_account3` rejects it. |
| Every account in `InitializeVault` is `Box`ed | A correctness requirement against the 4 KiB SBF stack, not style. |
| The swap program's ID was rotated after a keypair reached git history | Correct response, documented at the `declare_id!`. |

---

## 2. Findings

Severity is the impact **if the component is deployed and used as intended**. The three
Anchor programs are, per their own `DEPLOY.md`, not yet deployed — which is what makes
E-01 and E-02 fixable at no cost, and what makes fixing them before any mainnet deploy
non-negotiable.

| ID | Severity | Component | Summary |
|---|---|---|---|
| [E-01](#e-01) | **Critical** | `scematica-escrow` | `settle` is permissionless and its payout destinations are bound only by mint — anyone can redirect a finalized bond |
| [E-02](#e-02) | **High** | `scematica-escrow` | `finalize_timeout` discards the provisional outcome, so a dishonored fill finalizes honored and can never be slashed |
| [X-01](#x-01) | **High** | `scematica-protocol` | Payment verification never checks the Ed25519 signature — only that the slot is non-zero |
| [X-02](#x-02) | **High** | `scematica-protocol` | No replay protection, and the resource is served before settlement is attempted |
| [X-03](#x-03) | **High** | `scematica-protocol` | Settlement rewrites the blockhash the client signed over, so a verified payment can never be collected |
| [A-01](#a-01) | **High** | `scematica-api` | Control routes bind all interfaces and are unauthenticated unless an env var happens to be set |
| [E-03](#e-03) | Medium | `scematica-escrow` | Two states have no timeout, so a vanished authority strands the bond permanently |
| [E-04](#e-04) | Low | `scematica-escrow` | The escrow PDA is seeded by digest alone and opened permissionlessly with caller-supplied roles |
| [S-04](#s-04) | Low | `scemadex-sdk` | `SlashRouting::distribute` is unvalidated off-chain, unlike its hardened on-chain twin |
| [A-02](#a-02) | Info | `scematica-api` | The API token is compared with a short-circuiting `==` |
| [W-01](#w-01) | Info | `scematica-swap` | The profit guard identifies the source account only by mint |
| [W-02](#w-02) | Info | `scematica-swap` | The swap-state PDA outlives the transaction that set the baseline |
| [EF-01](#ef-01) | Info | `scema-effect` | For a `run` effect, confinement covers the working directory, not what the command writes |
| [E-05](#e-05) | Info | `scematica-escrow` | `BondEscrow::LEN` over-allocates by one byte and its comment miscounts the fields |

---

<a id="e-01"></a>
### E-01 — Critical — anyone can take a finalized bond

**Where:** `programs/scematica-escrow/src/lib.rs`, `struct Settle` and `pub fn settle`.

`settle` requires only that the bond is `Finalized`; there is no signer constraint, which
is deliberate and fine — finalization is a public fact and disbursement should not need a
privileged caller. What is not fine is the account validation. Each payout destination
carries exactly one constraint:

```rust
#[account(mut, constraint = agent_token.mint == escrow.mint @ EscrowError::MintMismatch)]
pub agent_token: Account<'info, TokenAccount>,
```

Nothing binds `agent_token` to `escrow.agent`, `caller_token` to `escrow.caller`,
`challenger_token` to `escrow.challenger`, or the insurance and lineage accounts to anything
at all. `has_one = agent` constrains only the `AccountInfo` that receives the closed
account's rent, not the token account that receives the money.

So on the honored path — where the whole `bond + challenger_stake` goes to `agent_token` —
any observer can call `settle` on a finalized bond, passing a token account they own of the
same mint, and take the entire bond. The slashed path is the same story for all four slices.

The sibling program already does this correctly: `scematica-vault`'s `Withdraw` constrains
`depositor_token.owner == depositor.key()`. The escrow simply omits the equivalent.

**Formalized:** `Audit.Escrow.finding_E01_any_account_of_the_mint_is_accepted` exhibits an
accepted destination with an arbitrary owner; `Audit.Escrow.fixed_binds_the_owner` shows the
vault-style constraint rejects it.

**Remediation.** Add an owner constraint to every destination, e.g.

```rust
#[account(mut,
  constraint = agent_token.mint == escrow.mint @ EscrowError::MintMismatch,
  constraint = agent_token.owner == escrow.agent @ EscrowError::Unauthorized)]
pub agent_token: Account<'info, TokenAccount>,
```

and equivalently for `caller_token` (`escrow.caller`) and `challenger_token`
(`escrow.challenger`). The insurance and lineage destinations have no counterpart on the
record today — add `insurance` and `lineage` `Pubkey` fields to `BondEscrow`, set them at
`escrow`, and bind them the same way. Requiring the associated token account of each
recorded party is the tightest version and removes the ambiguity entirely.

---

<a id="e-02"></a>
### E-02 — High — a dishonored fill finalizes as honored

**Where:** `programs/scematica-escrow/src/lib.rs`, `finalize_timeout`, the `Provisional` arm.

```rust
s if s == BondState::Provisional as u8 => {
    require!(now >= e.window_closes_unix, EscrowError::WindowOpen);
    e.final_slashed = false; // optimistic honor
}
```

`provisional_honored` is never read. The authority can call `mark_provisional(false)` —
recording that the fill was bad — and the bond still finalizes honored once the window
elapses. Nor can anyone intervene: `file_challenge` rejects a provisional dishonor with
`NothingToChallenge`, by design, since there is nothing to dispute.

The consequence is an inversion of incentives. An agent that delivers a bad fill and gets it
marked dishonored keeps its bond; an agent that delivers nothing at all is slashed by the
deadline arm. Doing something bad is strictly better than doing nothing.

That this is a slip rather than an intention is visible in the code's own twin: the
off-chain `SettlementMachine::finalize` carries the provisional `outcome` into `Finalized`.

**Formalized:** `Audit.Escrow.finding_E02_dishonored_provisional_finalizes_honored` and
`finding_E02_dishonor_cannot_be_challenged`. The fix is modelled as
`Audit.Escrow.finalizeTimeoutFixed`, with `fixed_finalize_preserves_provisional` (a
dishonor now slashes) and `fixed_finalize_agrees_on_honored` (the optimistic path is
unchanged).

**Remediation.** `e.final_slashed = !e.provisional_honored;`

---

<a id="x-01"></a>
### X-01 — High — payment verification does not verify a signature

**Where:** `crates/scematica-protocol/src/scheme/svm_exact.rs`, step 4 of `verify_inner`.

```rust
if tx.signatures[idx] == solana_sdk::signature::Signature::default() {
    bail!("Transaction is not signed by the payer authority");
}
```

That is the entire authentication step: the signature slot must not be all zeros. No
`tx.verify()`, no `verify_with_results()`, no per-signature Ed25519 check. An attacker
constructs a transaction whose TransferChecked names any wealthy stranger as authority,
fills the signature slot with 64 arbitrary non-zero bytes, and `verify` returns
`is_valid: true` with `payer: <the stranger>`.

The economic checks around it — mint, destination ATA, exact amount, instruction count — are
correct and well tested. It is only the "did anyone actually authorise this" step that is
missing, and it is the one the other four depend on for meaning.

**Formalized:** `Audit.X402.finding_X01_unsigned_payload_verifies`, with
`Audit.X402.strict_verify_is_authorised` for the repaired check.

**Remediation.** Verify the signature over the message, not its presence:
`tx.verify().is_ok()`, or `verify_with_results()` indexed at the payer's slot when a partial
signature set is expected. See X-03 for what the message must be.

---

<a id="x-02"></a>
### X-02 — High — served before settled, and replayable without limit

**Where:** `crates/scematica-protocol/src/middleware.rs`.

The gate runs `verify`, serves the resource, then `tokio::spawn`s settlement and logs a
warning if it fails. Two consequences:

1. **Nothing links a payload to a request.** There is no store of settled or in-flight
   payments, so one valid `X-Payment` header — captured from a proxy log, a shared client,
   or the operator's own retry — buys unlimited requests.
2. **Failure to settle costs the attacker nothing.** The resource has already been
   delivered; the settlement error is discarded in a detached task.

**Formalized:** `Audit.X402.finding_X02_replay_is_unbounded`,
`finding_X02_second_use_still_accepted`, versus `nonced_gate_rejects_replay` and
`nonced_gate_admits_fresh` for a gate that records what it has served.

**Remediation.** Keep a store keyed by transaction signature (or by a payment nonce) with a
TTL at least as long as `max_timeout_seconds`, reject any payload already present, and
settle **before** serving for anything whose delivery cannot be undone. Where latency
forbids that, cap the exposure: settle-then-serve for expensive routes, and treat the
async path as a best-effort optimisation for cheap ones only.

---

<a id="x-03"></a>
### X-03 — High — settlement invalidates the very signature it was going to submit

**Where:** `crates/scematica-protocol/src/client.rs` and `facilitator.rs::submit`.

The client signs over `Hash::default()`:

```rust
let mut tx = Transaction::new_unsigned(message);
tx.partial_sign(&[payer], solana_sdk::hash::Hash::default());
```

and the facilitator then rewrites the blockhash before submitting:

```rust
tx.message.recent_blockhash = recent_blockhash;
tx.partial_sign(&[self.fee_payer.as_ref()], recent_blockhash);
```

A Solana signature commits to the serialized message, blockhash included. Rewriting the
blockhash after the payer signed leaves a signature over a message that no longer exists, so
the network rejects the transaction. The bundled happy path therefore cannot collect a
payment at all — and because of X-01 the verifier never noticed, and because of X-02 the
resource was already served.

A second defect sits in the same three lines: `Message::new(&[..], None)` makes the *payer*
the fee payer, so `self.fee_payer` is not among the message's signer accounts. In current
`solana-sdk`, `Transaction::partial_sign` panics on that mismatch rather than returning an
error; inside a `tokio::spawn`ed task the panic is swallowed, which is why the failure
presents as "payments silently never arrive".

**Formalized:** `Audit.X402.finding_X03_rebinding_breaks_the_payer_signature`, together with
`msgHash_injective_in_blockhash`.

**Remediation.** The client must sign the message that will actually be submitted. Either
(a) the resource server supplies a recent blockhash in the 402 response and the client signs
over it, with the facilitator submitting unmodified; or (b) the client uses a durable nonce
account, so the message is stable until consumed. In both cases build the message with an
explicit fee payer (`Message::new(&ixs, Some(&facilitator_pubkey))`) so `partial_sign` has a
slot to fill, and make the payer's signature verification (X-01) part of `verify`.

---

<a id="a-01"></a>
### A-01 — High — the bot's control API is reachable and unauthenticated by default

**Where:** `crates/scematica-api/src/main.rs`, `require_token`, the `CorsLayer`, and the
bind address.

```rust
let addr = SocketAddr::from(([0, 0, 0, 0], port));
...
_ => Ok(next.run(req).await), // no token configured → open (local dev)
```

The control routes are not read-only: `/api/controls/dump-mode` force-sells, `sell-mode`
pauses buys, `params` retunes the strategy. The gate is fail-open — an operator who never
sets `SCEMATICA_API_TOKEN` gets an unauthenticated control plane — and the listener binds
every interface, so "unauthenticated" means anyone who can route to the host. The comment
describes the intent as a local-dev default, but the socket is not local.

`CorsLayer::new().allow_origin(Any).allow_headers(Any)` compounds it: any web page the
operator visits can issue these POSTs against `127.0.0.1` and read the replies. The omni
daemon in the same repository gets this exactly right — loopback, a generated token, a
`Host` check, no CORS — so the fix already exists in-tree.

**Formalized:** `Audit.Access.finding_A01_unconfigured_token_authorises_anyone`, with
`fail_closed_rejects_the_unconfigured_case` and `fail_closed_agrees_when_configured` showing
the fix changes nothing for a configured deployment.

**Remediation.** Fail closed: refuse control routes when no token is configured, or better,
generate one on first run into a `0600` file the way `scema-daemon::auth::load_or_create`
does. Default the bind to `127.0.0.1` and require an explicit flag for anything wider.
Restrict CORS to the origins that actually need it — the mobile app does not need
`allow_origin(Any)` — and add a `Host` check on the control routes.

---

<a id="e-03"></a>
### E-03 — Medium — bonds can be stranded by an absent authority

**Where:** `finalize_timeout` (both arms) and the absence of any timeout from `Disputed`.

* A `Disputed` bond can only leave that state through `resolve`, which is `authority`-only.
  If the authority never answers, the bond, the stake and the vault rent are locked forever.
* An `Escrowed` bond with `deadline_unix == 0` — the "no deadline" sentinel the code accepts
  — can never be finalized by timeout either.

Both are liveness failures rather than theft, but a bond nobody can settle is
indistinguishable from a bond that was taken.

**Formalized:** `Audit.Escrow.disputed_does_not_time_out`.

**Remediation.** Add a dispute-resolution deadline (`window_closes_unix + resolve_grace`)
after which `finalize_timeout` may settle a `Disputed` bond on a stated default — slashing
is the conservative choice, since an unanswered dispute should not reward the agent — and
either reject `deadline_unix == 0` at `escrow` or give it a bounded default.

---

<a id="e-04"></a>
### E-04 — Low — permissionless escrow creation with caller-supplied roles

**Where:** `EscrowBond`, seeds `[b"escrow", digest]`, and the `authority`/`caller` arguments.

The PDA depends on the intent digest alone, and `escrow` takes `authority` and `caller` as
plain arguments with no validation. Anyone who learns a digest before the agent submits can
open the escrow first with a 1-unit bond and an authority they control. The real agent's
`init` then fails (address in use), and any party reading "there is an escrow for this
digest" without also reading `agent`, `authority`, `bond_amount` and `routing` is looking at
an attacker's record.

**Remediation.** Include the agent in the seeds (`[b"escrow", digest, agent.key()]`) so
records cannot collide across agents, or have the facilitator sign `escrow` so only intended
bonds are openable. Independently: every consumer must validate the record's fields, not
merely its existence.

---

<a id="s-04"></a>
### S-04 — Low — the off-chain split is not hardened the way the on-chain one is

**Where:** `crates/scemadex-sdk/src/settlement.rs`, `SlashRouting`.

`distribute` does not check `is_valid()`, `with_slash_routing` does not validate what it
stores, `total_bps` sums four `u32`s without checking for overflow, and `caller = amount -
challengers - insurance - lineage` is a plain subtraction that underflows when the shares
exceed the bond (panicking in debug, wrapping to an enormous caller share in release). The
on-chain twin gets all of this right: it widens to `u32`/`u128`, requires validity at
`escrow`, and uses `saturating_sub`.

**Formalized:** `Audit.Escrow.finding_S04_invalid_routing_overdraws` shows an invalid
routing demanding twice the bond; `Audit.Escrow.distribute_conserves` shows exact
conservation once validity is enforced.

**Remediation.** Validate in `with_slash_routing` (or make `distribute` return a `Result`),
sum in `u64`, and use `saturating_sub` for the dust-absorbing share.

---

<a id="a-02"></a>
### A-02 — Informational — the API token comparison short-circuits

`present_token(&headers).as_deref() == Some(expected.as_str())` returns at the first
differing byte. The code says a constant-time comparison is "overkill for a LAN pairing
secret", which is a defensible judgement — but the repository already contains
`scema_daemon::auth::secret_eq`, so the cost of doing it properly is one call.

**Formalized:** `Audit.Access.earlySteps_leaks_the_matching_prefix` (what leaks) and
`Audit.Access.steps_depends_only_on_lengths` (what the existing comparator guarantees).

---

<a id="w-01"></a>
### W-01 / <a id="w-02"></a>W-02 — Informational — the profit guard is weaker than it reads

`SwapState` records the mint but not the token account, and `profit_or_revert` re-reads
`src.amount` from whatever account it is handed; `init_if_needed` plus no `close` means a
baseline can also survive into a later transaction. Neither is exploitable by a third party
— only the recorded `authority` can invoke the check, and the only thing a failure does is
revert that authority's own transaction — but the guard proves less than "this arbitrage was
profitable".

**Formalized:** `Audit.Swap.profit_or_revert_sound` (what it does prove),
`Audit.Swap.finding_W01_guard_ignores_account_identity`,
`finding_W01_substitution_passes`, `finding_W02_stale_baseline_still_passes`.

**Remediation.** Store `src.key()` in `SwapState` and `require_keys_eq!` it in
`profit_or_revert`; close the state PDA in the same instruction so a baseline cannot outlive
its transaction.

---

<a id="ef-01"></a>
### EF-01 — Informational — `run` confines the working directory, not the command

`exec::run` confines `effect.path()`, which for `Effect::Run` is the cwd. The program name
and its arguments are not confined and cannot be: a command approved to run can write
anywhere the user can. The protection is the trust gate, which is exactly where it belongs —
this is recorded so that "the workspace confines effects" is not read as stronger than it
is. `Audit.Effect.attempted_implies_both_gates` states the guarantee at its true strength.

---

<a id="e-05"></a>
### E-05 — Informational — account size arithmetic

`BondEscrow::LEN = 8 + (32*6) + 32 + (8*3) + (8*3) + 8 + 3 + 2` allocates 293 bytes; the
fields need 292 (`state`, `bump`, and two `bool`s are four single bytes, not "3 u8 + 2
bool"). Over-allocation is harmless — the extra byte costs rent and nothing else — but the
comment should match the struct, since the next person to add a field will trust it.

---

## 3. What was verified and found sound

These are the positive results. Each is a theorem in the `Audit` library, so a future change
that breaks one breaks the build.

| Property | Where |
|---|---|
| A slashed bond's four-way split pays out exactly the bond, dust included | `Audit.Escrow.distribute_conserves` |
| `settle` moves exactly `bond + stake` on both branches, so the vault can be closed | `Audit.Escrow.settle_conserves` |
| No instruction but `settle` moves the principal | `Audit.Escrow.bond_amount_immutable` |
| A challenge can only be filed against a provisional honor | `Audit.Escrow.challenge_only_against_honored` |
| The backing vault is solvent along every reachable history, donations included | `Audit.Vault.reachable_solvent` |
| Every vault payment is to the recorded depositor, for the recorded amount, after maturity | `Audit.Vault.payment_requires_matured_position_and_depositor` |
| A lock may be strengthened, never weakened | `Audit.Vault.extend_only_increases` |
| Donated tokens can never be paid out — there is no sweeper and no admin | `Audit.Vault.excess_never_decreases` |
| Passing the profit guard means that account gained and cleared the floor | `Audit.Swap.profit_or_revert_sound` |
| The daemon's token comparison is correct and its cost is data-independent | `Audit.Access.secretEq_iff`, `steps_depends_only_on_lengths` |
| A missing or foreign `Host` is rejected | `Audit.Access.absent_host_is_rejected`, `foreign_host_is_rejected` |
| Entitlement is fail-closed; one token grants one world; "chain unreachable" is not a denial | `Audit.Access.grant_requires_everything`, `one_token_one_world`, `unknown_is_not_a_denial` |
| A dry run neither acts nor prompts | `Audit.Effect.dry_run_never_attempts`, `dry_run_never_prompts` |
| Anything carried out was committed, confined, and allowed or approved | `Audit.Effect.attempted_implies_both_gates` |
| An unconfined path is refused even under `--commit --yes` | `Audit.Effect.unconfined_is_refused` |
| The command-line defaults carry nothing out | `Audit.Effect.cli_defaults_never_attempt` |
| A policy refusal is not promptable | `Audit.Effect.policy_refusal_is_not_promptable` |
| Odd Merkle nodes are promoted, so two leaf sets cannot share a root | `Scema.Merkle` (pre-existing) |

Housekeeping checks that came back clean: a search of every `.rs` file under `crates/`,
`programs/`, `scematica-omni/`, `tools/`, `scema-botchain/` and `agent-playground/` finds no
`unsafe` at all; `git ls-files` lists no `.env`, keypair, `.pem` or token file; the daemon
token is generated locally, mode-restricted and self-ignored; the swap program's leaked
keypair was rotated and the old ID retired. These are pattern-based checks over the tracked
tree, not a guarantee that no secret has ever been committed — a full history scan with a
dedicated tool is worth running separately.

---

## 4. The Lean model

```console
$ cd scema-lean ; lake build      # discharges Scema and Audit both
$ cd scema-lean ; lake exe formalize
```

The model landed as a **second `lean_lib` inside the existing `scema-lean` package**, not as
a package of its own at the repository root. One toolchain, one `lake build`, one README —
and, decisively, the no-Mathlib rule already covering `scema-lean` now covers the audit model
too, which it can because every module under `Audit/` imports nothing at all. A root package
would have carried its own `lean-toolchain` (4.28.0 against `scema-lean`'s pinned 4.18.0) and
a Mathlib dependency, which is exactly the arrangement `scema-lean`'s README exists to refuse.

Adapting the model to 4.18.0 cost one three-line lemma: `Nat.div_le_div_right` does not exist
under that name there, so `Audit/Escrow.lean` proves the monotonicity it needed locally from
`Nat.le_div_iff_mul_le` and `Nat.div_mul_le_self`, both of which the file already used. The
proofs are otherwise unmodified, carry no `sorry`, and `#print axioms` reports only
`propext` and `Quot.sound` — no `sorryAx`, no `Classical.choice`.

| Module | What it models | Findings it carries |
|---|---|---|
| `Audit/Escrow.lean` | bond routing arithmetic, the four-state lifecycle, `settle`'s account set | E-01, E-02, E-03, S-04 |
| `Audit/Vault.lean` | vault accounting as a transition system over deposits, extensions, withdrawals and donations | — (all positive) |
| `Audit/Swap.lean` | the profit-or-revert guard | W-01, W-02 |
| `Audit/X402.lean` | transactions, signatures, `verify`, settlement, and the HTTP gate | X-01, X-02, X-03 |
| `Audit/Access.lean` | constant-time comparison, `Host` check, entitlement, the control-API gate | A-01, A-02 |
| `Audit/Effect.lean` | the two gates in front of the agent's write path | EF-01 (as a scope limit) |

The library has no dependencies — not even Mathlib — for the same reason `scema-lean` has
none: a proof nobody can afford to re-check is in the same category as a test nobody runs.
Every statement is about lists, options, integers, booleans and small inductive types.

**The gap, stated plainly.** This is a model of the Rust, not the Rust. Each definition was
written against the implementation and names the function it mirrors, but no tool checks
that correspondence, and it will drift if nobody maintains it. What the model does buy is
that each finding is now a precise claim rather than a paragraph, each fix has a stated
post-condition, and a regression in any of the positive results is a build failure.

---

## 5. Recommended order of work

1. **E-01** and **E-02** before any mainnet deploy of `scematica-escrow`. Both are
   small, local diffs.
2. **A-01** now — it affects anyone already running the API, and the fix is in-tree.
3. **X-01/X-02/X-03** together, as one redesign of the payment path: sign the submitted
   message, verify the signature, record what has been settled, settle before serving.
4. **E-03**, **E-04**, **S-04** before the escrow's first production use.
5. The informational items whenever the surrounding code is next touched.

---

## 6. Remediation — what changed

Applied after the audit, against this tree. Each entry names the file so the claim is
checkable; the Lean model already states each fix's post-condition, so a regression in one
of the positive results is a build failure rather than a re-read.

| ID | Status | Where |
|---|---|---|
| E-01 | Fixed | `programs/scematica-escrow/src/lib.rs` — every `Settle` destination is now bound to the owner the escrow recorded, not merely to the mint |
| E-02 | Fixed | same — `finalize_timeout`'s `Provisional` arm writes `!provisional_honored` instead of an unconditional honor |
| E-03 | Fixed | same — a `Disputed` bond finalizes slashed after `window_closes_unix + resolve_grace_secs`, and `escrow` now rejects `deadline_unix == 0` |
| E-04 | Fixed | same — the escrow and vault PDAs are seeded by `(digest, agent)`, and the caller, insurance and lineage destinations must be named at open time |
| E-05 | Fixed | same — `LEN` recounted against the fields actually stored |
| X-01 | Fixed | `crates/scematica-protocol/src/scheme/svm_exact.rs` — the payer's signature is verified over the serialized message, and a payer absent from the account keys or outside the signing region is rejected rather than falling through |
| X-02 | Fixed | `crates/scematica-protocol/src/middleware.rs` — a payment is claimed under its signature by test-and-set before settlement, and the resource is served only after settling; a claim is released only when the settler reports something that cannot have moved money |
| X-03 | Fixed | `crates/scematica-protocol/src/{facilitator,client}.rs` — the 402 carries `extra.feePayer` and `extra.recentBlockhash`, the payer signs the message that will be submitted, and `submit` no longer rewrites the blockhash underneath the signature |
| A-01 | Fixed | `crates/scematica-api/src/main.rs` — the bind address defaults to loopback, a missing or empty token is `503` rather than open, and CORS is layered under the gate so control routes carry no CORS headers |
| A-02 | Fixed | same — `secret_eq` folds over the full length in `usize`, so neither an early exit nor a narrowing cancellation is possible |
| W-01 | Fixed | `programs/scematica-swap/src/lib.rs` — `SwapState` records `src`, and `profit_or_revert` requires the account it was given |
| W-02 | Fixed | same — the state PDA closes to the authority in the same transaction that set the baseline |
| S-04 | Fixed | `crates/scemadex-sdk/src/settlement.rs` — `distribute` conserves by construction from a running remainder, `total_bps` is widened, and `try_with_slash_routing` refuses an invalid split rather than debug-asserting it |
| EF-01 | Unchanged, by design | A scope statement, not a defect: confinement covers the working directory a command is run in, not what the command writes. Documented in `scema-effect`; narrowing it would require sandboxing the child process, which is a different piece of work |

The x402 changes are behavioural, not just defensive: a 402 now costs the server a blockhash
fetch, and a paid request costs it a settlement confirmation before the resource is served.
That is the price of "paid" meaning paid — a route too cheap to want it should not be behind
a payment gate at all.
