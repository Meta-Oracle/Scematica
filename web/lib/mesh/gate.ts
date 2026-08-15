// Counterfactual recomputation of the agentic gate, in the browser.
//
// **`crates/scematica-mesh/src/cognition.rs` is authoritative.** This is a port of its
// assembly arithmetic, and it exists for one reason: the equations are pure functions, so
// the page can answer *"what would it take to open this gate?"* without a round trip and
// without the bot having to actually be in that state.
//
// That is the whole interaction. A static Ψ tells you the gate is shut. A Ψ you can push
// on tells you WHICH term is holding it shut and by how much — the difference between a
// readout and an instrument.
//
// ## The rule that makes this safe
//
// A counterfactual must never be mistakable for an observation. Every function here takes
// an explicit `overrides` map and returns `dirty: true` the moment one is non-empty; the
// UI is required to change state visibly when that flips. Nothing in this file writes
// anything, requests anything, or influences the bot in any way — it is arithmetic over a
// payload that has already been fetched.
//
// ## Parity
//
// `scripts/check-mesh.mjs` pins these formulas against fixtures taken from the Rust unit
// tests. If cognition.rs changes its λ weights, its τ thresholds, or the rule that risk
// averages over measured components only, that script fails.

import type { Cognition, GateVerdict, Term } from './types'

/** Mirrors `TAU_PSI` in cognition.rs. */
export const TAU_PSI = 0.45
/** Mirrors `TAU_PSI_FULL`. */
export const TAU_PSI_FULL = 0.75

/** Mirrors the λ weights on §17's confidence terms. */
export const LAMBDA: Record<string, number> = {
  U_A: 1.0,
  U_E: 1.0,
  N_t: 0.5,
  D_t: 1.0,
}

export interface GateResult {
  confidence: number
  coherence: number
  risk: number
  psi: number
  verdict: GateVerdict
  measuredFraction: number
  /** True when any override is in play. The UI MUST look different when this is set. */
  dirty: boolean
}

export type Overrides = Record<string, number>

/**
 * Effective value and measured-ness of a term under a set of overrides.
 *
 * An overridden term counts as **measured**, and that is the instructive part rather than
 * a shortcut: dragging an unmeasured risk component from 0 changes the denominator of the
 * risk mean, so the page shows that instrumenting a subsystem can *raise* the reported
 * risk even when the subsystem is healthy. That is a true and counter-intuitive property
 * of the design, and hiding it would make the instrument lie in the comfortable direction.
 */
export function effective(term: Term, overrides: Overrides): { value: number; measured: boolean } {
  const o = overrides[term.symbol]
  if (o === undefined) return { value: term.value, measured: term.measured }
  return { value: o, measured: true }
}

/** §17 — confidence, in the anchored form (`1 − tanh(Σλᵢuᵢ / 2)`). */
export function confidenceOf(terms: Term[], overrides: Overrides): number {
  let load = 0
  for (const t of terms) {
    const { value } = effective(t, overrides)
    load += (LAMBDA[t.symbol] ?? 1.0) * value
  }
  return 1 - Math.tanh(load / 2)
}

/**
 * §20 — the risk field: mean over MEASURED components only.
 *
 * Averaging in unmeasured zeros would divide a real 1.0 model risk by six and report 0.17,
 * which reads as safe. Same rule as the Rust.
 */
export function riskOf(components: Term[], overrides: Overrides): number {
  const measured = components
    .map(t => effective(t, overrides))
    .filter(e => e.measured)
    .map(e => e.value)
  if (measured.length === 0) return 0
  return measured.reduce((a, b) => a + b, 0) / measured.length
}

export function verdictFor(psi: number, anyMeasured: boolean, subsystems: number): GateVerdict {
  if (!anyMeasured || subsystems === 0) return 'unevaluated'
  if (psi < TAU_PSI) return 'abstain'
  if (psi < TAU_PSI_FULL) return 'damp'
  return 'act'
}

/** §32 — Ψ = C · K · (1 − R), recomputed under overrides. */
export function recompute(c: Cognition, overrides: Overrides = {}, coherenceOverride?: number): GateResult {
  const coherence = coherenceOverride ?? c.coherence.value
  const confidence = confidenceOf(c.confidence_terms, overrides)
  const risk = riskOf(c.risk.components, overrides)
  const psi = confidence * coherence * (1 - risk)

  const all = [...c.confidence_terms, ...c.risk.components, ...c.omega_terms]
  const measuredCount = all.filter(t => effective(t, overrides).measured).length
  const dirty = Object.keys(overrides).length > 0 || coherenceOverride !== undefined

  return {
    confidence,
    coherence,
    risk,
    psi,
    verdict: verdictFor(psi, measuredCount > 0, c.coherence.subsystems),
    measuredFraction: all.length === 0 ? 0 : measuredCount / all.length,
    dirty,
  }
}

export interface Sensitivity {
  symbol: string
  section: string
  name: string
  /** ∂Ψ/∂term, numerically. Negative means raising this term closes the gate. */
  gradient: number
  measured: boolean
}

/**
 * How hard each term pushes on Ψ, right now.
 *
 * Numerical rather than analytic on purpose: the analytic derivative of the risk mean
 * changes discontinuously as a term crosses from unmeasured to measured, and a numerical
 * probe over the *actual* recomputation cannot drift from the function it is describing.
 * Correctness by construction beats a tidier formula here.
 */
export function sensitivities(c: Cognition, overrides: Overrides = {}): Sensitivity[] {
  const base = recompute(c, overrides).psi
  const H = 0.01
  const out: Sensitivity[] = []

  for (const t of [...c.confidence_terms, ...c.risk.components]) {
    const e = effective(t, overrides)
    const bumped = { ...overrides, [t.symbol]: Math.min(1, e.value + H) }
    const psi = recompute(c, bumped).psi
    out.push({
      symbol: t.symbol,
      section: t.section,
      name: t.name,
      gradient: (psi - base) / H,
      measured: e.measured,
    })
  }

  // Steepest push first, in either direction.
  return out.sort((a, b) => Math.abs(b.gradient) - Math.abs(a.gradient))
}

/**
 * The single most useful sentence: what is holding the gate, and what would move it.
 *
 * Reports only over MEASURED terms. Telling an operator to "reduce novelty" when novelty
 * is an unmeasured placeholder would send them after a subsystem that does not exist.
 */
export function dominantConstraint(c: Cognition, overrides: Overrides = {}): string {
  const r = recompute(c, overrides)
  const measured = sensitivities(c, overrides).filter(s => s.measured && Math.abs(s.gradient) > 1e-9)

  if (measured.length === 0) {
    return 'No measured term currently moves Ψ — there is no constraint to name, only an absence of evidence.'
  }

  // `unevaluated` means the gate cannot render a verdict (no live subsystem is speaking),
  // which is NOT the same as having nothing measured. Terms can be well measured while the
  // verdict is unavailable, and saying "nothing is measured" there would be false — it
  // would also hide the one number the operator can currently act on.
  if (r.verdict === 'unevaluated') {
    const top = measured[0]
    return `Ψ has no verdict — no live subsystem is speaking, so K is neutral rather than measured. Of what IS measured, ${top.symbol} (${top.name}) has the most leverage.`
  }

  const top = measured[0]
  const dir = top.gradient < 0 ? 'lowering' : 'raising'
  if (r.psi >= TAU_PSI_FULL) {
    return `Ψ is above τ_full. The term with most leverage is ${top.symbol} (${top.name}) — ${dir} it moves Ψ fastest.`
  }
  const gap = (r.psi < TAU_PSI ? TAU_PSI : TAU_PSI_FULL) - r.psi
  const needed = Math.abs(gap / top.gradient)
  return `Ψ is ${gap.toFixed(3)} short. ${top.symbol} (${top.name}) has the most leverage: ${dir} it by about ${needed.toFixed(2)} would close the gap on its own.`
}
