// The exit ladder, as arrival-driven predicates.
//
// ── Why this file has no timers in it at all ─────────────────────────────────
//
// The Rust sell monitor polls: 20 checks at 100ms, then `price_check_interval_ms`. A
// browser cannot do that. Chrome throttles `setInterval` in a hidden tab to roughly once
// a minute and tightens further after five, and it does not announce any of it — so a
// polled stop-loss does not fail loudly when the operator switches tabs, it *keeps
// holding the position and stops checking it*.
//
// So every rule below is a pure predicate over (position, arriving price, arrival time).
// It is evaluated when a vault notification arrives, because a WebSocket message handler
// is delivered to a hidden tab where a timer is not. Nothing here reads a clock; the
// caller passes the arrival's timestamp.
//
// The time-based rule (`no-pump`) is the interesting case and the one that proves the
// design: it is NOT "fire after 30 seconds". It is "when something arrives, if 30 seconds
// have passed and the price has not moved, exit". A position that has gone quiet has not
// satisfied its exits — it has *unevaluated* ones, which `gate.ts::liveness` reports and
// the UI shows. That distinction is the difference between a bot and a bot-shaped tab.

import { type Position, type Term, type ZeroConfig, absent, measured } from './types.ts'

export type ExitReason =
  | 'take-profit'
  | 'stop-loss'
  | 'pullback'
  | 'no-pump'
  | 'unknown-position'

export interface ExitDecision {
  exit: boolean
  reason?: ExitReason
  /** One line naming the cause, for the record and the UI. */
  detail: string
  /** The PnL the decision was made on. Unmeasured before an entry price exists. */
  pnlPct: Term
}

const hold = (detail: string, pnlPct: Term): ExitDecision => ({ exit: false, detail, pnlPct })

/**
 * Current PnL against the entry.
 *
 * Absent — never zero — when the entry price is unknown. A position whose entry nobody
 * observed has no P&L, and reporting 0% would put it exactly at breakeven, which is both
 * a specific claim and the one least likely to trigger any rule.
 */
export function pnlPct(position: Position, priceSol: number): Term {
  if (!position.entryPriceSol.measured || !(position.entryPriceSol.value > 0)) {
    return absent('entry price was never observed')
  }
  return measured(((priceSol - position.entryPriceSol.value) / position.entryPriceSol.value) * 100)
}

/**
 * Evaluate the whole ladder against one arriving price.
 *
 * Order is deliberate and is the order the Rust monitor uses: **stop-loss first**. A
 * position that is simultaneously past its take-profit and past its stop cannot be — but
 * a position past its stop that a later rule would hold must still be cut, and putting
 * the protective rule first is what guarantees no rule can ever override it.
 */
export function evaluateExit(
  position: Position,
  priceSol: number,
  atUnix: number,
  config: ZeroConfig,
): ExitDecision {
  // A position Zero could not observe opening is not one it may close on a price rule:
  // it does not know the entry, so every percentage below is meaningless. It is surfaced
  // for the operator instead. Closing it blind could sell tokens that were never bought.
  if (position.state === 'unknown') {
    return {
      exit: false,
      reason: 'unknown-position',
      detail: 'fill was never observed — Zero will not act on a position it cannot price',
      pnlPct: absent('fill unobserved'),
    }
  }
  if (position.state !== 'open') {
    return hold(`position is ${position.state}`, absent(`position is ${position.state}`))
  }

  const pnl = pnlPct(position, priceSol)
  if (!pnl.measured) {
    return hold('cannot price this position — no entry observed', pnl)
  }

  // 1. Stop-loss. First, always, and never gated on coherence: a degraded feed must not
  //    stop you closing existing risk.
  if (pnl.value <= -config.stopLossPct) {
    return {
      exit: true,
      reason: 'stop-loss',
      detail: `${pnl.value.toFixed(1)}% at or below stop -${config.stopLossPct}%`,
      pnlPct: pnl,
    }
  }

  // 2. Take-profit.
  if (pnl.value >= config.takeProfitPct) {
    return {
      exit: true,
      reason: 'take-profit',
      detail: `${pnl.value.toFixed(1)}% at or above target +${config.takeProfitPct}%`,
      pnlPct: pnl,
    }
  }

  // 3. Pullback from the peak — but only once the peak has cleared the momentum floor.
  //
  //    `configProblem` asserts `momentumMinPeakPct > takeProfitPct + pullbackExitPct`, or
  //    this branch is unsatisfiable: the peak arms only above the momentum floor, and any
  //    position that high has already taken profit above. That exact relationship has been
  //    broken in the Rust config before, which is why it is an assertion and not a comment.
  if (position.peakPriceSol.measured && position.entryPriceSol.measured) {
    const peakPnl =
      ((position.peakPriceSol.value - position.entryPriceSol.value) / position.entryPriceSol.value) * 100
    if (peakPnl >= config.momentumMinPeakPct) {
      const givenBack = peakPnl - pnl.value
      if (givenBack >= config.pullbackExitPct) {
        return {
          exit: true,
          reason: 'pullback',
          detail: `peaked at +${peakPnl.toFixed(0)}%, gave back ${givenBack.toFixed(0)}% (limit ${config.pullbackExitPct}%)`,
          pnlPct: pnl,
        }
      }
    }
  }

  // 4. No-pump timeout — evaluated ON ARRIVAL with elapsed read from the event, never
  //    assumed to have fired on schedule. See the header.
  const heldSecs = atUnix - position.openedAtUnix
  if (heldSecs >= config.noPumpTimeoutSecs && Math.abs(pnl.value) < 5) {
    return {
      exit: true,
      reason: 'no-pump',
      detail: `${Math.round(heldSecs)}s held and still ${pnl.value.toFixed(1)}% — no move`,
      pnlPct: pnl,
    }
  }

  return hold(
    `holding at ${pnl.value.toFixed(1)}% (${Math.round(heldSecs)}s)`,
    pnl,
  )
}

/**
 * Fold an arriving price into the position.
 *
 * The peak only ever rises, and it rises only on a MEASURED price. A peak seeded from an
 * unmeasured value would arm the pullback rule against a number nobody observed.
 */
export function applyPrice(position: Position, priceSol: number, atUnix: number): Position {
  const peakPriceSol =
    position.peakPriceSol.measured && position.peakPriceSol.value >= priceSol
      ? position.peakPriceSol
      : measured(priceSol)

  return {
    ...position,
    lastPriceSol: measured(priceSol),
    peakPriceSol,
    lastEvaluatedUnix: atUnix,
  }
}
