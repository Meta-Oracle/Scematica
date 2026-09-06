/-
  `scema-effect` and `scema-cli execute` — the two gates in front of the agent's write path.

  This is the highest-consequence code in the omni runtime, because it is the only place an
  autonomous agent can change a machine. It is also the code most likely to be misread as
  unsafe by a reviewer skimming for `std::process::Command`: the command runner is *there*,
  and the reason it is safe is that nothing reaches it without passing a path confinement
  and a trust decision, in that order.

  The audit's claim is a conservative one and it is what this module proves. Define
  `attempted` as "the disk may have been touched" — success, failure, or the honest
  `Unknown` arm where a write returned `Ok` and could not be read back. Then:

  * a dry run is never `attempted`, and never prompts;
  * anything `attempted` was committed, confined, and either allowed by an existing policy
    rule or approved by the operator at the prompt;
  * with the CLI's defaults (no `--allow-writes`, no `--yes`, no terminal) nothing is ever
    `attempted`, whatever the effect asks for.

  What is *not* claimed: `Effect::Run` confines only the working directory, so an approved
  command can still write outside the workspace. That is the operator's decision to make
  and the audit reports it as a scope limitation rather than a defect — see `SECURITY-AUDIT.md`.
-/

namespace Audit.Effect

inductive Effect where
  | writeFile (path contents : String)
  | createDir (path : String)
  | run (argv : List String) (cwd : String)
deriving DecidableEq, Repr

inductive Mode where
  | dryRun | commit
deriving DecidableEq, Repr

inductive RefusedBy where
  | workspace | policy | operator
deriving DecidableEq, Repr

inductive Outcome where
  | succeeded | failed | unknown
  | refused (by_ : RefusedBy)
  | simulated
deriving DecidableEq, Repr

/-- `Outcome::changed_the_world` — success only. -/
def Outcome.changedTheWorld : Outcome → Bool
  | .succeeded => true
  | _ => false

/-- The conservative predicate the gate argument is about: the effect was carried out, so
the disk may have been touched. `Unknown` counts, which is the whole reason that arm
exists. -/
def Outcome.attempted : Outcome → Bool
  | .succeeded | .failed | .unknown => true
  | _ => false

theorem changed_implies_attempted (o : Outcome) (h : o.changedTheWorld = true) :
    o.attempted = true := by
  cases o <;> simp_all [Outcome.changedTheWorld, Outcome.attempted]

/-- What the environment contributes to one `exec::run` call.

`confined` is `Workspace::resolve` plus the protected-name re-check on a rebuilt leaf:
`none` when the path escapes the roots, contains an unresolvable `..`, or names a
protected file. `preflight` is `TrustPolicy::preflight`: `some true` for an allowing rule,
`some false` for a refusing one, `none` for "no rule, ask". `approver` is the answer a
prompt would get — `AutoApprover` under `--yes`, `DenyApprover` otherwise, which is also
what a non-interactive stdin gets. -/
structure Env where
  confined : Option String
  preflight : Option Bool
  approver : Bool
deriving DecidableEq, Repr

/-- The result of one call: an outcome, and whether a prompt was shown. `perform` stands
for the three effect arms — they differ in what they write and in how they observe it, and
that difference is below the gates. -/
def run (e : Effect) (env : Env) (mode : Mode) (perform : Effect → Outcome) :
    Outcome × Bool :=
  match env.confined with
  | none => (.refused .workspace, false)
  | some _ =>
      match env.preflight with
      | some false => (.refused .policy, false)
      | some true => if mode = .dryRun then (.simulated, false) else (perform e, false)
      | none =>
          if mode = .dryRun then (.simulated, false)
          else if env.approver then (perform e, true)
          else (.refused .operator, true)

/-- **A dry run touches nothing.** Whatever the effect, the policy or the operator would
say. -/
theorem dry_run_never_attempts (e : Effect) (env : Env) (perform : Effect → Outcome) :
    (run e env .dryRun perform).1.attempted = false := by
  simp only [run]
  split
  · rfl
  · split <;> simp [Outcome.attempted]

/-- **A dry run never prompts.** Asking somebody to approve an act that is not going to
happen is how a prompt becomes a formality. -/
theorem dry_run_never_prompts (e : Effect) (env : Env) (perform : Effect → Outcome) :
    (run e env .dryRun perform).2 = false := by
  simp only [run]
  split
  · rfl
  · split <;> simp

/-- **Both gates are load-bearing.** Anything that reached the disk was committed, had a
confined path, and was either allowed by a policy rule or approved at a prompt. -/
theorem attempted_implies_both_gates (e : Effect) (env : Env) (mode : Mode)
    (perform : Effect → Outcome) (h : (run e env mode perform).1.attempted = true) :
    mode = .commit ∧ env.confined.isSome = true ∧
      (env.preflight = some true ∨ (env.preflight = none ∧ env.approver = true)) := by
  simp only [run] at h
  split at h
  · exact absurd h (by simp [Outcome.attempted])
  · rename_i path hconf
    split at h
    · exact absurd h (by simp [Outcome.attempted])
    · rename_i hp
      split at h
      · rename_i hdry
        exact absurd h (by simp [Outcome.attempted])
      · rename_i hdry
        refine ⟨by cases mode <;> simp_all, by simp [hconf], Or.inl hp⟩
    · rename_i hp
      split at h
      · rename_i hdry; exact absurd h (by simp [Outcome.attempted])
      · rename_i hdry
        split at h
        · rename_i hyes
          refine ⟨by cases mode <;> simp_all, by simp [hconf], Or.inr ⟨hp, hyes⟩⟩
        · exact absurd h (by simp [Outcome.attempted])

/-- An unconfined path is refused by the first gate, so `--commit --yes` on a path outside
the workspace still does nothing. -/
theorem unconfined_is_refused (e : Effect) (env : Env) (mode : Mode)
    (perform : Effect → Outcome) (h : env.confined = none) :
    run e env mode perform = (.refused .workspace, false) := by
  simp [run, h]

/-- **The CLI defaults are safe.** No `--allow-writes` leaves the policy without a rule;
no `--yes` (or no terminal) makes the prompt a refusal. Under those two, `--commit` still
carries nothing out. -/
theorem cli_defaults_never_attempt (e : Effect) (path : String) (perform : Effect → Outcome) :
    run e { confined := some path, preflight := none, approver := false } .commit perform
      = (.refused .operator, true) := by
  simp [run]

/-- A refusing policy rule is final: the operator is not asked to override it. This is the
difference between `Refused Policy` and `Refused Operator`, and it is why the two are
separate variants. -/
theorem policy_refusal_is_not_promptable (e : Effect) (path : String) (approver : Bool)
    (mode : Mode) (perform : Effect → Outcome) :
    run e { confined := some path, preflight := some false, approver := approver } mode perform
      = (.refused .policy, false) := by
  simp [run]

end Audit.Effect
