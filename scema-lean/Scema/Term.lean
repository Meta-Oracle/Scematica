/-
  `Term` and `Coverage` — the rule this whole project is built on, as theorems.

  Scematica states one discipline everywhere: **an unmeasured thing must never look like a
  measured zero.** It is enforced in Rust by `Term { value, measured }`, in the renderers by
  an em dash, in the Ψ gates by a neutral element, and in the NN state vector by a
  `FeatureMask`. In every one of those places it is enforced by *convention and tests*.

  Here it is enforced by proof, and the interesting result is a negative one: the arithmetic
  provably **cannot** tell a measured zero from an unmeasured term (`contribution_collapses`),
  which is exactly why the *rendering* has to (`render_distinguishes`). The two theorems
  together are the argument for the em-dash rule, and neither is obvious until it is written
  down: the first says information is genuinely destroyed by aggregation, the second says the
  only place it survives is the string.

  ## Why values are `Int`, not `Float`

  Not an approximation of the Rust. `scema-verify`'s canonical encoding already hashes a
  float as `round(v * 1e9)` in `i64`, because `serde_json`'s parser is not correctly rounded
  and a commitment over raw IEEE-754 bits is unverifiable the moment a record crosses JSON.
  So the *committed* value of every term in Scematica is already an integer in units of 1e-9.
  Modelling it as `Int` here is faithful to what is actually signed, and it makes every
  statement below decidable.
-/

namespace Scema

/-- Fixed-point scale: values are integers in units of 1e-9, matching `canonical.rs`. -/
def scale : Int := 1000000000

/--
  One dimension of a judgement, and whether anybody measured it.

  `value` is meaningful only when `measured` is true. It is deliberately *not* an
  `Option Int`: the Rust carries a value alongside the flag too, and modelling it as an
  option would make the collapse below unstateable — there would be no measured zero to
  confuse with an absent one.
-/
structure Term where
  value : Int
  measured : Bool
deriving DecidableEq, Repr

namespace Term

/-- A term that was measured, carrying `v`. -/
def counted (v : Int) : Term := ⟨v, true⟩

/-- A term nobody measured. The `value` is carried but must never be read. -/
def absent : Term := ⟨0, false⟩

/--
  What this term contributes to an aggregate.

  The additive neutral element when unmeasured. This is the choice `scema-policy` makes and
  the one `scematica-mesh::cognition` had to be corrected *to* — a multiplicative form pins
  the whole aggregate at zero on any unmeasured dimension, which is how the sentience Ψ
  jammed at 0 and how the agentic gate jammed shut on subsystems nobody had built.
-/
def contribution (t : Term) : Int := if t.measured then t.value else 0

end Term

/-! ## The two theorems that justify the em dash -/

/-- An unmeasured term contributes the additive neutral element. -/
theorem unmeasured_contributes_neutral (t : Term) (h : t.measured = false) :
    t.contribution = 0 := by
  simp [Term.contribution, h]

/-- A measured term contributes exactly what was measured, including when that is zero. -/
theorem measured_contributes_its_value (t : Term) (h : t.measured = true) :
    t.contribution = t.value := by
  simp [Term.contribution, h]

/--
  **The arithmetic cannot tell them apart.**

  A measured zero and an unmeasured term make the same contribution to every additive
  aggregate. Information is genuinely destroyed here — this is not a deficiency in the
  implementation, it is a property of summation.
-/
theorem contribution_collapses :
    (Term.counted 0).contribution = Term.absent.contribution := by
  decide

/-- ...and yet they are different observations. -/
theorem counted_zero_ne_absent : Term.counted 0 ≠ Term.absent := by
  decide

/--
  Formatting a term. The single rule `scema_policy::render::cell` implements, and that
  `lib/omni/view.ts`, the extension HUD and `scema-tui` each port.
-/
def render (t : Term) : String :=
  if t.measured then toString t.value else "—"

/--
  **Therefore the rendering must distinguish them, and does.**

  Paired with `contribution_collapses`: since the arithmetic loses the distinction, the
  string is the only place it survives. A renderer that printed `0` for an unmeasured term
  would destroy the last copy of it.
-/
theorem render_distinguishes : render (Term.counted 0) ≠ render Term.absent := by
  decide

/-- An unmeasured term always renders as the em dash, whatever value it carries. -/
theorem absent_renders_as_dash (v : Int) : render ⟨v, false⟩ = "—" := by
  simp [render]

/-! ## Coverage -/

/--
  How much of an aggregate was measured.

  Kept as a pair rather than a ratio on purpose, and the reason is in `scema-tui`: a
  proportional bar renders 2/5 and 4/10 identically, and the denominator is the number that
  matters. A `Coverage` that had been divided cannot be un-divided.
-/
structure Coverage where
  measured : Nat
  total : Nat
deriving DecidableEq, Repr

namespace Coverage

/-- Coverage of a list of terms. -/
def of (ts : List Term) : Coverage :=
  ⟨ts.countP (·.measured), ts.length⟩

/-- A coverage is well-formed when it does not claim more measured than exist. -/
def valid (c : Coverage) : Prop := c.measured ≤ c.total

end Coverage

/-- Coverage never claims more measured terms than there are terms. -/
theorem coverage_valid (ts : List Term) : (Coverage.of ts).valid := by
  simpa [Coverage.of, Coverage.valid] using List.countP_le_length (l := ts) (p := (·.measured))

/-- An empty aggregate has coverage 0/0 — which is *undefined*, not zero percent. -/
theorem coverage_of_nil : Coverage.of [] = ⟨0, 0⟩ := by
  decide

/-- Adding a measured term raises the numerator and the denominator together. -/
theorem coverage_cons_counted (v : Int) (ts : List Term) :
    Coverage.of (Term.counted v :: ts)
      = ⟨(Coverage.of ts).measured + 1, (Coverage.of ts).total + 1⟩ := by
  simp [Coverage.of, Term.counted, List.countP_cons, Nat.add_comm]

/-- Adding an unmeasured term raises only the denominator. -/
theorem coverage_cons_absent (ts : List Term) :
    Coverage.of (Term.absent :: ts)
      = ⟨(Coverage.of ts).measured, (Coverage.of ts).total + 1⟩ := by
  simp [Coverage.of, Term.absent, List.countP_cons]

/-! ## The utility equation -/

/--
  Weights on the utility equation. A *stated preference*, never a fitted parameter — which
  is why they are hashed into every decision record.
-/
structure Weights where
  risk : Int
  cost : Int
  uncertainty : Int
  reversibility : Int
deriving Repr

/--
  `U = R − λ₁K − λ₂C − λ₃U + λ₄V`, additive.

  A multiplicative form is more expressive and is the trap this repository has paid for
  twice. Additive means an unmeasured dimension contributes `0` and the score degrades
  gracefully instead of collapsing.
-/
def utility (w : Weights) (gain risk cost uncertainty reversibility : Term) : Int :=
  gain.contribution
    - w.risk * risk.contribution
    - w.cost * cost.contribution
    - w.uncertainty * uncertainty.contribution
    + w.reversibility * reversibility.contribution

/--
  **A branch with nothing measured scores exactly zero.**

  Not "near zero" and not "undefined": the additive identity. This is what makes
  `NoPositiveUtility` a meaningful abstention rather than an artefact — on a barely-perceived
  world most branches project exactly 0 and the agent declines, which is the correct and
  uncomfortable consequence stated in the omni docs.
-/
theorem utility_of_nothing_measured (w : Weights) :
    utility w Term.absent Term.absent Term.absent Term.absent Term.absent = 0 := by
  simp [utility, Term.contribution, Term.absent]

/--
  An unmeasured risk cannot lower a score.

  The reason the neutral element has to be additive-zero rather than, say, a pessimistic
  default: a gate that treated "nobody measured the risk" as "maximum risk" would pin itself
  shut on exactly the subsystems nobody has built yet.
-/
theorem unmeasured_risk_does_not_penalise (w : Weights) (g c u v : Term) :
    utility w g Term.absent c u v = utility w g (Term.counted 0) c u v := by
  simp [utility, Term.contribution, Term.absent, Term.counted]

end Scema
