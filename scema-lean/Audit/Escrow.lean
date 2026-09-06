/-
  `programs/scematica-escrow` — the Conviction-Routing bond escrow, as a state machine.

  This module is part of the security audit of Scematica (see `SECURITY-AUDIT.md`). It
  models the parts of `programs/scematica-escrow/src/lib.rs` that decide **who gets paid,
  how much, and when**:

  * `SlashRouting::distribute` — the four-way split of a slashed bond, dust to the caller;
  * the `Escrowed → Provisional → Disputed → Finalized` lifecycle and its guards;
  * the account set `settle` accepts, which is where the payout destinations are (not)
    bound to the parties recorded in the escrow.

  Two of the theorems below are *negative*: they exhibit a behaviour of the program as
  written rather than a guarantee of it. They are marked `finding_` and each names the
  audit item it witnesses. Every other theorem is a property the implementation has and
  should keep.

  No `import`. Every statement here is about naturals, integers, booleans and a small
  inductive state, so it is checkable by `decide`, `omega` or a short term — the same
  constraint the pre-existing `scema-lean` package adopts, and for the same reason.
-/

namespace Audit.Escrow

/-! ## Basis-point routing

`u64`/`u128` arithmetic in the Rust becomes `Nat` here. This is faithful rather than
approximate on the paths that matter: `share` is computed in `u128` from `u64` inputs and
so cannot overflow, and `saturating_sub` on `u64` is exactly truncated `Nat` subtraction.
-/

/-- `SlashRouting`: four shares in basis points. -/
structure Routing where
  caller : Nat
  challengers : Nat
  insurance : Nat
  lineage : Nat
deriving DecidableEq, Repr

/-- `SlashRouting::is_valid` — the four shares must sum to a full 10 000 bps. -/
def Routing.valid (r : Routing) : Prop :=
  r.caller + r.challengers + r.insurance + r.lineage = 10000

instance (r : Routing) : Decidable r.valid := by
  unfold Routing.valid; infer_instance

/-- One share of `bond`, floor-divided. Mirrors `|bps| (bond as u128) * bps / 10_000`. -/
def share (bond bps : Nat) : Nat := bond * bps / 10000

/-- `SlashSplit`. -/
structure Split where
  caller : Nat
  challengers : Nat
  insurance : Nat
  lineage : Nat
deriving DecidableEq, Repr

def Split.total (s : Split) : Nat :=
  s.caller + s.challengers + s.insurance + s.lineage

/-- `SlashRouting::distribute`. The caller share is the remainder, so it absorbs the
rounding dust of the other three. Truncated `Nat` subtraction is `saturating_sub`. -/
def Routing.distribute (r : Routing) (bond : Nat) : Split :=
  let ch := share bond r.challengers
  let ins := share bond r.insurance
  let lin := share bond r.lineage
  { caller := bond - ch - ins - lin, challengers := ch, insurance := ins, lineage := lin }

/-- Floor division is superadditive in the numerator: `a/n + b/n ≤ (a+b)/n`. -/
private theorem div_add_div_le (a b n : Nat) (hn : 0 < n) : a / n + b / n ≤ (a + b) / n := by
  rw [Nat.le_div_iff_mul_le hn, Nat.add_mul]
  exact Nat.add_le_add (Nat.div_mul_le_self a n) (Nat.div_mul_le_self b n)

/-- Floor division is monotone in the numerator.

Proved here rather than taken from core: the name and shape of this lemma have moved
between Lean releases, and a package whose whole selling point is that it builds offline
in seconds on a pinned toolchain should not be one core rename away from failing. The
proof needs nothing the lemma above did not already need. -/
private theorem div_le_div_of_le (a b n : Nat) (hn : 0 < n) (h : a ≤ b) : a / n ≤ b / n := by
  rw [Nat.le_div_iff_mul_le hn]
  exact Nat.le_trans (Nat.div_mul_le_self a n) h

/-- The three non-dust shares never exceed the bond, as long as their bps do not exceed
10 000. This is what `require!(routing.is_valid())` at `escrow` time buys. -/
theorem three_shares_le_bond (r : Routing) (bond : Nat)
    (h : r.challengers + r.insurance + r.lineage ≤ 10000) :
    share bond r.challengers + share bond r.insurance + share bond r.lineage ≤ bond := by
  have h10 : 0 < 10000 := by omega
  have h1 : bond * r.challengers / 10000 + bond * r.insurance / 10000
      ≤ (bond * r.challengers + bond * r.insurance) / 10000 := div_add_div_le _ _ _ h10
  have h2 : (bond * r.challengers + bond * r.insurance) / 10000 + bond * r.lineage / 10000
      ≤ (bond * r.challengers + bond * r.insurance + bond * r.lineage) / 10000 :=
    div_add_div_le _ _ _ h10
  have hmul : bond * r.challengers + bond * r.insurance + bond * r.lineage
      = bond * (r.challengers + r.insurance + r.lineage) := by
    rw [Nat.mul_add, Nat.mul_add]
  have h3 : bond * (r.challengers + r.insurance + r.lineage) / 10000 ≤ bond * 10000 / 10000 :=
    div_le_div_of_le _ _ _ h10 (Nat.mul_le_mul_left bond h)
  have h4 : bond * 10000 / 10000 = bond := Nat.mul_div_cancel _ h10
  have h5 : bond * r.challengers / 10000 + bond * r.insurance / 10000
      + bond * r.lineage / 10000 ≤ bond := by
    refine Nat.le_trans (Nat.add_le_add_right h1 _) (Nat.le_trans h2 ?_)
    rw [hmul]
    exact Nat.le_trans h3 (Nat.le_of_eq h4)
  simpa [share] using h5

/-- **Value is conserved by a slash.** Under a valid routing the four disbursements sum to
exactly the bond: no dust is stranded in the vault (which would make `close_account` fail)
and no more than the bond is paid out. -/
theorem distribute_conserves (r : Routing) (h : r.valid) (bond : Nat) :
    (r.distribute bond).total = bond := by
  have hb : r.challengers + r.insurance + r.lineage ≤ 10000 := by
    unfold Routing.valid at h; omega
  have := three_shares_le_bond r bond hb
  simp only [Routing.distribute, Split.total]
  omega

/-- **The validity guard is load-bearing.** Drop `require!(routing.is_valid())` and the
split can demand more than the bond: with two 100 % shares the vault is asked for twice
what it holds. The on-chain program rejects such a routing at `escrow`; the off-chain
`scemadex_sdk::SlashRouting::distribute` does not check, which is audit item **S-04**. -/
theorem finding_S04_invalid_routing_overdraws :
    ({ caller := 0, challengers := 10000, insurance := 10000, lineage := 0 : Routing }.distribute
      1000).total = 2000 := by decide

/-! ## The bond lifecycle

`BondState` and the four state-changing instructions. Each instruction is modelled as
`Bond → Option Bond`: `none` is a failed `require!`, which on Solana reverts the whole
transaction and leaves the account untouched.
-/

inductive BondState where
  | escrowed | provisional | disputed | finalized
deriving DecidableEq, Repr

/-- The mutable part of the `BondEscrow` account. `Int` for timestamps, matching `i64`. -/
structure Bond where
  state : BondState
  provisionalHonored : Bool
  finalSlashed : Bool
  disputeWindowSecs : Int
  windowCloses : Int
  deadline : Int
  bondAmount : Nat
  challengerStake : Nat
deriving DecidableEq, Repr

/-- `mark_provisional`, gated to the recorded `authority` by `has_one`. -/
def markProvisional (now : Int) (honored : Bool) (b : Bond) : Option Bond :=
  if b.state = BondState.escrowed then
    some { b with state := .provisional, provisionalHonored := honored,
                  windowCloses := now + b.disputeWindowSecs }
  else none

/-- `file_challenge`. Only a provisionally *honored* bond can be challenged, only inside
the window, and only once (the second challenger arrives in `Disputed` and is rejected). -/
def fileChallenge (now : Int) (stake : Nat) (b : Bond) : Option Bond :=
  if b.state = BondState.provisional ∧ b.provisionalHonored ∧ now < b.windowCloses ∧ 0 < stake then
    some { b with state := .disputed, challengerStake := stake }
  else none

/-- `resolve`, gated to the recorded `authority`. -/
def resolve (challengerWon : Bool) (b : Bond) : Option Bond :=
  if b.state = BondState.disputed then
    some { b with state := .finalized, finalSlashed := challengerWon }
  else none

/-- `finalize_timeout` **as implemented**: permissionless, clock-only.

Note the `Provisional` arm sets `final_slashed = false` unconditionally — it does not read
`provisional_honored`. -/
def finalizeTimeout (now : Int) (b : Bond) : Option Bond :=
  match b.state with
  | .provisional =>
      if now ≥ b.windowCloses then some { b with state := .finalized, finalSlashed := false }
      else none
  | .escrowed =>
      if b.deadline ≠ 0 ∧ now ≥ b.deadline then
        some { b with state := .finalized, finalSlashed := true }
      else none
  | _ => none

/-- `finalize_timeout` **as it should be**: the unchallenged provisional outcome is what
becomes final, which is what the off-chain twin `SettlementMachine::finalize` already
does (it carries `outcome` into `Finalized`). -/
def finalizeTimeoutFixed (now : Int) (b : Bond) : Option Bond :=
  match b.state with
  | .provisional =>
      if now ≥ b.windowCloses then
        some { b with state := .finalized, finalSlashed := !b.provisionalHonored }
      else none
  | .escrowed =>
      if b.deadline ≠ 0 ∧ now ≥ b.deadline then
        some { b with state := .finalized, finalSlashed := true }
      else none
  | _ => none

/-- Every instruction that succeeds from `Provisional`/`Escrowed`/`Disputed` leaves the
bond amount alone: no instruction but `settle` moves the principal. -/
theorem bond_amount_immutable (now : Int) (b b' : Bond) :
    (markProvisional now true b = some b' ∨ markProvisional now false b = some b' ∨
     resolve true b = some b' ∨ resolve false b = some b' ∨
     finalizeTimeout now b = some b' ∨ finalizeTimeoutFixed now b = some b') →
    b'.bondAmount = b.bondAmount := by
  intro h
  rcases h with h | h | h | h | h | h <;>
    simp only [markProvisional, resolve, finalizeTimeout, finalizeTimeoutFixed] at h <;>
    split at h <;> (try split at h) <;> first | (cases h; rfl) | cases h

/-- A challenge can only ever be filed against a provisional *honor*. -/
theorem challenge_only_against_honored (now : Int) (stake : Nat) (b b' : Bond)
    (h : fileChallenge now stake b = some b') : b.provisionalHonored = true := by
  simp only [fileChallenge] at h
  split at h
  · rename_i hc; exact hc.2.1
  · exact absurd h (by simp)

/-- **Finding E-02.** A bond the authority marked provisionally *dishonored* finalizes as
**honored** once the window elapses: `finalize_timeout` writes `final_slashed = false`
without consulting `provisional_honored`. -/
theorem finding_E02_dishonored_provisional_finalizes_honored
    (now : Int) (b : Bond) (hs : b.state = BondState.provisional)
    (hh : b.provisionalHonored = false) (hw : now ≥ b.windowCloses) :
    ∃ b', finalizeTimeout now b = some b' ∧ b'.state = BondState.finalized ∧
      b'.finalSlashed = false ∧ b.provisionalHonored = false := by
  refine ⟨{ b with state := .finalized, finalSlashed := false }, ?_, rfl, rfl, hh⟩
  simp [finalizeTimeout, hs, hw]

/-- …and nobody can prevent that outcome, because the one instruction that could have
slashed the bond — `file_challenge` — refuses a provisional dishonor (`NothingToChallenge`).
So a fill the oracle judged bad is strictly *safer* for the agent than no fill at all,
which is the inversion this finding is about. -/
theorem finding_E02_dishonor_cannot_be_challenged (now : Int) (stake : Nat) (b : Bond)
    (hh : b.provisionalHonored = false) : fileChallenge now stake b = none := by
  simp [fileChallenge, hh]

/-- The fix behaves: an unchallenged provisional dishonor finalizes slashed. -/
theorem fixed_finalize_preserves_provisional (now : Int) (b : Bond)
    (hs : b.state = BondState.provisional) (hw : now ≥ b.windowCloses) :
    ∃ b', finalizeTimeoutFixed now b = some b' ∧
      b'.finalSlashed = !b.provisionalHonored := by
  refine ⟨{ b with state := .finalized, finalSlashed := !b.provisionalHonored }, ?_, rfl⟩
  simp [finalizeTimeoutFixed, hs, hw]

/-- And the fix does not disturb the honored path, which is the one the optimistic design
is built around: an unchallenged *honor* still finalizes honored. -/
theorem fixed_finalize_agrees_on_honored (now : Int) (b : Bond)
    (hh : b.provisionalHonored = true) :
    finalizeTimeoutFixed now b = finalizeTimeout now b := by
  simp only [finalizeTimeoutFixed, finalizeTimeout, hh]
  cases b.state <;> simp

/-- `settle` runs only from `Finalized`, and `Finalized` is reached only through `resolve`
or `finalize_timeout`. Neither is reachable while a dispute is open and unresolved, so a
disputed bond cannot be settled by waiting. -/
theorem disputed_does_not_time_out (now : Int) (b : Bond)
    (hs : b.state = BondState.disputed) : finalizeTimeout now b = none := by
  simp [finalizeTimeout, hs]

/-! ## Who gets paid

`settle` is permissionless: it checks only that the bond is `Finalized`. What it pays out
is fixed by the record; **where** it pays is taken from the accounts the caller supplies.
The model below is exactly that — a destination is a token account, and the program's
check on it is a predicate.
-/

/-- A token account: who owns it and which mint it holds. -/
structure TokenAccount where
  owner : Nat
  mint : Nat
deriving DecidableEq, Repr

/-- The constraint `settle` actually places on `agent_token`, `caller_token`,
`challenger_token`, `insurance_token` and `lineage_token`:

```rust
#[account(mut, constraint = agent_token.mint == escrow.mint @ EscrowError::MintMismatch)]
```

— the mint, and nothing else. -/
def acceptedByImpl (escrowMint : Nat) (dest : TokenAccount) : Bool :=
  dest.mint = escrowMint

/-- The constraint the sibling program `scematica-vault` places on the same kind of
destination (`depositor_token.owner == depositor.key()`), transplanted here. -/
def acceptedByFixed (escrowMint expectedOwner : Nat) (dest : TokenAccount) : Bool :=
  dest.mint = escrowMint && dest.owner = expectedOwner

/-- **Finding E-01.** `settle` is permissionless and its payout destinations are
constrained only by mint, so a stranger's token account of the right mint is accepted in
the slot that is supposed to be the agent's. Since the honored branch sends
`bond + challenger_stake` to `agent_token`, anyone may call `settle` on a finalized bond
and receive the whole thing. -/
theorem finding_E01_any_account_of_the_mint_is_accepted
    (escrowMint agent attacker : Nat) (h : attacker ≠ agent) :
    ∃ dest : TokenAccount,
      acceptedByImpl escrowMint dest = true ∧ dest.owner ≠ agent := by
  exact ⟨{ owner := attacker, mint := escrowMint }, by simp [acceptedByImpl], h⟩

/-- The same account is rejected once the destination is bound to the recorded party. -/
theorem fixed_binds_the_owner (escrowMint agent : Nat) (dest : TokenAccount)
    (h : acceptedByFixed escrowMint agent dest = true) : dest.owner = agent := by
  simp only [acceptedByFixed, Bool.and_eq_true, decide_eq_true_eq] at h
  exact h.2

/-! ## Conservation across `settle`

Whichever branch runs, the vault pays out exactly `bond + stake` — which is exactly what
was paid in (`escrow` moved `bond`, `file_challenge` moved `stake`). That is what lets the
final `close_account` succeed: a token account can only be closed at zero balance, so a
conservation bug here would surface as a stuck vault rather than a silent loss.
-/

/-- Total moved out of the vault by `settle`, as a function of the branch taken. -/
def settlePayout (r : Routing) (bond stake : Nat) (slashed : Bool) : Nat :=
  if slashed then (r.distribute bond).total + stake else bond + stake

theorem settle_conserves (r : Routing) (h : r.valid) (bond stake : Nat) (slashed : Bool) :
    settlePayout r bond stake slashed = bond + stake := by
  cases slashed <;> simp [settlePayout, distribute_conserves r h bond]

end Audit.Escrow
