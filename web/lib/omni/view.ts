/**
 * The rendering rules, ported from `scema_policy::render`.
 *
 * Third implementation of one rule, and the last one: Rust (`scema_policy::render::cell`),
 * the extension HUD (`plugins/scema-web/src/content.js`), and here. Each is tested, and
 * `npm run check:omni` pins this one.
 *
 * > **An unmeasured term renders as `—`, never `0.00`.**
 *
 * The duplication is deliberate rather than sloppy — three runtimes cannot share a function
 * — but the *rule* is shared, and a copy that drifts is worse than no copy. Anything
 * formatting a term outside this file is a bug.
 */

import type { Abstention, Coverage, Term } from './types.ts'

/** Format a term for a matrix cell. */
export function cell(term: Term | null | undefined): string {
  if (!term) return '?'
  return term.measured ? term.value.toFixed(2) : '—'
}

/** `2/5`. Never shown apart from the score it qualifies. */
export function coverageLabel(c: Coverage | null | undefined): string {
  return c ? `${c.measured}/${c.total}` : '?'
}

export function coverageFraction(c: Coverage | null | undefined): number {
  if (!c || c.total === 0) return 0
  return c.measured / c.total
}

/**
 * Mirror of `Abstention::headline` in Rust.
 *
 * Each reason is a different instruction to the reader, which is the whole reason the enum
 * has five arms instead of a boolean.
 */
export function abstentionHeadline(a: Abstention | null | undefined): string {
  if (!a) return ''
  switch (a.reason) {
    case 'no_candidates':
      return 'no hypotheses were proposed'
    case 'all_forbidden':
      return `all ${a.count} branch(es) violate a constraint on the goal`
    case 'no_positive_utility':
      return `the best branch scores ${a.best.toFixed(3)}; acting is worse than not acting`
    case 'too_little_measured':
      return `the ranking stands on ${coverageLabel(a.coverage)} measured term(s) (${(
        coverageFraction(a.coverage) * 100
      ).toFixed(0)}% < ${(a.floor * 100).toFixed(0)}% floor)`
    case 'contested':
      return `\`${a.by}\` is qualified here and scores the top branch ${a.utility.toFixed(3)}`
    default:
      return (a as { reason: string }).reason
  }
}

/** What the reader should do about each abstention reason. */
export function abstentionAdvice(a: Abstention | null | undefined): string {
  if (!a) return ''
  switch (a.reason) {
    case 'no_candidates':
      return 'Nothing was proposed — check the hypothesisers, or the world had no counted signals.'
    case 'all_forbidden':
      return 'The goal is unsatisfiable as stated. Relax a constraint or change the goal.'
    case 'no_positive_utility':
      return 'Accept that, or lower the bar deliberately. This is the common and usually correct outcome.'
    case 'too_little_measured':
      return 'This is a statement about how little was observed, not about the branches. Observe more.'
    case 'contested':
      return 'A specialist that understands this problem disagrees with the top branch.'
    default:
      return ''
  }
}

/** Tone for a verification verdict. One place decides, as in `lib/mesh/view.ts::toneFor`. */
export type Tone = 'valid' | 'invalid' | 'unknown'

export function verdictTone(valid: boolean | null | undefined): Tone {
  if (valid === null || valid === undefined) return 'unknown'
  return valid ? 'valid' : 'invalid'
}

/**
 * Provenance label for an object in a world.
 *
 * `absent` is not `0` and must not render as one — see `scema_world::provenance`.
 */
export function provenanceLabel(p: { kind: string } | null | undefined): string {
  if (!p) return 'UNKNOWN'
  return p.kind.toUpperCase()
}

/** Is this object's value safe to act on? Stale is deliberately not. */
export function isActionable(p: { kind: string } | null | undefined): boolean {
  return p?.kind === 'live'
}

export function truncate(s: string, n: number): string {
  return s.length <= n ? s : `${s.slice(0, n - 1)}…`
}
