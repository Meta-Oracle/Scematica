/-
  `programs/scematica-vault` — the time-locked backing vault, as an accounting machine.

  The program's whole product is a negative claim: *nobody, including its deployer, can
  take these funds*. The audit's job is to say precisely what that reduces to in the code,
  and this module states the reduction as theorems:

  * **Solvency.** The vault's SPL balance is never below the sum of the live positions,
    for every reachable sequence of deposits, withdrawals and third-party donations.
  * **No early exit, no third-party exit.** A payment happens only for a matured position
    and only to that position's recorded depositor.
  * **Donations are stuck.** The surplus of balance over recorded positions never
    decreases — which is the flip side of having no sweeper and no admin.

  The one modelling decision worth flagging: a deposit credits *what arrived*, not what was
  asked for (`backing_vault.reload()` and the balance delta), because a Token-2022 mint may
  charge a transfer fee. `Op.deposit` therefore carries both the requested and the received
  amounts, with `received ≤ requested`; the credited figure is the received one. That
  asymmetry is exactly what makes solvency hold for fee-bearing mints, so it has to be in
  the model rather than assumed away.
-/

namespace Audit.Vault

/-- One `Position` PDA. Pubkeys are `Nat`; `unlock` is the `i64` unix instant. -/
structure Position where
  depositor : Nat
  tokenAmt : Nat
  backingAmt : Nat
  unlock : Int
deriving DecidableEq, Repr

/-- The `Vault` account together with the two token accounts it is the authority for. -/
structure VaultState where
  /-- SPL balance of `token_vault`. -/
  balTok : Nat
  /-- SPL balance of `backing_vault`. -/
  balBack : Nat
  /-- `vault.total_token_locked`. -/
  totalTok : Nat
  /-- `vault.total_backing_locked`. -/
  totalBack : Nat
  positions : List Position
deriving Repr

def empty : VaultState :=
  { balTok := 0, balBack := 0, totalTok := 0, totalBack := 0, positions := [] }

def sumWith (f : Position → Nat) : List Position → Nat
  | [] => 0
  | p :: ps => f p + sumWith f ps

/-- Delete the first occurrence of a position — `close = depositor` on the PDA. -/
def removeOne (p : Position) : List Position → List Position
  | [] => []
  | q :: qs => if q = p then qs else q :: removeOne p qs

theorem sum_removeOne (f : Position → Nat) (p : Position) :
    ∀ l : List Position, p ∈ l → sumWith f (removeOne p l) + f p = sumWith f l
  | q :: qs, h => by
      by_cases hq : q = p
      · simp [removeOne, hq, sumWith, Nat.add_comm]
      · have hmem : p ∈ qs := by
          rcases List.mem_cons.mp h with h' | h'
          · exact absurd h'.symm hq
          · exact h'
        have := sum_removeOne f p qs hmem
        simp [removeOne, hq, sumWith, Nat.add_assoc, this]

/-- What a step moved out of the vault, and to whom. -/
structure Payment where
  to : Nat
  tokenAmt : Nat
  backingAmt : Nat
deriving DecidableEq, Repr

/-- The four things that can happen to a vault.

`donate` is not an instruction of the program — it is the fact that anyone may transfer
SPL tokens into any account. It is in the model because leaving it out would prove
solvency of a vault nobody can send tokens to. -/
inductive Op where
  /-- `deposit`: `reqTok`/`reqBack` asked for, `recvTok`/`recvBack` actually arrived. -/
  | deposit (depositor : Nat) (reqTok reqBack recvTok recvBack : Nat) (now lockSecs : Int)
  /-- `extend_lock`, signed by `signer`. -/
  | extend (p : Position) (signer : Nat) (newUnlock : Int)
  /-- `withdraw`, signed by `signer` at time `now`. -/
  | withdraw (p : Position) (signer : Nat) (now : Int)
  /-- A stranger transferring tokens in. -/
  | donate (tok back : Nat)
deriving Repr

/-- `MIN_LOCK_SECS` / `MAX_LOCK_SECS`. -/
def minLock : Int := 7 * 24 * 60 * 60
def maxLock : Int := 10 * 365 * 24 * 60 * 60

/-- One step. A failed `require!` reverts the transaction, which is modelled as leaving
the state untouched and paying nothing. -/
def step (v : VaultState) : Op → VaultState × Option Payment
  | .deposit depositor reqTok reqBack recvTok recvBack now lockSecs =>
      if 0 < reqBack ∧ recvTok ≤ reqTok ∧ recvBack ≤ reqBack ∧ 0 < recvBack ∧
         minLock ≤ lockSecs ∧ lockSecs ≤ maxLock then
        let p : Position :=
          { depositor := depositor, tokenAmt := recvTok, backingAmt := recvBack,
            unlock := now + lockSecs }
        ({ v with balTok := v.balTok + recvTok, balBack := v.balBack + recvBack,
                  totalTok := v.totalTok + recvTok, totalBack := v.totalBack + recvBack,
                  positions := p :: v.positions }, none)
      else (v, none)
  | .extend p signer newUnlock =>
      if p ∈ v.positions ∧ signer = p.depositor ∧ p.unlock < newUnlock then
        ({ v with positions := { p with unlock := newUnlock } :: removeOne p v.positions }, none)
      else (v, none)
  | .withdraw p signer now =>
      if p ∈ v.positions ∧ signer = p.depositor ∧ p.unlock ≤ now then
        ({ v with balTok := v.balTok - p.tokenAmt, balBack := v.balBack - p.backingAmt,
                  totalTok := v.totalTok - p.tokenAmt, totalBack := v.totalBack - p.backingAmt,
                  positions := removeOne p v.positions },
         some { to := p.depositor, tokenAmt := p.tokenAmt, backingAmt := p.backingAmt })
      else (v, none)
  | .donate tok back => ({ v with balTok := v.balTok + tok, balBack := v.balBack + back }, none)

def run (v : VaultState) : List Op → VaultState
  | [] => v
  | op :: ops => run (step v op).1 ops

/-! ## Solvency -/

/-- The published totals agree with the live positions, and the tokens are there. -/
def Solvent (v : VaultState) : Prop :=
  v.totalTok = sumWith Position.tokenAmt v.positions ∧
  v.totalBack = sumWith Position.backingAmt v.positions ∧
  v.totalTok ≤ v.balTok ∧ v.totalBack ≤ v.balBack

theorem empty_solvent : Solvent empty := by
  refine ⟨rfl, rfl, Nat.le_refl _, Nat.le_refl _⟩

/-- A position that is live is covered by the totals. -/
theorem le_sum_of_mem (f : Position → Nat) (p : Position) :
    ∀ l : List Position, p ∈ l → f p ≤ sumWith f l
  | q :: qs, h => by
      rcases List.mem_cons.mp h with h' | h'
      · subst h'; exact Nat.le_add_right _ _
      · exact Nat.le_trans (le_sum_of_mem f p qs h') (Nat.le_add_left _ _)

/-- **Every instruction preserves solvency**, including a stranger's donation. -/
theorem step_preserves_solvent (v : VaultState) (op : Op) (h : Solvent v) :
    Solvent (step v op).1 := by
  obtain ⟨h1, h2, h3, h4⟩ := h
  cases op with
  | deposit depositor reqTok reqBack recvTok recvBack now lockSecs =>
      simp only [step]
      split
      · exact ⟨by simp only [sumWith]; omega, by simp only [sumWith]; omega,
               Nat.add_le_add_right h3 _, Nat.add_le_add_right h4 _⟩
      · exact ⟨h1, h2, h3, h4⟩
  | extend p signer newUnlock =>
      simp only [step]
      split
      · rename_i hc
        have hTok := sum_removeOne Position.tokenAmt p v.positions hc.1
        have hBack := sum_removeOne Position.backingAmt p v.positions hc.1
        have e1 : ({ p with unlock := newUnlock } : Position).tokenAmt = p.tokenAmt := rfl
        have e2 : ({ p with unlock := newUnlock } : Position).backingAmt = p.backingAmt := rfl
        refine ⟨?_, ?_, h3, h4⟩ <;> dsimp only [sumWith] <;> omega
      · exact ⟨h1, h2, h3, h4⟩
  | withdraw p signer now =>
      simp only [step]
      split
      · rename_i hc
        have hTok := sum_removeOne Position.tokenAmt p v.positions hc.1
        have hBack := sum_removeOne Position.backingAmt p v.positions hc.1
        refine ⟨?_, ?_, ?_, ?_⟩ <;> dsimp only <;> omega
      · exact ⟨h1, h2, h3, h4⟩
  | donate tok back =>
      exact ⟨h1, h2, Nat.le_trans h3 (Nat.le_add_right _ _),
             Nat.le_trans h4 (Nat.le_add_right _ _)⟩

/-- **Solvency is a property of every reachable vault**, not just of one instruction. -/
theorem run_preserves_solvent : ∀ (ops : List Op) (v : VaultState), Solvent v → Solvent (run v ops)
  | [], _, h => h
  | op :: ops, v, h => run_preserves_solvent ops _ (step_preserves_solvent v op h)

/-- The headline: a vault that started empty is solvent forever. -/
theorem reachable_solvent (ops : List Op) : Solvent (run empty ops) :=
  run_preserves_solvent ops empty empty_solvent

/-! ## Who is paid, and when -/

/-- **No early exit and no third-party exit.** Any payment at all is to the position's
recorded depositor, is exactly the recorded amounts, and happens only at or after the
recorded unlock instant. There is no admin, no pause and no sweeper that could add
another way out — the model has every instruction the program exposes. -/
theorem payment_requires_matured_position_and_depositor
    (v : VaultState) (op : Op) (pay : Payment) (h : (step v op).2 = some pay) :
    ∃ p : Position, p ∈ v.positions ∧ pay.to = p.depositor ∧
      pay.tokenAmt = p.tokenAmt ∧ pay.backingAmt = p.backingAmt ∧
      ∃ now : Int, op = Op.withdraw p p.depositor now ∧ p.unlock ≤ now := by
  cases op with
  | deposit _ _ _ _ _ _ _ => simp only [step] at h; split at h <;> exact absurd h (by simp)
  | extend p signer newUnlock => simp only [step] at h; split at h <;> exact absurd h (by simp)
  | donate tok back => exact absurd h (by simp [step])
  | withdraw p signer now =>
      simp only [step] at h
      split at h
      · rename_i hc
        obtain ⟨hmem, hsig, hmat⟩ := hc
        cases h
        exact ⟨p, hmem, rfl, rfl, rfl, now, by rw [hsig], hmat⟩
      · exact absurd h (by simp)

/-- `extend_lock` is strictly increasing: a lock may be strengthened, never weakened. -/
theorem extend_only_increases (v : VaultState) (p : Position) (signer : Nat) (newUnlock : Int)
    (h : (step v (.extend p signer newUnlock)).1 ≠ v) : p.unlock < newUnlock := by
  by_cases hc : p ∈ v.positions ∧ signer = p.depositor ∧ p.unlock < newUnlock
  · exact hc.2.2
  · exact absurd (by simp [step, hc]) h

/-! ## Donations are stuck

The doc comment on `Vault::total_token_locked` says a stranger's donation is permanently
stuck, and that this is preferable to an admin who could sweep it. That is a claim about
every future execution, so it is worth stating as one.
-/

/-- Balance in excess of what the live positions account for. -/
def excessTok (v : VaultState) : Nat := v.balTok - v.totalTok
def excessBack (v : VaultState) : Nat := v.balBack - v.totalBack

/-- **The surplus never falls.** No instruction can pay out a donation: `withdraw` moves
the amounts recorded on a position and decrements the totals by the same figures, so the
gap between balance and totals is monotone. A sweeper — or any admin — would break this. -/
theorem excess_never_decreases (v : VaultState) (op : Op) (h : Solvent v) :
    excessTok v ≤ excessTok (step v op).1 ∧ excessBack v ≤ excessBack (step v op).1 := by
  obtain ⟨h1, h2, h3, h4⟩ := h
  cases op with
  | deposit depositor reqTok reqBack recvTok recvBack now lockSecs =>
      simp only [step]; split <;>
        refine ⟨?_, ?_⟩ <;> dsimp only [excessTok, excessBack] <;> omega
  | extend p signer newUnlock =>
      simp only [step]; split <;>
        refine ⟨?_, ?_⟩ <;> dsimp only [excessTok, excessBack] <;> omega
  | withdraw p signer now =>
      simp only [step]
      split
      · rename_i hc
        have hTok : p.tokenAmt ≤ v.totalTok := by
          rw [h1]; exact le_sum_of_mem Position.tokenAmt p v.positions hc.1
        have hBack : p.backingAmt ≤ v.totalBack := by
          rw [h2]; exact le_sum_of_mem Position.backingAmt p v.positions hc.1
        refine ⟨?_, ?_⟩ <;> dsimp only [excessTok, excessBack] <;> omega
      · exact ⟨Nat.le_refl _, Nat.le_refl _⟩
  | donate tok back =>
      refine ⟨?_, ?_⟩ <;> dsimp only [step, excessTok, excessBack] <;> omega

end Audit.Vault
