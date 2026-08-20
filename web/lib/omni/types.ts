/**
 * The decision-record shape, mirroring `scematica-omni/crates/scema-verify`.
 *
 * **Rust is authoritative.** A field added there must be added here in the same shape, and
 * the tripwire is `npm run check:omni` — it re-derives the commitment from a real record, so
 * a field this file does not know about still hashes correctly (the verifier works on the
 * raw text, not on these types) while anything that *renders* it will simply be missing.
 * That asymmetry is intentional: verification must never depend on this file being current.
 */

/** One number, plus whether anybody measured it. The honesty mechanism. */
export interface Term {
  symbol: string
  name: string
  value: number
  measured: boolean
  note: string
}

/** How much of an aggregate stood on real observations. Never shown apart from the score. */
export interface Coverage {
  measured: number
  total: number
}

/** Where an observation came from. `absent` carries no value — it is unseen, not zero. */
export type Provenance =
  | { kind: 'live'; age_secs: number }
  | { kind: 'stale'; age_secs: number; budget_secs: number }
  | { kind: 'absent' }
  | { kind: 'simulated' }

export type Scalar =
  | { t: 'int'; v: number }
  | { t: 'num'; v: number }
  | { t: 'text'; v: string }
  | { t: 'bool'; v: boolean }

export interface WorldObject {
  id: string
  kind: string
  label: string
  attrs: Record<string, Scalar>
  provenance: Provenance
}

export interface Signal {
  id: string
  polarity: 'risk' | 'opportunity'
  label: string
  detail: string
  magnitude: number
  /** A guessed magnitude must never move a score as if it had been counted. */
  measured: boolean
  targets: string[]
  evidence: string[]
}

export interface Extent {
  observed: number
  /** `null` means the observer does not know the denominator — not that it saw everything. */
  total: number | null
  note: string
}

export interface WorldState {
  observer: string
  entity: { kind: string; locator: string; label: string }
  domain: 'software' | 'infrastructure' | 'trading' | 'unknown'
  observed_at: number
  objects: WorldObject[]
  facts: unknown[]
  signals: Signal[]
  extent: Extent
  /** Things the observer tried to read and could not. An input, not a log. */
  blind_spots: string[]
}

export interface Goal {
  id: string
  statement: string
  constraints: Array<{ kind: string; subject: string; detail: string }>
  horizon: string | null
  /** Signal ids the operator asserted this goal addresses. Never inferred. */
  grounded_in: string[]
}

export interface Hypothesis {
  id: string
  statement: string
  rationale: string
  origin: { kind: string; rule?: string; name?: string; record?: string }
  actions: Array<{
    id: string
    verb: string
    target: string
    detail: string
    reversibility: string
  }>
  grounded_in: string[]
  tags: Record<string, string>
}

export interface FailureMode {
  label: string
  detail: string
  likelihood: Term
}

export interface Projection {
  hypothesis: string
  simulator: string
  expected_gain: Term
  risk: Term
  cost: Term
  uncertainty: Term
  reversibility: Term
  failure_modes: FailureMode[]
  shadow: {
    touched_objects: string[]
    addresses_signals: string[]
    unaddressed_risks: string[]
  }
  /** Set when a constraint removed this branch. It stays in the record regardless. */
  forbidden_by: string | null
  coverage: Coverage
}

export interface Contribution {
  symbol: string
  effect: number
  measured: boolean
  note: string
}

export interface Utility {
  value: number
  contributions: Contribution[]
  coverage: Coverage
}

export interface Ranked {
  hypothesis: string
  statement: string
  utility: Utility
  evaluations: Array<{
    evaluator: string
    utility: Term
    confidence: Term
    note: string
  }>
}

export type Applicability =
  | { kind: 'applicable'; note: string }
  | { kind: 'out_of_domain'; note: string }
  | { kind: 'insufficient'; note: string }

/** Five distinct reasons, each a different instruction to the reader. */
export type Abstention =
  | { reason: 'no_candidates' }
  | { reason: 'all_forbidden'; count: number }
  | { reason: 'no_positive_utility'; best: number }
  | { reason: 'too_little_measured'; coverage: Coverage; floor: number }
  | { reason: 'contested'; by: string; utility: number; note: string }

export interface Decision {
  chosen: string | null
  ranked: Ranked[]
  excluded: Array<{ hypothesis: string; statement: string; reason: string }>
  abstention: Abstention | null
  config: {
    weights: { risk: number; cost: number; uncertainty: number; reversibility: number }
    min_coverage: number
    veto_at_or_below: number
  }
  evaluator_status: Array<{ evaluator: string; about: string; applicability: Applicability }>
  coverage: Coverage
}

export interface Commitment {
  world: string
  goal: string
  hypotheses: string
  projections: string
  policy: string
  decision: string
  root: string
}

export interface DecisionRecord {
  id: string
  at: number
  runtime: string
  world: WorldState
  goal: Goal
  hypotheses: Hypothesis[]
  projections: Projection[]
  decision: Decision
  commitment: Commitment
}

/** A loose runtime check before rendering. Verification does not depend on this passing. */
export function looksLikeRecord(v: unknown): v is DecisionRecord {
  if (!v || typeof v !== 'object') return false
  const r = v as Record<string, unknown>
  return (
    typeof r.id === 'string' &&
    typeof r.runtime === 'string' &&
    typeof r.world === 'object' &&
    typeof r.decision === 'object' &&
    typeof r.commitment === 'object' &&
    Array.isArray(r.projections)
  )
}
