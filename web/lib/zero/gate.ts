// The coherence gate — Zero's epistemic breaker.
//
// A port of `crates/scematica-sniper/src/coherence.rs` in intent rather than in code:
// every other breaker in this system fires on money, and therefore after the damage.
// This one fires on the condition that precedes it — the pipeline passing pools it
// could not actually verify.
//
// Zero needs it MORE than the Rust bot does. RPC-bound filters fail open on timeout,
// and in a browser against a shared or rate-limited endpoint a timeout is an ordinary
// event rather than an outage. Past some fraction of unresolved checks, the safety
// checks the operator believes are running are silently not running, and Zero is a
// pass-through that still reports "passed".
//
// Two rules carried over verbatim, both load-bearing:
//
//   • **Buys only.** A degraded feed must never stop you closing existing risk. The
//     gate returns a verdict about ENTRIES; `exits.ts` never consults it.
//   • **It needs samples before it may trip.** A gate that fires at startup, when it
//     knows least, teaches the operator to disable it.

import { type Term, absent, measured } from './types.ts'

/** A rolling window, not a monotonic counter — the question is "lately", not "ever". */
const WINDOW = 64

export interface CoherenceState {
  /**
   * true = the read resolved, false = it failed open. Newest last.
   *
   * Named `samples` rather than `window`: an identifier called `window` shadows the
   * browser global inside a module that is supposed to have no browser in it, and the
   * purity scan in `check:zero` correctly flagged it. A core that must be hostable by a
   * page and an extension offscreen document alike should not contain the word at all.
   */
  samples: boolean[]
}

export const newCoherence = (): CoherenceState => ({ samples: [] })

export function record(state: CoherenceState, resolved: boolean): CoherenceState {
  const samples = [...state.samples, resolved]
  if (samples.length > WINDOW) samples.splice(0, samples.length - WINDOW)
  return { samples }
}

export interface CoherenceVerdict {
  /**
   * Ψ — the share of RPC-bound checks that actually resolved.
   *
   * UNMEASURED below `minSamples`, and that is the whole point of the type: a Ψ of 0.0
   * and "nobody has measured Ψ yet" are different claims, and only one of them is a
   * reason to stop trading. Rendering both as 0.00 is the failure this codebase has hit
   * three separate times.
   */
  psi: Term
  /** Whether entries may proceed. `true` when Ψ is unmeasured — see below. */
  entriesAllowed: boolean
  /** How many samples the verdict rests on. Never separated from Ψ. */
  samples: number
  reason: string
}

/**
 * `entriesAllowed` is TRUE while Ψ is unmeasured, and that is deliberate.
 *
 * The literal reading — "an unmeasured gate is a closed gate" — pins Zero shut on a
 * fresh page load, forever, because a gate that blocks every read also prevents the
 * samples that would open it. The same trap jammed the sentience Ψ at 0 and the agentic
 * gate shut on subsystems nobody had built. An unmeasured dimension takes the neutral
 * element; only MEASURED degradation moves the verdict.
 */
export function evaluate(
  state: CoherenceState,
  minSamples: number,
  minPsi: number,
): CoherenceVerdict {
  const samples = state.samples.length
  if (samples < minSamples) {
    return {
      psi: absent(`only ${samples} of ${minSamples} samples`),
      entriesAllowed: true,
      samples,
      reason: `coherence not yet measurable (${samples}/${minSamples} reads)`,
    }
  }

  const resolved = state.samples.filter(Boolean).length
  const psi = resolved / samples
  const ok = psi >= minPsi

  return {
    psi: measured(psi),
    entriesAllowed: ok,
    samples,
    reason: ok
      ? `${resolved}/${samples} reads resolved`
      : `only ${resolved}/${samples} reads resolved — entries halted, exits unaffected`,
  }
}

// ── socket liveness ──────────────────────────────────────────────────────────

/**
 * Zero's other blindness, and the one specific to running in a page.
 *
 * A hidden tab throttles timers to roughly once a minute, so a position whose vault has
 * gone quiet has not satisfied its exit conditions — it has *unevaluated* ones. That is
 * not the same as "hold", and Zero must not let it look like one.
 *
 * `staleAfterSecs` is generous on purpose: a genuinely idle pool produces no vault
 * changes either, and crying degraded on a quiet market would train the operator to
 * ignore the badge.
 */
export type Liveness = 'live' | 'quiet' | 'closed'

export interface LivenessVerdict {
  state: Liveness
  secsSinceArrival: Term
  /** True only while Zero can honestly claim its exits are being evaluated. */
  exitsEvaluable: boolean
  reason: string
}

export function liveness(
  socketOpen: boolean,
  lastArrivalUnix: number | null,
  nowUnix: number,
  staleAfterSecs = 90,
): LivenessVerdict {
  if (!socketOpen) {
    return {
      state: 'closed',
      secsSinceArrival: absent('socket closed'),
      exitsEvaluable: false,
      reason: 'socket closed — exits are NOT being evaluated',
    }
  }
  if (lastArrivalUnix === null) {
    return {
      state: 'quiet',
      secsSinceArrival: absent('nothing has arrived yet'),
      exitsEvaluable: false,
      reason: 'connected, but nothing has arrived yet',
    }
  }
  const age = nowUnix - lastArrivalUnix
  if (age > staleAfterSecs) {
    return {
      state: 'quiet',
      secsSinceArrival: measured(age),
      exitsEvaluable: false,
      reason: `no chain event for ${Math.round(age)}s — exits are NOT being evaluated`,
    }
  }
  return {
    state: 'live',
    secsSinceArrival: measured(age),
    exitsEvaluable: true,
    reason: `last event ${Math.round(age)}s ago`,
  }
}
