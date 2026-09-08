// Position sizing — fractional Kelly, clamped by everything that can say no.
//
// A port of `crates/scematica-sniper/src/kelly.rs` in intent: size from a rolling
// win-rate rather than a fixed fraction, take a fraction of the Kelly optimum because
// full Kelly is correct only if the edge estimate is, and clamp hard at both ends.
//
// The clamps are the important part here. In the Rust bot the wallet is the last line;
// in Zero the session key's caps are, and they are enforced in `session.ts` *after* this
// runs. So this function's job is to produce a number that is honest about the edge, and
// the ledger's job is to refuse it if it cannot be afforded. Two gates, different
// questions — the same split as `Workspace` (where) and `TrustPolicy` (whether).

import { type Term, absent, measured } from './types.ts'

/**
 * Kelly fraction. Half-Kelly, because the edge estimate here is a rolling window over a
 * handful of trades and full Kelly on a mis-estimated edge is a reliable way to go broke
 * while being technically optimal.
 */
export const KELLY_FRACTION = 0.5

/** Below this many settled trades, there is no win rate — only a small number of trades. */
export const MIN_TRADES_FOR_KELLY = 8

export interface TradeOutcome {
  /** Realised, settled. A mark is not an outcome. */
  pnlPct: number
}

export interface EdgeEstimate {
  /** Absent until there are enough settled trades to speak of a rate at all. */
  winRate: Term
  avgWinPct: Term
  avgLossPct: Term
  samples: number
}

/**
 * Estimate the edge from settled trades.
 *
 * Every field is absent rather than zero when it cannot be computed. A win rate of 0.0 is
 * a claim that nothing has ever won; "we have three trades" is not that claim, and the
 * difference decides whether Zero sizes up or refuses to trade.
 */
export function estimateEdge(history: TradeOutcome[]): EdgeEstimate {
  const samples = history.length
  if (samples < MIN_TRADES_FOR_KELLY) {
    const why = `only ${samples} of ${MIN_TRADES_FOR_KELLY} settled trades`
    return { winRate: absent(why), avgWinPct: absent(why), avgLossPct: absent(why), samples }
  }

  const wins = history.filter(t => t.pnlPct > 0)
  const losses = history.filter(t => t.pnlPct <= 0)

  const avgWin = wins.length ? wins.reduce((s, t) => s + t.pnlPct, 0) / wins.length : null
  const avgLoss = losses.length
    ? Math.abs(losses.reduce((s, t) => s + t.pnlPct, 0) / losses.length)
    : null

  return {
    winRate: measured(wins.length / samples),
    // No wins yet is a MEASURED fact about this window, not an unmeasured one — but there
    // is no average of an empty set, so the average itself is absent.
    avgWinPct: avgWin === null ? absent('no winning trades in the window') : measured(avgWin),
    avgLossPct: avgLoss === null ? absent('no losing trades in the window') : measured(avgLoss),
    samples,
  }
}

export interface SizeDecision {
  lamports: number
  /** Every clamp that bit, in the order applied. The reason a number is what it is. */
  applied: string[]
  reason: string
}

/**
 * Size an entry.
 *
 * `baseLamports` is the operator's own per-trade intent; Kelly and conviction scale it,
 * and the caps clamp it. When the edge is unmeasured Zero uses a DEFENSIVE fraction
 * rather than the full base — an unknown edge is not a good edge, and this is the one
 * place where "we do not know" has to become a number because a trade needs a size.
 * Naming it here, rather than defaulting silently, is the whole point.
 */
export const UNKNOWN_EDGE_FRACTION = 0.5

export function sizeEntry(
  baseLamports: number,
  edge: EdgeEstimate,
  conviction: number,
  policyMultiplier: number,
  maxPerTradeLamports: number,
  remainingBudgetLamports: number,
  dustLamports: number,
): SizeDecision {
  const applied: string[] = []
  let lamports = baseLamports

  if (!edge.winRate.measured || !edge.avgWinPct.measured || !edge.avgLossPct.measured) {
    lamports *= UNKNOWN_EDGE_FRACTION
    applied.push(`edge unmeasured (${edge.winRate.note ?? 'unknown'}) → ×${UNKNOWN_EDGE_FRACTION}`)
  } else {
    // b = payoff ratio; f* = (b·p − q) / b
    const b = edge.avgWinPct.value / Math.max(1e-9, edge.avgLossPct.value)
    const p = edge.winRate.value
    const f = (b * p - (1 - p)) / b
    const kelly = Math.max(0, f) * KELLY_FRACTION
    if (kelly <= 0) {
      return {
        lamports: 0,
        applied: [`Kelly is non-positive (win rate ${(p * 100).toFixed(0)}%, payoff ${b.toFixed(2)})`],
        reason: 'the measured edge does not support a position',
      }
    }
    lamports *= Math.min(1, kelly / KELLY_FRACTION) // normalise so half-Kelly = full base
    applied.push(`half-Kelly ${(kelly * 100).toFixed(1)}% (win ${(p * 100).toFixed(0)}%, payoff ${b.toFixed(2)})`)
  }

  if (conviction !== 1) {
    lamports *= conviction
    applied.push(`strategy conviction ×${conviction.toFixed(2)}`)
  }
  if (policyMultiplier !== 1) {
    lamports *= policyMultiplier
    applied.push(`policy ×${policyMultiplier.toFixed(2)}`)
  }

  if (lamports > maxPerTradeLamports) {
    lamports = maxPerTradeLamports
    applied.push(`clamped to the per-trade cap`)
  }
  if (lamports > remainingBudgetLamports) {
    lamports = remainingBudgetLamports
    applied.push(`clamped to the remaining budget`)
  }

  lamports = Math.floor(lamports)

  if (lamports < dustLamports) {
    return {
      lamports: 0,
      applied,
      reason: `${lamports} lamports is below the dust floor ${dustLamports} — the fee would dominate`,
    }
  }

  return { lamports, applied, reason: applied.join('; ') || 'base size, no adjustment' }
}
