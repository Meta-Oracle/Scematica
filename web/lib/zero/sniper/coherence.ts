// The coherence breaker — a port of `crates/scematica-sniper/src/coherence.rs`.
//
// ⚠️  PORT. Rust is authoritative. `check:zero` pins every case in this file against
// `fixtures/sniper-parity.json`, emitted by the sniper itself.
//
// The epistemic breaker: every other breaker in the bot fires on money, and therefore
// after the damage. This one fires on the condition that precedes it — the pipeline
// passing pools it could not verify. RPC-bound filters fail open on timeout, so a
// degraded node turns the pipeline into a pass-through that still reports "passed".
//
// Zero needs it MORE than the Rust bot does, because in a browser against a shared or
// rate-limited endpoint a timeout is an ordinary event rather than an outage.
//
// ── What changed when this became a real port ────────────────────────────────
//
// The previous version was a reimplementation with its own constants: a 64-SAMPLE window
// (Rust rolls a 120-SECOND one), a `minSamples` and a `minPsi` read from Zero's own
// config (Rust has MIN_SAMPLES = 20 and halts on a HOLD gate, not on a threshold anybody
// configures), no feed-staleness term at all, and Ψ reported as the resolution rate —
// see `psi.ts` for why that last one is not a smaller version of the same number.
//
// Three rules carried over unchanged, because they are the design and not the arithmetic:
//
//   • **Buys only.** A degraded feed must never stop you closing existing risk. The
//     verdict is about ENTRIES; `exit-ladder.ts` never consults it.
//   • **It needs samples before it may trip.** `decisive` is false below MIN_SAMPLES, and
//     an indecisive breaker permits. A gate that fires at startup, when it knows least,
//     is one the operator disables.
//   • **A rolling window, not a monotonic counter.** The question is "is it bad *now*".

import { type Term, absent, measured } from '../types.ts'
import { type Gate, PSI_MAX, gateOf, masterEquation } from './psi.ts'

/** `coherence::WINDOW_SECS`. A hard roll, not a decay — it keeps the arithmetic obvious. */
export const WINDOW_SECS = 120

/**
 * `coherence::MIN_SAMPLES`.
 *
 * A cold start has resolved 0 of 0 checks, which is not evidence of a problem. Tripping
 * on an empty sample would halt the bot the moment it launched.
 */
export const MIN_SAMPLES = 20

/** `coherence::FEED_STALL_SECS`. A feed with no events for this long is stalled. */
export const FEED_STALL_SECS = 180

export interface CoherenceState {
  /** Unix seconds the current window opened. `null` before the first observation. */
  windowStartedUnix: number | null
  resolved: number
  unresolved: number
  /**
   * When the listener last produced a pool. `null` means nothing has arrived yet, which
   * is NOT the same as "arrived at time zero" — `coherence.rs` keeps the same distinction
   * with a sentinel of 0 millis and ages from process start instead.
   */
  lastEventUnix: number | null
  /** When Zero started watching. Feed age is measured from here until an event lands. */
  startedUnix: number
}

export const newCoherence = (nowUnix: number): CoherenceState => ({
  windowStartedUnix: null,
  resolved: 0,
  unresolved: 0,
  lastEventUnix: null,
  startedUnix: nowUnix,
})

/**
 * `CoherenceBreaker::record_check` — did this RPC-bound read come back with real data?
 *
 * In Rust this is called from the two shared retry helpers in `filters.rs` rather than
 * from each filter, so a new filter is instrumented by construction. Zero's equivalent is
 * `host/rpc.ts` reporting every read as a `read.resolved` / `read.failed` event; the same
 * property holds for the same reason.
 */
export function record(
  state: CoherenceState,
  resolved: boolean,
  atUnix: number,
): CoherenceState {
  const started = state.windowStartedUnix
  // `if w.started.elapsed() >= WINDOW { reset }` — the roll happens on write, before the
  // count, so a sample arriving after a long silence opens a new window rather than
  // landing in a stale one.
  const rolled = started === null || atUnix - started >= WINDOW_SECS
  const base = rolled
    ? { windowStartedUnix: atUnix, resolved: 0, unresolved: 0 }
    : { windowStartedUnix: started, resolved: state.resolved, unresolved: state.unresolved }

  return {
    ...state,
    ...base,
    resolved: base.resolved + (resolved ? 1 : 0),
    unresolved: base.unresolved + (resolved ? 0 : 1),
  }
}

/** `CoherenceBreaker::record_pool_seen`. Feed liveness, recorded on arrival. */
export function recordPoolSeen(state: CoherenceState, atUnix: number): CoherenceState {
  return { ...state, lastEventUnix: atUnix }
}

/** `CoherenceBreaker::feed_age_secs`. */
export function feedAgeSecs(state: CoherenceState, nowUnix: number): number {
  // Nothing seen yet: age from when we started, so a socket that never delivers a pool
  // eventually reads as stalled instead of as permanently fresh.
  const from = state.lastEventUnix ?? state.startedUnix
  return Math.max(0, nowUnix - from)
}

export interface Coherence {
  /**
   * Ψ from the master equation — NOT the resolution rate.
   *
   * Unmeasured below MIN_SAMPLES, and the `Term` is what keeps that honest: a Ψ of 0.0
   * and "nobody has measured Ψ yet" are different claims and only one is a reason to stop
   * trading. Rendering both as 0.00 is the failure this codebase has hit three times.
   */
  psi: Term
  gate: Gate
  /** The share of recent RPC-bound checks that returned real data. */
  resolutionRate: Term
  resolved: number
  unresolved: number
  feedAgeSecs: number
  /** False while the sample is too small to judge. An indecisive breaker permits. */
  decisive: boolean
  /** `Coherence::should_halt` — only a HOLD halts. CAUTION is reported, not enforced. */
  shouldHalt: boolean
  /** Whether entries may proceed. Exits never consult this. */
  entriesAllowed: boolean
  reason: string
}

/**
 * `CoherenceBreaker::evaluate` + `Coherence::should_halt` + `Coherence::reason`.
 *
 * `enabled` mirrors the `coherence_breaker` config flag, which defaults to **true** in
 * Rust via `default_true()` — `#[serde(default)]` yields `false` for a missing bool, and
 * silently disabling a safety feature for every existing config is how that default was
 * chosen. Zero keeps the same default for the same reason.
 */
export function evaluate(
  state: CoherenceState,
  nowUnix: number,
  enabled = true,
): Coherence {
  const total = state.resolved + state.unresolved
  // `if total == 0 { 1.0 }` — with nothing observed the rate is not zero, it is unknown,
  // and the neutral element is what an unknown contributes. `decisive` is what actually
  // stops that 1.0 being read as a healthy measurement.
  const rate = total === 0 ? 1.0 : state.resolved / total
  const age = feedAgeSecs(state, nowUnix)

  // `assess`: feed_health = (1 - age/FEED_STALL_SECS).clamp(0,1)
  const feedHealth = Math.min(1, Math.max(0, 1.0 - age / FEED_STALL_SECS))
  const { psi } = masterEquation(feedHealth, rate)
  const gate = gateOf(psi)

  const decisive = enabled && total >= MIN_SAMPLES
  const shouldHalt = decisive && gate === 'HOLD'

  const reason = !enabled
    ? 'coherence breaker is disabled'
    : !decisive
      ? `coherence not yet decisive (${total}/${MIN_SAMPLES} reads in the last ${WINDOW_SECS}s)`
      : age > FEED_STALL_SECS
        ? `pool feed stalled for ${age.toFixed(0)}s`
        : shouldHalt
          ? `only ${(rate * 100).toFixed(0)}% of ${total} filter checks resolved — the pipeline is ` +
            'passing pools it could not verify; entries halted, exits unaffected'
          : `${state.resolved}/${total} reads resolved (Ψ ${psi.toFixed(4)} of ${PSI_MAX.toFixed(4)} max, ${gate})`

  return {
    psi: decisive ? measured(psi) : absent(`only ${total} of ${MIN_SAMPLES} reads`),
    gate,
    resolutionRate: total === 0 ? absent('no reads yet') : measured(rate),
    resolved: state.resolved,
    unresolved: state.unresolved,
    feedAgeSecs: age,
    decisive,
    shouldHalt,
    entriesAllowed: !shouldHalt,
    reason,
  }
}

// ── socket liveness ──────────────────────────────────────────────────────────
//
// Zero's other blindness, and the one with no counterpart in Rust: the sniper polls, so a
// position it is holding is a position it is checking. A browser tab cannot — Chrome
// throttles a hidden tab's timers to roughly once a minute — so a position whose vault
// has gone quiet has not satisfied its exit conditions, it has *unevaluated* ones.
//
// That is a fact about the host, not about the sniper, so it lives here rather than
// pretending to be a port of something. `FEED_STALL_SECS` is reused as the threshold
// because the question is the same one the breaker's feed term asks.

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
  staleAfterSecs = FEED_STALL_SECS,
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
