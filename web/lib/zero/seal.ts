// Sealing a decision — including the ones where Zero declined.
//
// This is what makes Zero the sixth `scema.world/1` producer and the first one that can
// act: the decision record and the money are the same event. The commitment is computed
// with `lib/omni/canonical.ts`, which is pinned byte-for-byte against Rust by
// `check:omni`, so a record sealed here verifies in `/omni` — in the reader's own
// browser, with no server in the path and nobody to trust.
//
// ── Why the declines are sealed too ──────────────────────────────────────────
//
// A branch nobody took has no outcome and never will. That asymmetry is not a gap in the
// record, it is the reason the record is worth keeping: `calibration.rs` found the bot's
// DQ* emitting `SELL_PARTIAL` on 399 of 399 pieces of advice with a mean absolute error
// of 0.0000 on the 16 that resolved — which is exactly what "always say bearish" earns in
// a losing window. A policy that is only ever scored on the trades it took can improve
// its score by taking fewer. Sealing the declines is what makes that visible.
//
// So an unresolved decision's error is `null`, never `0.0`, and the count of them is
// reported beside anything computed from the rest.

import { canonicalBytes, rootBytes, toHex, type CanonValue } from '../omni/canonical.ts'
import { type DecisionRecord, type ObservedPool, type Term } from './types.ts'

/** SHA-256 via WebCrypto. Present in every browser Zero supports and in Node 20+. */
async function sha256(bytes: Uint8Array): Promise<Uint8Array> {
  const buf = await crypto.subtle.digest('SHA-256', bytes as unknown as ArrayBuffer)
  return new Uint8Array(buf)
}

// ── canonical constructors ───────────────────────────────────────────────────
//
// `CanonValue` is a TAGGED union, and the tag is load-bearing: the encoding separates an
// integer zero from a float zero, so `0` and `0.0` produce different digests. That is not
// a quirk to work around — it is the property that lets a commitment bind a measured 0.00
// distinctly from an integer count that happens to be 0. Every field below therefore
// declares which it is, rather than letting a JS `number` pick for it.

const cNull: CanonValue = { t: 'null' }
const cBool = (v: boolean): CanonValue => ({ t: 'bool', v })
const cInt = (v: number): CanonValue => ({ t: 'int', v: BigInt(Math.trunc(v)) })
const cFloat = (v: number): CanonValue => ({ t: 'float', v })
const cStr = (v: string): CanonValue => ({ t: 'string', v })
const cArr = (v: CanonValue[]): CanonValue => ({ t: 'array', v })
/** Key order is irrelevant to the digest — `canonicalBytes` sorts byte-wise. */
const cObj = (v: Array<[string, CanonValue]>): CanonValue => ({ t: 'object', v })

/**
 * A `Term` as canonical data.
 *
 * The `measured` flag is committed, not just the number. A record that hashed only the
 * value would let an edit flip a measured 0.0 into an unmeasured one — or the reverse —
 * without moving the digest, and that flip is precisely the distinction the whole system
 * is built on.
 *
 * The value is always a FLOAT tag, including when it is unmeasured. Emitting an int for
 * the unmeasured placeholder would make an unmeasured term and a measured integer zero
 * hash alike on the one field where they must not.
 */
function termValue(t: Term): CanonValue {
  const fields: Array<[string, CanonValue]> = [
    ['value', cFloat(t.measured ? t.value : 0)],
    ['measured', cBool(t.measured)],
  ]
  if (t.note !== undefined) fields.push(['note', cStr(t.note)])
  return cObj(fields)
}

function poolValue(p: ObservedPool): CanonValue {
  return cObj([
    ['mint', cStr(p.mint)],
    ['symbol', cStr(p.symbol)],
    ['dev', cStr(p.dev)],
    ['createdAtUnix', cInt(p.createdAtUnix)],
    ['sizeSol', termValue(p.sizeSol)],
    ['ageSecs', termValue(p.ageSecs)],
    ['holderCount', termValue(p.holderCount)],
    ['mintRenounced', termValue(p.mintRenounced)],
    ['freezeDisabled', termValue(p.freezeDisabled)],
    ['devHoldingPct', termValue(p.devHoldingPct)],
    ['buyPressure', termValue(p.buyPressure)],
    ['lpBurned', termValue(p.lpBurned)],
  ])
}

/**
 * The fields bound by the commitment, in a fixed list.
 *
 * A field added to `DecisionRecord` and not added here is committed by nothing, so an
 * edit to it is undetectable. `check:zero` asserts the two agree, the same way
 * `COMMITTED_FIELDS` is pinned for omni records.
 */
export const COMMITTED_FIELDS = [
  'schema',
  'atUnix',
  'mint',
  'act',
  'decline',
  'reason',
  'score',
  'psi',
  'coverage',
  'sizeLamports',
  'q',
  'world',
  'policyId',
] as const

function fieldValue(r: DecisionRecord, field: (typeof COMMITTED_FIELDS)[number]): CanonValue {
  switch (field) {
    case 'schema': return cStr(r.schema)
    case 'atUnix': return cInt(r.atUnix)
    case 'mint': return cStr(r.mint)
    case 'act': return cBool(r.act)
    // An absent decline is committed as an explicit null rather than omitted: omitting it
    // would make "acted" and "declined for a reason that was later deleted" hash alike.
    case 'decline': return r.decline === undefined ? cNull : cStr(r.decline)
    case 'reason': return cStr(r.reason)
    case 'score': return termValue(r.score)
    case 'psi': return termValue(r.psi)
    // Counts, so INT — a coverage of 0/0 must not hash like a measured 0.0 anywhere.
    case 'coverage':
      return cObj([
        ['measuredCount', cInt(r.coverage.measuredCount)],
        ['total', cInt(r.coverage.total)],
      ])
    case 'sizeLamports': return cInt(r.sizeLamports)
    // Q-values are floats. An absent Q-vector is null, not an empty array: "the policy
    // was not consulted" and "the policy returned nothing" are different facts.
    case 'q': return r.q ? cArr(r.q.map(cFloat)) : cNull
    case 'world': return poolValue(r.world)
    case 'policyId': return cStr(r.policyId)
  }
}

export interface SealedDecision {
  record: DecisionRecord
  /** Hex SHA-256 over the canonical encoding of the committed fields. */
  commitment: string
}

/**
 * Seal a record.
 *
 * Note what is NOT committed: `id`. The id is derived from the commitment, so committing
 * it would be circular. Everything else in the record is bound.
 */
export async function seal(record: Omit<DecisionRecord, 'id'>): Promise<SealedDecision> {
  const full = { ...record, id: '' } as DecisionRecord
  const parts: Array<[string, Uint8Array]> = []
  for (const field of COMMITTED_FIELDS) {
    parts.push([field, await sha256(canonicalBytes(fieldValue(full, field)))])
  }
  const commitment = toHex(await sha256(rootBytes(parts)))
  return { record: { ...full, id: commitment.slice(0, 16) }, commitment }
}

/**
 * Re-derive a commitment to check a record was not edited after sealing.
 *
 * What this proves and does not, stated here because it is stated on `/omni` too and the
 * two must not drift: it proves the record has not been edited. It does **not** prove the
 * world was as described — provenance carries that — and it does **not** prove this is the
 * original record, only that it is internally consistent. Tamper-evident, not
 * tamper-proof, until a root is anchored somewhere Zero's operator does not control.
 */
export async function verify(record: DecisionRecord, commitment: string): Promise<boolean> {
  const { commitment: recomputed } = await seal({ ...record })
  return recomputed === commitment
}

// ── calibration ──────────────────────────────────────────────────────────────

export interface CalibrationLine {
  /** Decisions where Zero acted and the position settled. */
  resolved: number
  /** Decisions where Zero acted and the outcome is not known yet, or never will be. */
  unresolved: number
  /** Decisions where Zero declined. These NEVER resolve, by construction. */
  declined: number
  /** `null` when nothing resolved — never 0, which reads as perfect calibration. */
  meanAbsErrorPct: number | null
  /** True when every acted decision took the same branch: the score is a base rate. */
  actionNeverVaried: boolean
  verdict: string
}

/**
 * Score the policy against what actually happened.
 *
 * Three rules, and the first is what makes this honest: **a decline has no outcome and
 * never will**, so it is counted rather than scored. Imputing one would mean the system
 * generating its own training signal, and a conservative policy would improve its score
 * every time it refused to act.
 *
 * The third rule is the one that caught a real defect: a score a CONSTANT policy earns is
 * the base rate, not skill. If every acted decision took the same branch, the number is
 * reported with that caveat attached rather than on its own.
 */
export function calibrate(
  decisions: Array<{ act: boolean; sizeLamports: number }>,
  outcomes: Array<{ predictedPct: number; realisedPct: number } | null>,
): CalibrationLine {
  const acted = decisions.filter(d => d.act)
  const declined = decisions.length - acted.length
  const settled = outcomes.filter((o): o is { predictedPct: number; realisedPct: number } => o !== null)

  const actionNeverVaried =
    acted.length > 1 && new Set(acted.map(a => a.sizeLamports)).size === 1

  if (settled.length === 0) {
    return {
      resolved: 0,
      unresolved: acted.length,
      declined,
      meanAbsErrorPct: null,
      actionNeverVaried,
      verdict:
        acted.length === 0
          ? `no decision was acted on; ${declined} declined and a decline never resolves`
          : `${acted.length} acted, none settled yet — nothing to score`,
    }
  }

  const mae =
    settled.reduce((s, o) => s + Math.abs(o.predictedPct - o.realisedPct), 0) / settled.length

  return {
    resolved: settled.length,
    unresolved: acted.length - settled.length,
    declined,
    meanAbsErrorPct: mae,
    actionNeverVaried,
    verdict: actionNeverVaried
      ? `MAE ${mae.toFixed(2)}% over ${settled.length} — but every entry was the same size, so this is a base rate, not skill`
      : `MAE ${mae.toFixed(2)}% over ${settled.length} settled; ${declined} declined and uncounted`,
  }
}
