/-
  `crates/scematica-protocol` — the x402 payment gate, modelled end to end.

  The crate has four moving parts and the audit's findings are all about how they compose:

  * `client::build_payment_payload` builds a TransferChecked transaction and signs it over
    the *default* blockhash;
  * `scheme::svm_exact::verify` decides whether a payload satisfies the requirements;
  * `facilitator::submit` replaces the blockhash and submits;
  * `middleware::payment_middleware` serves the resource on `verify` alone and settles in a
    detached task whose result is discarded.

  The model keeps a transaction abstract except for what those functions look at: a payment
  instruction, and a list of signatures. A signature carries *what it was made over* —
  that is the whole content of "a signature binds a message", and it is the field the
  implementation never consults.
-/

namespace Audit.X402

/-! ## Transactions and signatures -/

/-- The one instruction `verify` inspects. -/
structure Transfer where
  mint : Nat
  /-- Destination token account; `verify` requires the ATA of `(pay_to, asset)`. -/
  dest : Nat
  /-- The transfer authority — reported back as the payer. -/
  authority : Nat
  amount : Nat
deriving DecidableEq, Repr

/-- A signature slot. `zero` is Solana's all-zero placeholder; a real signature records
whose key made it and which message body it commits to. -/
structure Sig where
  zero : Bool
  signer : Nat
  /-- The message the signature actually authorises. -/
  over : Nat
deriving DecidableEq, Repr

structure Tx where
  transfer : Transfer
  /-- Instruction count; the SVM scheme requires 3–6. -/
  instrCount : Nat
  blockhash : Nat
  sigs : List Sig
deriving DecidableEq, Repr

/-- The message body a signature must commit to. Everything in the transaction except the
signatures — in particular the recent blockhash, which is what makes a Solana signature
expire and non-replayable. -/
def msgHash (tx : Tx) : Nat :=
  (tx.transfer.mint + 7 * tx.transfer.dest + 13 * tx.transfer.authority +
    17 * tx.transfer.amount + 19 * tx.instrCount) * 23 + tx.blockhash

/-- What the chain does: a transaction is accepted only if every required signature is a
real signature by the right key over *this* message. Modelled for the payer's slot. -/
def chainAccepts (tx : Tx) : Bool :=
  tx.sigs.any fun s =>
    !s.zero && s.signer = tx.transfer.authority && s.over = msgHash tx

/-- What the payment requirements demand. -/
structure Requirements where
  mint : Nat
  /-- The ATA derived from `pay_to`. -/
  dest : Nat
  amount : Nat
deriving DecidableEq, Repr

/-! ## `verify`, as written and as it should be -/

/-- The economic checks: right mint, right destination, exact amount, plausible shape.
These the implementation does get right, and the crate's tests pin all four. -/
def economicallyValid (r : Requirements) (tx : Tx) : Bool :=
  tx.transfer.mint = r.mint && tx.transfer.dest = r.dest && tx.transfer.amount = r.amount &&
    3 ≤ tx.instrCount && tx.instrCount ≤ 6

/-- `svm_exact::verify_inner` step 4, verbatim in behaviour: locate the payer's slot and
require only that it is **not the default (all-zero) signature**. Nothing checks who made
it or what it was made over. -/
def verifyImpl (r : Requirements) (tx : Tx) : Bool :=
  economicallyValid r tx && tx.sigs.any fun s => !s.zero

/-- The same check with the cryptography restored: the payer's signature must be a real
signature, by the payer, over this transaction. -/
def verifyStrict (r : Requirements) (tx : Tx) : Bool :=
  economicallyValid r tx && chainAccepts tx

/-- The forgery: a real-looking payment whose signature slot holds a non-zero constant
that commits to nothing. -/
def forgedTx (r : Requirements) (victim : Nat) : Tx :=
  { transfer := { mint := r.mint, dest := r.dest, authority := victim, amount := r.amount },
    instrCount := 3, blockhash := 0,
    sigs := [{ zero := false, signer := victim, over := 0 }] }

/-- **Finding X-01.** A transaction nobody signed — a 64-byte constant in the signature
slot, naming a stranger as the transfer authority — passes `verify`. The economic checks
pass too, so the facilitator reports `is_valid: true` and names the victim as payer. -/
theorem finding_X01_unsigned_payload_verifies (r : Requirements) (victim : Nat) :
    verifyImpl r (forgedTx r victim) = true ∧ verifyStrict r (forgedTx r victim) = false ∧
      (forgedTx r victim).transfer.authority = victim := by
  refine ⟨by simp [verifyImpl, economicallyValid, forgedTx], ?_, rfl⟩
  simp only [verifyStrict, chainAccepts, forgedTx, Bool.and_eq_false_iff]
  simp only [List.any_cons, List.any_nil, Bool.or_false, Bool.and_eq_false_iff,
    decide_eq_false_iff_not]
  refine Or.inr (Or.inr ?_)
  simp only [msgHash]
  omega

/-- With the check restored, an accepted payload really is authorised: some non-placeholder
signature by the transfer authority, over this exact message. -/
theorem strict_verify_is_authorised (r : Requirements) (tx : Tx) (h : verifyStrict r tx = true) :
    ∃ s ∈ tx.sigs, s.zero = false ∧ s.signer = tx.transfer.authority ∧ s.over = msgHash tx := by
  simp only [verifyStrict, Bool.and_eq_true, chainAccepts, List.any_eq_true] at h
  obtain ⟨_, s, hmem, hs⟩ := h
  simp only [Bool.not_eq_true', decide_eq_true_eq] at hs
  exact ⟨s, hmem, hs.1.1, hs.1.2, hs.2⟩

/-- And a strictly-verified payload is one the chain will accept, which is the property
that makes "verified" mean "collectible". -/
theorem strict_verify_settles (r : Requirements) (tx : Tx) (h : verifyStrict r tx = true) :
    chainAccepts tx = true := by
  simp only [verifyStrict, Bool.and_eq_true] at h
  exact h.2

/-! ## Settlement rebinds the blockhash the client signed over -/

/-- `Facilitator::submit`: overwrite `recent_blockhash`, then add only the *fee payer's*
signature. The client's signature is carried over untouched. -/
def rebind (fresh : Nat) (tx : Tx) : Tx := { tx with blockhash := fresh }

/-- Rebinding changes the message. -/
theorem msgHash_injective_in_blockhash (tx : Tx) (h₁ h₂ : Nat) (h : h₁ ≠ h₂) :
    msgHash (rebind h₁ tx) ≠ msgHash (rebind h₂ tx) := by
  simp only [msgHash, rebind]
  omega

/-- **Finding X-03.** The bundled client signs over `Hash::default()`, and the facilitator
replaces the blockhash before submitting. The payer's signature then commits to a message
that is not the one submitted, so the network rejects it: a payment that `verify` accepted
can never actually settle. (The middleware has already served the resource by then, and
discards the settlement error.) -/
theorem finding_X03_rebinding_breaks_the_payer_signature (tx : Tx) (fresh : Nat)
    (hfresh : fresh ≠ tx.blockhash)
    (honest : ∀ s ∈ tx.sigs, s.over = msgHash tx) :
    chainAccepts (rebind fresh tx) = false := by
  cases hc : chainAccepts (rebind fresh tx) with
  | false => rfl
  | true =>
      exfalso
      simp only [chainAccepts, List.any_eq_true] at hc
      obtain ⟨s, hmem, hs⟩ := hc
      simp only [Bool.and_eq_true, Bool.not_eq_true', decide_eq_true_eq] at hs
      have h1 : s.over = msgHash tx := honest s hmem
      have h2 : s.over = msgHash (rebind fresh tx) := hs.2
      rw [h1] at h2
      simp only [msgHash, rebind] at h2
      omega

/-! ## Serving before settling, and replay

`payment_middleware` runs `verify`, serves the resource, and spawns settlement into a
detached task. Nothing records that a payload was used. The model is a gate over a history
of payloads already seen.
-/

/-- A payload is the transaction the header carries. -/
abbrev Payload := Tx

/-- The gate as written: the history is not consulted. -/
def gateImpl (r : Requirements) (_seen : List Payload) (p : Payload) : Bool :=
  verifyImpl r p

/-- The gate with a settled-payload store, which is what stops a captured header from
being reused. -/
def gateNonced (r : Requirements) (seen : List Payload) (p : Payload) : Bool :=
  verifyStrict r p && !(decide (p ∈ seen))

/-- **Finding X-02.** One captured `X-Payment` header buys unlimited access: the gate's
answer does not depend on how many times the payload has already been served. -/
theorem finding_X02_replay_is_unbounded (r : Requirements) (p : Payload) (seen seen' : List Payload) :
    gateImpl r seen p = gateImpl r seen' p := rfl

/-- Concretely: a payload that was accepted once is accepted again after being served. -/
theorem finding_X02_second_use_still_accepted (r : Requirements) (p : Payload) (seen : List Payload)
    (h : gateImpl r seen p = true) : gateImpl r (p :: seen) p = true := h

/-- The nonced gate accepts a payload at most once. -/
theorem nonced_gate_rejects_replay (r : Requirements) (p : Payload) (seen : List Payload) :
    gateNonced r (p :: seen) p = false := by
  simp [gateNonced]

/-- …and it still admits a fresh, properly signed payment. -/
theorem nonced_gate_admits_fresh (r : Requirements) (p : Payload) (seen : List Payload)
    (hv : verifyStrict r p = true) (hs : p ∉ seen) :
    gateNonced r seen p = true := by
  simp [gateNonced, hv, hs]

/-! ## What "paid" should mean

Putting the three findings together: with the implementation's gate, being served does not
imply that anything was, or could be, collected. With the strict gate plus a settled-payload
store, it does — and that is the property to hold the crate to.
-/

/-- The desired end-to-end guarantee, and it holds of the fixed gate: anything served was
authorised by its payer, for the exact required amount and recipient, and is a transaction
the chain will accept. -/
theorem nonced_gate_served_implies_collectible (r : Requirements) (p : Payload)
    (seen : List Payload) (h : gateNonced r seen p = true) :
    economicallyValid r p = true ∧ chainAccepts p = true := by
  simp only [gateNonced, Bool.and_eq_true, verifyStrict] at h
  exact ⟨h.1.1, h.1.2⟩

/-- The same statement fails for the gate as written — served, and not collectible. -/
theorem finding_X01_served_but_not_collectible (r : Requirements) (victim : Nat) :
    gateImpl r [] (forgedTx r victim) = true ∧ chainAccepts (forgedTx r victim) = false := by
  obtain ⟨h1, h2, _⟩ := finding_X01_unsigned_payload_verifies r victim
  refine ⟨h1, ?_⟩
  simp only [verifyStrict, Bool.and_eq_false_iff] at h2
  rcases h2 with h2 | h2
  · simp only [verifyImpl, Bool.and_eq_true] at h1
    exact absurd h1.1 (by simp [h2])
  · exact h2

end Audit.X402
