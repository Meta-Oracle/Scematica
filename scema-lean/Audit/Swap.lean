/-
  `programs/scematica-swap` — the profit-or-revert guard around an arbitrage bundle.

  Unlike the escrow and the vault, this program guards **its own caller's** funds: only
  the authority that opened the swap state can invoke `profit_or_revert`, and the only
  thing a failure does is revert that authority's transaction. So nothing here is an
  attack surface against third parties — the audit question is narrower and still worth
  answering: *does passing the guard actually mean the arbitrage made money?*

  It does, for the account the guard was handed. What the guard does not do is check that
  that account is the one `start_swap` measured: `SwapState` records the mint but not the
  token account, and the state PDA is created with `init_if_needed` and never closed, so a
  baseline can also outlive the transaction that set it. Both are modelled below.
-/

namespace Audit.Swap

/-- An SPL token account, reduced to what the program reads. -/
structure Acct where
  key : Nat
  mint : Nat
  amount : Nat
deriving DecidableEq, Repr

/-- The `SwapState` PDA. `swap_input` holds the *balance at the start*, despite the name;
`expected_output` holds the minimum final balance. -/
structure SwapState where
  authority : Nat
  srcMint : Nat
  baseline : Nat
  minFinal : Nat
deriving DecidableEq, Repr

/-- `start_swap`. `min_final_balance = initial.saturating_sub(input).saturating_add(min_out)`,
and truncated `Nat` subtraction is exactly `saturating_sub`. -/
def startSwap (authority : Nat) (src : Acct) (swapInput minExpectedOutput : Nat) : SwapState :=
  { authority := authority, srcMint := src.mint, baseline := src.amount,
    minFinal := src.amount - swapInput + minExpectedOutput }

/-- `profit_or_revert`. `true` means the instruction returned `Ok`. -/
def profitOrRevert (st : SwapState) (src : Acct) : Bool :=
  src.mint = st.srcMint && st.baseline < src.amount && st.minFinal ≤ src.amount

/-- **The guard is sound for the account it is given**: passing it means that account
strictly gained, and cleared the slippage floor. -/
theorem profit_or_revert_sound (st : SwapState) (src : Acct) (h : profitOrRevert st src = true) :
    st.baseline < src.amount ∧ st.minFinal ≤ src.amount ∧ src.mint = st.srcMint := by
  simp only [profitOrRevert, Bool.and_eq_true, decide_eq_true_eq] at h
  exact ⟨h.1.2, h.2, h.1.1⟩

/-- Round trip: hand `profit_or_revert` the same account `start_swap` measured, and it
passes exactly when the balance rose and cleared the floor. -/
theorem guard_characterisation (authority : Nat) (src src' : Acct) (swapInput minOut : Nat)
    (hmint : src'.mint = src.mint) :
    profitOrRevert (startSwap authority src swapInput minOut) src' = true ↔
      (src.amount < src'.amount ∧ src.amount - swapInput + minOut ≤ src'.amount) := by
  simp [profitOrRevert, startSwap, hmint]

/-- The floor is the honest one whenever the position is actually funded: if the trade
input does not exceed the starting balance, the required final balance is
`start − input + min_out` on the nose. -/
theorem min_final_exact (authority : Nat) (src : Acct) (swapInput minOut : Nat)
    (h : swapInput ≤ src.amount) :
    (startSwap authority src swapInput minOut).minFinal + swapInput = src.amount + minOut := by
  simp only [startSwap]
  omega

/-- **Finding W-01 (informational).** The guard cannot tell two accounts of the same mint
apart: it reads `src.amount` and `src.mint`, and `SwapState` never recorded which account
was measured. So a client that assembles the bundle with the wrong `src` gets a green
light from an account that never took part in the trade. -/
theorem finding_W01_guard_ignores_account_identity (st : SwapState) (a b : Acct)
    (hmint : a.mint = b.mint) (hamt : a.amount = b.amount) :
    profitOrRevert st a = profitOrRevert st b := by
  simp only [profitOrRevert, hmint, hamt]

/-- Concretely: the traded account can be down on the day while an untouched account of
the same mint passes the check. -/
theorem finding_W01_substitution_passes :
    let st := startSwap 1 { key := 10, mint := 5, amount := 1000 } 1000 100
    let traded : Acct := { key := 10, mint := 5, amount := 900 }
    let bystander : Acct := { key := 11, mint := 5, amount := 5000 }
    profitOrRevert st traded = false ∧ profitOrRevert st bystander = true := by
  decide

/-- **Finding W-02 (informational).** The state PDA is opened with `init_if_needed` and
never closed, and `profit_or_revert` does not require that `start_swap` ran in the same
transaction. A stale baseline therefore keeps satisfying the guard: whether the check is
meaningful is a property of how the client orders its instructions, not of the program. -/
theorem finding_W02_stale_baseline_still_passes (st : SwapState) (src : Acct)
    (h : profitOrRevert st src = true) (later : Acct)
    (hmint : later.mint = src.mint) (hgrew : src.amount ≤ later.amount) :
    profitOrRevert st later = true := by
  simp only [profitOrRevert, Bool.and_eq_true, decide_eq_true_eq] at h ⊢
  exact ⟨⟨by rw [hmint]; exact h.1.1, by omega⟩, by omega⟩

end Audit.Swap
