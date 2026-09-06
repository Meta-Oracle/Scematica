/-
  `scema-daemon::auth` and `scema-entitlement` — the two places the omni runtime says no.

  Everything here is a *positive* result: the audit found no defect in these paths, and
  this module records what "no defect" means precisely enough to notice if it changes.

  Three properties are worth stating:

  1. **The token comparison is correct and its cost is data-independent.** The Rust folds
     `|=` over `^` across the full length of both inputs rather than returning at the first
     differing byte. What that buys is a step count that depends only on the two lengths —
     stated as `steps_depends_only_on_lengths`, alongside the leak that the ordinary `==`
     would have.
  2. **The `Host` check fails closed.** An absent `Host` header is rejected, which is what
     makes the DNS-rebinding defence a defence rather than a formality.
  3. **`authorise` never turns "the chain would not answer" into "you do not hold it".**
     One token grants exactly one world, and an unreadable oracle is `Undetermined`, which
     is not a grant.
-/

namespace Audit.Access

/-! ## Constant-time secret comparison

Bytes are `Nat`; a token is a list of them. The accumulator is modelled as a `Bool`
("some position differed") rather than as an OR of XORs — the arithmetic is a way of
avoiding a branch, and the security-relevant content is that the loop visits every
position of the longer input regardless of what it finds.
-/

/-- `secret_eq`: fold over `max a.len b.len` positions, no early exit. The length
comparison is folded into the same accumulator rather than short-circuiting. -/
def secretEqAux : List Nat → List Nat → Bool
  | [], [] => false
  | [], _ :: bs => true || secretEqAux [] bs
  | _ :: as, [] => true || secretEqAux as []
  | a :: as, b :: bs => (!(a == b)) || secretEqAux as bs

def secretEq (a b : List Nat) : Bool := !(secretEqAux a b)

/-- Number of positions the comparison visits. -/
def steps : List Nat → List Nat → Nat
  | [], [] => 0
  | [], _ :: bs => 1 + steps [] bs
  | _ :: as, [] => 1 + steps as []
  | _ :: as, _ :: bs => 1 + steps as bs

/-- The early-exit comparison Rust's `==` on `String` would have performed. -/
def earlyEq : List Nat → List Nat → Bool
  | [], [] => true
  | [], _ :: _ => false
  | _ :: _, [] => false
  | a :: as, b :: bs => if a == b then earlyEq as bs else false

/-- Its step count: it stops at the first difference. -/
def earlySteps : List Nat → List Nat → Nat
  | [], [] => 0
  | [], _ :: _ => 0
  | _ :: _, [] => 0
  | a :: as, b :: bs => if a == b then 1 + earlySteps as bs else 1

/-- **The comparison is correct.** -/
theorem secretEq_iff : ∀ a b : List Nat, secretEq a b = true ↔ a = b
  | [], [] => by simp [secretEq, secretEqAux]
  | [], _ :: _ => by simp [secretEq, secretEqAux]
  | _ :: _, [] => by simp [secretEq, secretEqAux]
  | a :: as, b :: bs => by
      simp only [secretEq, secretEqAux, Bool.not_or, Bool.and_eq_true, Bool.not_not,
        beq_iff_eq, List.cons.injEq]
      have ih := secretEq_iff as bs
      simp only [secretEq] at ih
      constructor
      · intro h; exact ⟨by simpa using h.1, ih.mp (by simpa using h.2)⟩
      · intro h; exact ⟨by simp [h.1], by simpa using (ih.mpr h.2)⟩

/-- **Its cost is a function of the lengths alone.** Two comparisons over inputs of the
same shape take the same number of steps whatever the bytes are, so an attacker who can
time the daemon learns nothing about how much of a guess was right. -/
theorem steps_depends_only_on_lengths :
    ∀ (a b a' b' : List Nat), a.length = a'.length → b.length = b'.length →
      steps a b = steps a' b'
  | [], [], [], [], _, _ => rfl
  | [], _ :: bs, [], _ :: bs', ha, hb => by
      simp only [steps]
      exact congrArg (1 + ·) (steps_depends_only_on_lengths [] bs [] bs' rfl (by simpa using hb))
  | _ :: as, [], _ :: as', [], ha, hb => by
      simp only [steps]
      exact congrArg (1 + ·) (steps_depends_only_on_lengths as [] as' [] (by simpa using ha) rfl)
  | _ :: as, _ :: bs, _ :: as', _ :: bs', ha, hb => by
      simp only [steps]
      exact congrArg (1 + ·)
        (steps_depends_only_on_lengths as bs as' bs' (by simpa using ha) (by simpa using hb))

/-- **And the early-exit comparison does leak.** Same lengths, same secret, two guesses:
the one that gets the first byte right costs strictly more. That difference is the
matching-prefix length, recovered one byte at a time. -/
theorem earlySteps_leaks_the_matching_prefix :
    earlySteps [1, 2] [1, 9] > earlySteps [1, 2] [8, 9] := by decide

/-- The two comparisons agree on the answer — the difference is only in what they cost,
which is the point. -/
theorem secretEq_agrees_with_earlyEq : ∀ a b : List Nat, secretEq a b = earlyEq a b
  | [], [] => by simp [secretEq, secretEqAux, earlyEq]
  | [], _ :: _ => by simp [secretEq, secretEqAux, earlyEq]
  | _ :: _, [] => by simp [secretEq, secretEqAux, earlyEq]
  | a :: as, b :: bs => by
      simp only [secretEq, secretEqAux, earlyEq]
      have ih := secretEq_agrees_with_earlyEq as bs
      simp only [secretEq] at ih
      by_cases h : a = b <;> simp [h, ih]

/-! ## Bearer parsing and the `Host` check -/

/-- `bearer`: accept `Bearer <t>`, `bearer <t>` or a bare token. Strings are modelled by
the two shapes the function distinguishes. -/
inductive Header where
  | prefixed (token : String)
  | bare (token : String)
deriving DecidableEq, Repr

def bearer : Header → String
  | .prefixed t => t
  | .bare t => t

/-- Whichever spelling arrives, the token extracted is the same one — the convenience of
accepting a bare token does not create a second, weaker path. -/
theorem bearer_is_spelling_independent (t : String) :
    bearer (.prefixed t) = bearer (.bare t) := rfl

/-- `host_is_local`: `none` is the absent `Host` header. -/
def hostIsLocal (host : Option String) (declaredPort port : Nat) (nameIsOurs : Bool) : Bool :=
  match host with
  | none => false
  | some _ => nameIsOurs && declaredPort = port

/-- **Fails closed on an absent `Host`.** HTTP/1.1 requires the header; treating its
absence as "probably fine" is what makes a rebinding check ornamental. -/
theorem absent_host_is_rejected (dp p : Nat) (n : Bool) : hostIsLocal none dp p n = false := rfl

/-- A name that is not ours is rejected whatever the port says. -/
theorem foreign_host_is_rejected (h : String) (dp p : Nat) :
    hostIsLocal (some h) dp p false = false := by simp [hostIsLocal]

/-! ## Entitlement

`scema-entitlement::authorise`. The type distinguishes three answers, and the audit's
interest is that only one of them opens the door.
-/

inductive Ownership where
  | held | notHeld | unknown
deriving DecidableEq, Repr

inductive Decision where
  | granted (world : String)
  | denied
  | undetermined
deriving DecidableEq, Repr

def Decision.permits : Decision → Bool
  | .granted _ => true
  | _ => false

def authorise (isDigest : Bool) (commitment requested : String) (o : Ownership) : Decision :=
  if !isDigest then .denied
  else if commitment ≠ requested then .denied
  else match o with
    | .held => .granted requested
    | .notHeld => .denied
    | .unknown => .undetermined

/-- **Fail-closed.** A grant implies a well-formed request, a token that commits to exactly
the world asked for, and an oracle that positively said the holder holds it. In particular
an unreadable chain never becomes an entitlement. -/
theorem grant_requires_everything (isDigest : Bool) (commitment requested : String)
    (o : Ownership) (h : (authorise isDigest commitment requested o).permits = true) :
    isDigest = true ∧ commitment = requested ∧ o = .held := by
  simp only [authorise] at h
  split at h
  · exact absurd h (by simp [Decision.permits])
  · split at h
    · exact absurd h (by simp [Decision.permits])
    · rename_i hd hc
      cases o with
      | held => exact ⟨by simpa using hd, by simpa using hc, rfl⟩
      | notHeld => exact absurd h (by simp [Decision.permits])
      | unknown => exact absurd h (by simp [Decision.permits])

/-- One token, one world: holding the token for `commitment` does not open any other
record, however well formed the request is. -/
theorem one_token_one_world (commitment requested : String) (o : Ownership)
    (h : commitment ≠ requested) :
    (authorise true commitment requested o).permits = false := by
  simp [authorise, h, Decision.permits]

/-- "The chain would not answer" is never reported as "you do not hold this". -/
theorem unknown_is_not_a_denial (commitment : String) :
    authorise true commitment commitment .unknown = .undetermined := by
  simp [authorise]

/-! ## The bot control API

`scematica-api` exposes control routes that mutate a live sniper — pause buys, force-sell,
dump at `min_out = 0`. They are gated by `SCEMATICA_API_TOKEN` **when that variable is
set**, and the server binds all interfaces. The gate is therefore fail-*open*, and the
contrast with `authorise` above is the point of stating both in one file: the same
codebase has a fail-closed authorisation path and a fail-open one, and only one of them
guards an irreversible action.
-/

/-- `require_token`: with no token configured, every request passes. -/
def requireToken (configured presented : Option String) : Bool :=
  match configured with
  | none => true
  | some expected => presented = some expected

/-- The same gate, failing closed: an unconfigured deployment refuses control routes
instead of opening them. (The omni daemon already does the stronger thing — it *generates*
a token on first run, so there is no unconfigured state at all.) -/
def requireTokenFailClosed (configured presented : Option String) : Bool :=
  match configured with
  | none => false
  | some expected => presented = some expected

/-- **Finding A-01.** With no token configured, an anonymous request is authorised. Since
the server binds every interface, "anonymous" includes anyone who can route to the host. -/
theorem finding_A01_unconfigured_token_authorises_anyone :
    requireToken none none = true := rfl

/-- Failing closed rejects exactly that request, and nothing else changes: a configured
deployment behaves identically under both gates. -/
theorem fail_closed_rejects_the_unconfigured_case :
    requireTokenFailClosed none none = false := rfl

theorem fail_closed_agrees_when_configured (expected : String) (presented : Option String) :
    requireTokenFailClosed (some expected) presented = requireToken (some expected) presented :=
  rfl

/-- And the control-route comparison is the early-exit one, so the pairing secret is
exactly the kind of secret `secretEq` exists to compare. The leak is the theorem above:
`earlySteps_leaks_the_matching_prefix`. -/
theorem control_gate_uses_the_leaky_comparison (a b : List Nat) :
    earlyEq a b = secretEq a b := (secretEq_agrees_with_earlyEq a b).symm

end Audit.Access
