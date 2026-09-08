// What Zero shows about itself.
//
// An honest readout, not decoration — the same rule `lib/scylar/sigil.ts` and
// `lib/mesh/view.ts` are built on, and the one that shapes this whole file:
//
//   **An unmeasured gauge must not look like a measured zero.**
//
// A Ψ of 0.00 and "nobody has measured Ψ yet" are different claims. One says every read
// is failing; the other says Zero just started. Rendering both as an empty bar is how an
// operator learns to ignore the bar.
//
// This module is pure and returns a *description* of what to draw. It picks no colours
// and places no pixels — the component does that, and `globals.css` owns every hex. A
// renderer names a ROLE, never a colour.

import { type Term, type Coverage, cell } from './types.ts'
import { type CoherenceVerdict, type LivenessVerdict } from './gate.ts'
import { type SessionReadout } from './session.ts'
import { type LeaseState, leaseNote } from './lease.ts'
import { type GateState } from './gatekeep.ts'

/** What a gauge means, never what colour it is. */
export type Role = 'ok' | 'warn' | 'alarm' | 'idle' | 'unmeasured' | 'claim'

export interface Gauge {
  label: string
  /** The formatted value. An em dash when unmeasured — never "0.00". */
  text: string
  /** 0..1 for a measured value; `null` when unmeasured, which draws differently. */
  fill: number | null
  role: Role
  /** Why it reads the way it does. Always present; a gauge without one is decoration. */
  note: string
}

/**
 * A coverage meter is ONE CELL PER TERM, never a proportional bar.
 *
 * A bar renders 2/5 and 4/10 identically, and the denominator is the number that matters.
 * An empty coverage is `∅`, never an empty meter — 0/0 is undefined, not zero percent.
 */
export function coverageMeter(c: Coverage): string {
  if (c.total === 0) return '∅'
  return '▰'.repeat(c.measuredCount) + '▱'.repeat(Math.max(0, c.total - c.measuredCount))
}

function gauge(label: string, t: Term, note: string, role: Role, scale = 1): Gauge {
  return {
    label,
    text: cell(t),
    fill: t.measured ? Math.max(0, Math.min(1, t.value / scale)) : null,
    role: t.measured ? role : 'unmeasured',
    note: t.measured ? note : (t.note ?? 'not measured'),
  }
}

export interface ZeroReadout {
  gauges: Gauge[]
  /** Never separated from Ψ. A gate computed on two samples is a claim about ignorance. */
  coherenceCoverage: string
  /** The single most important line: is Zero actually watching your positions? */
  headline: { text: string; role: Role }
  lease: { text: string; role: Role }
  session: { text: string; role: Role; warning: string }
  gate: { text: string; role: Role }
  notes: string[]
}

/**
 * Build the readout.
 *
 * The headline answers one question — *are my exits being evaluated?* — because that is
 * the only question whose wrong answer costs money silently. Everything else on the page
 * is context for it.
 */
export function buildReadout(
  coherence: CoherenceVerdict,
  liveness: LivenessVerdict,
  session: SessionReadout,
  lease: LeaseState,
  gate: GateState,
  openPositions: number,
  minPsi: number,
): ZeroReadout {
  const gauges: Gauge[] = [
    gauge('Ψ coherence', coherence.psi, coherence.reason, coherence.entriesAllowed ? 'ok' : 'alarm'),
    gauge('feed age (s)', liveness.secsSinceArrival, liveness.reason, liveness.exitsEvaluable ? 'ok' : 'alarm', 120),
    gauge('session left (s)', session.secsUntilExpiry, session.armed ? 'time before the key stops signing' : 'not armed', 'ok', 3600),
    gauge(
      'budget left',
      session.armed
        ? { value: session.remainingLamports / Math.max(1, session.budgetLamports), measured: true }
        : { value: 0, measured: false, note: 'not armed' },
      `${session.remainingLamports} of ${session.budgetLamports} lamports`,
      session.remainingLamports > 0 ? 'ok' : 'warn',
    ),
  ]

  // The headline. Exits first, because an entry Zero misses costs nothing.
  const headline = !liveness.exitsEvaluable && openPositions > 0
    ? {
        text: `${openPositions} position(s) open and NOT being evaluated — ${liveness.reason}`,
        role: 'alarm' as Role,
      }
    : !liveness.exitsEvaluable
      ? { text: `not receiving chain events — ${liveness.reason}`, role: 'warn' as Role }
      : coherence.entriesAllowed
        ? { text: `watching ${openPositions} position(s); entries open`, role: 'ok' as Role }
        : {
            text: `watching ${openPositions} position(s); entries HALTED — Ψ ${cell(coherence.psi)} below ${minPsi}`,
            role: 'warn' as Role,
          }

  const notes: string[] = []
  if (session.strandedCount > 0) {
    notes.push(
      `${session.strandedCount} swap(s) were submitted and never observed. Their budget stays reserved and Zero has not retried — check the signatures before acting by hand.`,
    )
  }
  if (!coherence.psi.measured) {
    notes.push(`Ψ is unmeasured (${coherence.psi.note}). That is not a Ψ of zero — entries are allowed until it can be measured.`)
  }

  return {
    gauges,
    coherenceCoverage: coverageMeter({ measuredCount: coherence.samples, total: Math.max(coherence.samples, 1) }),
    headline,
    lease: {
      text: leaseNote(lease),
      role: lease === 'writer' ? 'ok' : lease === 'unsupported' ? 'alarm' : 'idle',
    },
    session: {
      text: session.armed
        ? `armed — ${session.remainingLamports} lamports of ${session.budgetLamports} left`
        : 'not armed — Zero will not sign anything',
      role: session.armed ? 'claim' : 'idle',
      warning: session.warning,
    },
    gate: {
      text: gate.reason,
      role: gate.verdict === 'open' ? 'ok' : gate.verdict === 'unknown' ? 'warn' : 'idle',
    },
    notes,
  }
}
