// Wire shapes for GET /api/mesh.
//
// **`crates/scematica-mesh` is authoritative.** These are a transcription of the serde
// output, not a second definition. Field names are snake_case because the Rust structs
// carry no `rename_all`, and the `Provenance` union is internally tagged on `kind`.
//
// If a field is added in Rust and not here, TypeScript will not notice — it will simply be
// dropped. The tripwire is `scripts/check-mesh.mjs`, which asserts the discriminants and
// the layer table are exhaustive; anything structural beyond that is caught by the shape
// of the render failing visibly rather than silently.

/** Internally-tagged union: `{ kind: 'live', age_secs }`, `{ kind: 'absent' }`, … */
export type Provenance =
  | { kind: 'live'; age_secs: number }
  | { kind: 'stale'; age_secs: number; budget_secs: number }
  | { kind: 'absent' }
  | { kind: 'simulated' }

export type NodeKind =
  | 'listener'
  | 'filter'
  | 'scorer'
  | 'breaker'
  | 'learner'
  | 'reasoner'
  | 'gate'
  | 'executor'
  | 'peer'
  | 'agent'

export type Verdict = 'pass' | 'veto' | 'damp' | 'degraded' | 'idle' | 'unknown'

export type EdgeKind = 'signal' | 'veto' | 'gate' | 'promotion' | 'experience'

export interface MeshNode {
  id: string
  kind: NodeKind
  label: string
  blurb: string
  provenance: Provenance
  verdict: Verdict
  /** 0..1, or null when not measurable. **Null is not zero** — do not render it as an
   *  empty bar, which reads as "measured, and it is nothing". */
  activity: number | null
  /** Ordered [key, value] pairs, rendered verbatim. */
  detail: [string, string][]
  reason: string | null
}

export interface MeshEdge {
  from: string
  to: string
  kind: EdgeKind
  /** `null` means the endpoints could not be read. Must never render as `false`. */
  active: boolean | null
  label: string | null
}

export interface MeshSummary {
  nodes_total: number
  nodes_live: number
  nodes_stale: number
  nodes_absent: number
  nodes_simulated: number
  /** 0..1 fraction of nodes with usable current data. */
  visibility: number
  /** Veto edges blocking now, from a source that is currently readable. */
  blocking: number
  /** Veto edges that were active as of a stale reading. */
  blocking_stale: number
  diagnosis: string
}

/** One term entering an equation, carrying whether it was actually measured.
 *
 *  `measured: false` means the term contributed its NEUTRAL element, not a guess and not
 *  a zero-with-confidence. Any UI that hides this flag turns the gate into a number with
 *  no evidence behind it, which is the failure mode the Rust module is built around. */
export interface Term {
  symbol: string
  /** Spec section, e.g. `§16`. */
  section: string
  name: string
  value: number
  measured: boolean
  note: string
}

export interface Uncertainty {
  aleatoric: Term
  epistemic: Term
  total: number
}

export interface RiskField {
  components: Term[]
  value: number
}

export interface Coherence {
  value: number
  subsystems: number
  disagreement: number
  /** The spec's continuous D(Yᵢ,Ȳ) is approximated by a discrete majority measure. */
  approximation: boolean
  note: string
}

/** §22. `unevaluated` is NOT `abstain` — one is ignorance, the other is a decision. */
export type GateVerdict = 'act' | 'damp' | 'abstain' | 'unevaluated'

export interface Cognition {
  confidence: number
  confidence_terms: Term[]
  uncertainty: Uncertainty
  risk: RiskField
  coherence: Coherence
  /** §32 — Ψ = C · K · (1 − R). */
  psi: number
  verdict: GateVerdict
  /** §33 — null until at least one of its five subsystems exists. */
  omega: number | null
  omega_terms: Term[]
  /** Fraction of terms actually measured. Must be rendered beside Ψ, always. */
  measured_fraction: number
  reading: string
}

export interface Mesh {
  nodes: MeshNode[]
  edges: MeshEdge[]
  generated_at: string
  summary: MeshSummary
  cognition: Cognition
}

/** The 503 the web layer returns when no bot is paired. There is deliberately no
 *  simulated mesh — see the `case 'mesh'` comment in `app/api/[...slug]/route.ts`. */
export interface MeshUnavailable {
  error: string
  hint?: string
}

export function isMesh(v: unknown): v is Mesh {
  if (!v || typeof v !== 'object') return false
  const m = v as Partial<Mesh>
  return Array.isArray(m.nodes) && Array.isArray(m.edges) && typeof m.summary === 'object'
}
