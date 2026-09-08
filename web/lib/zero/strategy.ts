// Entry strategies — what Zero is willing to buy, and why each one is latency-tolerant.
//
// Zero loses the first-block race by construction (docs/SCEMATICA-ZERO.md §1), so every
// strategy here has to survive being a second or two late. That is the selection
// criterion, and it is why "snipe the launch" is absent rather than merely deprioritised.
//
// Each strategy is a pure predicate over an `ObservedPool` plus a little history. None of
// them reads a clock directly: they are evaluated when something arrives, and they take
// the arrival's timestamp. See Z-1.

import { type ObservedPool, type Term, type ZeroConfig, absent, measured } from './types.ts'

export type StrategyName = 'scored-entry' | 'pullback' | 'continuation'

export interface StrategySignal {
  strategy: StrategyName
  fires: boolean
  /** Multiplies the base size. A strategy may express conviction, never invent budget. */
  conviction: number
  reason: string
}

/**
 * Price history for one mint, newest last.
 *
 * Bounded and small on purpose: this is a browser tab that may run for hours across
 * dozens of mints, and an unbounded array per mint is how a page becomes unresponsive
 * long after anybody would connect it to the bot.
 */
export const HISTORY_CAP = 64

export interface PriceHistory {
  mint: string
  /** [unixSecs, priceSol] pairs. */
  points: Array<[number, number]>
}

export function pushPrice(h: PriceHistory, atUnix: number, priceSol: number): PriceHistory {
  const points = [...h.points, [atUnix, priceSol] as [number, number]]
  if (points.length > HISTORY_CAP) points.splice(0, points.length - HISTORY_CAP)
  return { mint: h.mint, points }
}

/**
 * Percentage change over the window, or the admission that there is no window.
 *
 * Two prints have no velocity and one print has no change at all. Returning 0 for either
 * claims the price did not move, which is a measurement nobody took — the same rule
 * `alchem_link.omni` applies to a volatility computed from two samples.
 */
export function changePct(h: PriceHistory, overSecs: number, nowUnix: number): Term {
  if (h.points.length < 2) return absent('fewer than two prints')
  const cutoff = nowUnix - overSecs
  const older = h.points.filter(([t]) => t <= cutoff)
  const base = older.length > 0 ? older[older.length - 1][1] : h.points[0][1]
  const last = h.points[h.points.length - 1][1]
  if (!(base > 0)) return absent('no positive reference price')
  return measured(((last - base) / base) * 100)
}

/** Peak price in the history. Absent on an empty history — never 0, which is a price. */
export function peak(h: PriceHistory): Term {
  if (h.points.length === 0) return absent('no prints')
  return measured(Math.max(...h.points.map(([, p]) => p)))
}

// ── the strategies ───────────────────────────────────────────────────────────

/**
 * **Scored entry.** The default: a pool whose ported score clears the floor and whose
 * filters passed. Latency-tolerant because the score is a claim about the pool's
 * structure — depth, authorities, deployer — not about a price that is moving away.
 */
export function scoredEntry(score: Term, config: ZeroConfig): StrategySignal {
  if (!score.measured) {
    return {
      strategy: 'scored-entry',
      fires: false,
      conviction: 0,
      reason: `pool score unmeasured (${score.note ?? 'no reason given'}) — an unscored pool is not a low-scoring one`,
    }
  }
  if (score.value < config.minPoolScore) {
    return {
      strategy: 'scored-entry',
      fires: false,
      conviction: 0,
      reason: `score ${score.value.toFixed(0)} below floor ${config.minPoolScore}`,
    }
  }
  // Conviction rises with the margin over the floor, capped. A score of exactly the floor
  // is a pass, not an endorsement.
  const margin = (score.value - config.minPoolScore) / Math.max(1, 100 - config.minPoolScore)
  return {
    strategy: 'scored-entry',
    fires: true,
    conviction: 1 + Math.min(0.5, margin * 0.5),
    reason: `score ${score.value.toFixed(0)} clears floor ${config.minPoolScore}`,
  }
}

/** How far the price must fall from its peak before `pullback` will look at it. */
export const PULLBACK_MIN_DRAWDOWN_PCT = 20
/** ...and how far is too far. Past this it is not a pullback, it is a decline. */
export const PULLBACK_MAX_DRAWDOWN_PCT = 55
/** The run-up that has to have happened first, or there is no pullback to buy. */
export const PULLBACK_MIN_RUNUP_PCT = 40

/**
 * **Pullback.** A pool that ran up, gave some back, and is still well above where it
 * started. The most latency-tolerant entry available — it is *defined* by waiting.
 *
 * The bounds matter in both directions. Too shallow and every wobble is a signal; too
 * deep and Zero is buying the first leg of a rug, which looks identical to a pullback
 * until it does not stop. `PULLBACK_MAX_DRAWDOWN_PCT` is the line, and it is a guess
 * stated as a constant rather than a threshold hidden in an expression.
 */
export function pullback(h: PriceHistory, nowUnix: number): StrategySignal {
  const no = (reason: string): StrategySignal => ({ strategy: 'pullback', fires: false, conviction: 0, reason })

  const pk = peak(h)
  if (!pk.measured || h.points.length < 4) return no('not enough price history for a pullback')

  const first = h.points[0][1]
  const last = h.points[h.points.length - 1][1]
  if (!(first > 0) || !(pk.value > 0)) return no('no positive reference price')

  const runup = ((pk.value - first) / first) * 100
  const drawdown = ((pk.value - last) / pk.value) * 100

  if (runup < PULLBACK_MIN_RUNUP_PCT) return no(`run-up ${runup.toFixed(0)}% below ${PULLBACK_MIN_RUNUP_PCT}%`)
  if (drawdown < PULLBACK_MIN_DRAWDOWN_PCT) return no(`drawdown ${drawdown.toFixed(0)}% — not a pullback yet`)
  if (drawdown > PULLBACK_MAX_DRAWDOWN_PCT) {
    return no(`drawdown ${drawdown.toFixed(0)}% exceeds ${PULLBACK_MAX_DRAWDOWN_PCT}% — this is a decline, not a pullback`)
  }
  // Still above the start, or the "pullback" has simply retraced the whole move.
  if (last <= first) return no('price is back at or below where the history starts')

  const recent = changePct(h, 60, nowUnix)
  if (recent.measured && recent.value < -PULLBACK_MAX_DRAWDOWN_PCT / 2) {
    return no(`still falling hard (${recent.value.toFixed(0)}% in 60s) — no attempt to catch it`)
  }

  return {
    strategy: 'pullback',
    fires: true,
    conviction: 1,
    reason: `ran ${runup.toFixed(0)}%, gave back ${drawdown.toFixed(0)}%, still above entry`,
  }
}

/** Sustained rise required before `continuation` fires. */
export const CONTINUATION_MIN_PCT = 25
/** Over this window. Long enough that one print cannot manufacture the signal. */
export const CONTINUATION_WINDOW_SECS = 120

/**
 * **Continuation.** A pool rising steadily rather than spiking. Latency-tolerant because
 * a trend measured over two minutes does not evaporate in the second Zero takes to act.
 *
 * Requires the rise to be *monotone-ish* rather than merely net-positive: a mint that
 * doubled and halved and doubled has the same two-minute change as one that climbed
 * steadily, and only the second is a trend. This is the cheap version of that test —
 * a majority of steps up — and it is deliberately cheap, because an elaborate one over
 * 64 points in a browser is a claim about noise.
 */
export function continuation(h: PriceHistory, nowUnix: number): StrategySignal {
  const no = (reason: string): StrategySignal => ({ strategy: 'continuation', fires: false, conviction: 0, reason })

  if (h.points.length < 5) return no('not enough price history for a trend')
  const change = changePct(h, CONTINUATION_WINDOW_SECS, nowUnix)
  if (!change.measured) return no(`no measurable change (${change.note})`)
  if (change.value < CONTINUATION_MIN_PCT) {
    return no(`${change.value.toFixed(0)}% over ${CONTINUATION_WINDOW_SECS}s is below ${CONTINUATION_MIN_PCT}%`)
  }

  let up = 0
  for (let i = 1; i < h.points.length; i++) if (h.points[i][1] > h.points[i - 1][1]) up++
  const steps = h.points.length - 1
  if (up * 2 <= steps) {
    return no(`${up}/${steps} steps up — net rise without a trend`)
  }

  return {
    strategy: 'continuation',
    fires: true,
    conviction: 1.2,
    reason: `+${change.value.toFixed(0)}% over ${CONTINUATION_WINDOW_SECS}s, ${up}/${steps} steps up`,
  }
}

// ── selection ────────────────────────────────────────────────────────────────

export interface StrategyChoice {
  fired: StrategySignal[]
  declined: StrategySignal[]
  /** Highest conviction among those that fired. 0 when none did. */
  conviction: number
  reason: string
}

/**
 * Run every enabled strategy and report all of them.
 *
 * The declines are returned, not discarded: "nothing fired" is useless to an operator,
 * where "score 71 cleared, pullback wanted a 40% run-up and saw 12%" is a description of
 * the market. Same reasoning as `conform` reporting every finding at once rather than
 * bailing on the first.
 */
export function selectStrategies(
  enabled: StrategyName[],
  score: Term,
  history: PriceHistory,
  nowUnix: number,
  config: ZeroConfig,
): StrategyChoice {
  const all: StrategySignal[] = []
  if (enabled.includes('scored-entry')) all.push(scoredEntry(score, config))
  if (enabled.includes('pullback')) all.push(pullback(history, nowUnix))
  if (enabled.includes('continuation')) all.push(continuation(history, nowUnix))

  const fired = all.filter(s => s.fires)
  const declined = all.filter(s => !s.fires)
  const conviction = fired.length ? Math.max(...fired.map(s => s.conviction)) : 0

  return {
    fired,
    declined,
    conviction,
    reason: fired.length
      ? fired.map(s => `${s.strategy}: ${s.reason}`).join('; ')
      : declined.map(s => `${s.strategy}: ${s.reason}`).join('; ') || 'no strategies enabled',
  }
}
