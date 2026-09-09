// The loop, as a pure reducer.
//
//   (state, ZeroEvent) -> { state, effects }
//
// No I/O, no clock, no storage, no network. Every time value arrives on the event; every
// action leaves as an `Effect` for the shell to perform. That is what lets `check:zero`
// pin a stop-loss firing, a cap refusing and a fill nobody could observe — none of which
// can be tested against mainnet without spending money to reach the interesting cases.
//
// ── Z-1 lives here ───────────────────────────────────────────────────────────
//
// **No timer may decide money.** The `tick` event is handled below and produces no
// `swap` effect under any state whatsoever. Every entry is reached from `pool.observed`
// and every exit from `vault.changed` — both of which are WebSocket arrivals, which are
// delivered to a hidden tab where a `setInterval` is throttled to roughly once a minute.
//
// This is not a convention to be careful about; `check:zero` feeds the reducer a million
// ticks in every reachable state and asserts no swap ever comes out.

import {
  type Coverage,
  type DecisionRecord,
  type DeclineReason,
  type Effect,
  type ObservedPool,
  type Position,
  type Term,
  type Verdict,
  type ZeroConfig,
  type ZeroEvent,
  absent,
  measured,
} from './types.ts'
import { type CoherenceState, evaluate as evaluateCoherence, newCoherence, record as recordRead } from './gate.ts'
import { type PriceHistory, pushPrice, selectStrategies, type StrategyName } from './strategy.ts'
import { applyPrice, evaluateExit } from './exits.ts'
import { advise } from './policy.ts'
import { POLICY_ID } from './policy.ts'
import { estimateEdge, sizeEntry, type TradeOutcome } from './size.ts'
import {
  type SessionCaps,
  type SessionLedger,
  authorise,
  newLedger,
  release,
  settle,
  strand,
  remaining,
} from './session.ts'
import { type LeaseState, mayTrade } from './lease.ts'

export interface ZeroState {
  config: ZeroConfig
  strategies: StrategyName[]
  armed: boolean
  killed: boolean
  lease: LeaseState
  socketOpen: boolean
  lastArrivalUnix: number | null
  coherence: CoherenceState
  positions: Record<string, Position>
  history: Record<string, PriceHistory>
  ledger: SessionLedger
  caps: SessionCaps
  /** Settled trades, for the edge estimate. */
  outcomes: TradeOutcome[]
  /** Reservation id per mint, so a resolution can find its hold. */
  holds: Record<string, string>
  /** Sealed decisions, newest last. Bounded — see `RECORD_CAP`. */
  records: DecisionRecord[]
  notices: Array<{ level: 'info' | 'warn' | 'alarm'; text: string; atUnix: number }>
}

/** A browser tab may run for hours. An unbounded array is how a page dies quietly. */
export const RECORD_CAP = 500
export const NOTICE_CAP = 50

export function initialState(config: ZeroConfig, caps: SessionCaps): ZeroState {
  return {
    config,
    strategies: ['scored-entry', 'pullback', 'continuation'],
    armed: false,
    killed: false,
    lease: 'follower',
    socketOpen: false,
    lastArrivalUnix: null,
    coherence: newCoherence(),
    positions: {},
    history: {},
    ledger: newLedger(),
    caps,
    outcomes: [],
    holds: {},
    records: [],
    notices: [],
  }
}

export interface Step {
  state: ZeroState
  effects: Effect[]
}

const cap = <T>(xs: T[], n: number): T[] => (xs.length > n ? xs.slice(xs.length - n) : xs)

function notify(
  state: ZeroState,
  level: 'info' | 'warn' | 'alarm',
  text: string,
  atUnix: number,
): ZeroState {
  return {
    ...state,
    notices: cap([...state.notices, { level, text, atUnix }], NOTICE_CAP),
  }
}

// ── the entry decision ───────────────────────────────────────────────────────

/**
 * Decide whether to open a position.
 *
 * The order of refusals is the order an operator needs them in: the cheap, local,
 * definitely-true reasons first, so an armed-but-broke Zero says "budget exhausted"
 * rather than running a net and a scorer to arrive at the same place.
 */
export function decideEntry(state: ZeroState, pool: ObservedPool, atUnix: number): Verdict {
  const emptyCoverage: Coverage = { measuredCount: 0, total: 0 }
  const decline = (
    reason: DeclineReason,
    text: string,
    extra: Partial<Verdict> = {},
  ): Verdict => ({
    act: false,
    decline: reason,
    sizeLamports: 0,
    reason: text,
    coverage: extra.coverage ?? emptyCoverage,
    score: extra.score ?? absent('not scored'),
    psi: extra.psi ?? absent('not evaluated'),
    q: extra.q,
  })

  if (state.killed) return decline('killed', 'kill switch is engaged')
  if (!state.armed) return decline('not-armed', 'autonomy is not armed')
  if (!mayTrade(state.lease)) {
    return decline('no-lease', 'another tab holds the writer lease')
  }
  if (state.positions[pool.mint]) {
    return decline('position-open', `already holding ${pool.symbol}`)
  }
  const open = Object.values(state.positions).filter(p => p.state === 'open' || p.state === 'opening')
  if (open.length >= state.config.maxOpenPositions) {
    return decline('max-positions', `${open.length} positions open (max ${state.config.maxOpenPositions})`)
  }

  // Coherence. Entries only — `vault.changed` never consults this.
  const coh = evaluateCoherence(state.coherence, state.config.minCoherenceSamples, state.config.minPsi)
  if (!coh.entriesAllowed) {
    return decline('coherence-degraded', coh.reason, { psi: coh.psi })
  }

  // Strategy.
  const score = poolScore(pool)
  const history = state.history[pool.mint] ?? { mint: pool.mint, points: [] }
  const choice = selectStrategies(state.strategies, score, history, atUnix, state.config)
  if (choice.fired.length === 0) {
    return decline('filters-rejected', choice.reason, { score, psi: coh.psi })
  }

  // Policy. Attached, never averaged into the decision — a utility and a Q are not the
  // same quantity — and a measured veto is a veto while an unmeasured one is silence.
  const advice = advise({
    initial_liquidity_sol: pool.sizeSol.measured ? pool.sizeSol.value : undefined,
    pool_age_secs: pool.ageSecs.measured ? pool.ageSecs.value : undefined,
    lp_burned: pool.lpBurned.measured ? (pool.lpBurned.value ? 1 : 0) : undefined,
    mint_renounced: pool.mintRenounced.measured ? (pool.mintRenounced.value ? 1 : 0) : undefined,
    buy_sell_ratio: pool.buyPressure.measured ? pool.buyPressure.value : undefined,
    pool_score_norm: score.measured ? score.value / 100 : undefined,
    deployer_rug_rate: pool.devHoldingPct.measured ? pool.devHoldingPct.value / 100 : undefined,
    open_positions: open.length,
    time_of_day_norm: ((atUnix % 86400) / 86400),
  })

  if (advice.lean === 'veto') {
    return decline('policy-veto', advice.reason, {
      score, psi: coh.psi, coverage: advice.coverage, q: advice.q,
    })
  }

  // Size.
  const edge = estimateEdge(state.outcomes)
  const base = Math.floor(state.caps.maxPerTradeLamports * state.config.maxEntryFraction * 4)
  const sized = sizeEntry(
    base,
    edge,
    choice.conviction,
    advice.sizeMultiplier,
    state.caps.maxPerTradeLamports,
    remaining(state.ledger, state.caps),
    state.config.dustLamports,
  )

  if (sized.lamports === 0) {
    const reason: DeclineReason =
      remaining(state.ledger, state.caps) < state.config.dustLamports
        ? 'budget-exhausted'
        : 'size-below-dust'
    return decline(reason, sized.reason, {
      score, psi: coh.psi, coverage: advice.coverage, q: advice.q,
    })
  }

  return {
    act: true,
    sizeLamports: sized.lamports,
    reason: `${choice.reason} | ${advice.reason} | ${sized.reason}`,
    coverage: advice.coverage,
    score,
    psi: coh.psi,
    q: advice.q,
  }
}

/**
 * The pool's score.
 *
 * Absent when the inputs the scorer needs were not measured — an unscored pool is not a
 * low-scoring one, and `scoredEntry` refuses on absence rather than treating it as a
 * failing grade.
 */
function poolScore(pool: ObservedPool): Term {
  if (!pool.sizeSol.measured) return absent('pool depth was not read')
  // The full ladder lives in lib/feed/scorer.ts; this is the depth-only fallback used
  // when a pool arrives from a subscription rather than the scored feed. It is
  // deliberately conservative: it can decline an entry, never inflate one.
  const size = pool.sizeSol.value
  if (size < 10 || size > 150) return measured(0)
  const mid = 1 - Math.abs(size - 45) / 105
  let s = 50 + mid * 30
  if (pool.mintRenounced.measured && pool.mintRenounced.value) s += 8
  if (pool.freezeDisabled.measured && pool.freezeDisabled.value) s += 6
  if (pool.lpBurned.measured && pool.lpBurned.value) s += 6
  return measured(Math.max(0, Math.min(100, s)))
}

function recordOf(verdict: Verdict, pool: ObservedPool, atUnix: number): DecisionRecord {
  return {
    schema: 'scema.zero.decision/1',
    id: '',
    atUnix,
    mint: pool.mint,
    act: verdict.act,
    decline: verdict.decline,
    reason: verdict.reason,
    score: verdict.score,
    psi: verdict.psi,
    coverage: verdict.coverage,
    sizeLamports: verdict.sizeLamports,
    q: verdict.q,
    world: pool,
    policyId: POLICY_ID,
  }
}

// ── the reducer ──────────────────────────────────────────────────────────────

export function step(state: ZeroState, event: ZeroEvent): Step {
  switch (event.kind) {
    // ── the powerless heartbeat ──────────────────────────────────────────────
    //
    // Deliberately inert. It updates nothing that can decide, emits no swap, and exists
    // so a UI can re-render and an operator can see the loop is alive. Z-1.
    case 'tick':
      return { state, effects: [] }

    case 'socket.opened':
      return {
        state: { ...state, socketOpen: true },
        effects: [{ kind: 'notify', level: 'info', text: 'chain feed connected' }],
      }

    case 'socket.closed': {
      // Not a reason to close positions — it is a reason to say the exits are not being
      // evaluated. Closing on a dropped socket would sell on a network blip.
      const next = notify(
        { ...state, socketOpen: false },
        'alarm',
        `chain feed closed (${event.reason}) — exits are NOT being evaluated`,
        event.atUnix,
      )
      return { state: next, effects: [{ kind: 'notify', level: 'alarm', text: 'feed closed — exits are not being evaluated' }] }
    }

    case 'read.resolved':
      return { state: { ...state, coherence: recordRead(state.coherence, true) }, effects: [] }

    case 'read.failed':
      return { state: { ...state, coherence: recordRead(state.coherence, false) }, effects: [] }

    case 'lease.acquired':
      return { state: { ...state, lease: 'writer' }, effects: [] }

    case 'lease.lost':
      // Losing the lease must not close positions: the tab that HAS the lease is
      // evaluating them. Two tabs both closing is exactly the race the lease prevents.
      return { state: { ...state, lease: 'follower' }, effects: [] }

    case 'arm':
      return {
        state: { ...state, armed: true, killed: false },
        effects: [{ kind: 'persist' }],
      }

    case 'disarm':
      return {
        state: notify({ ...state, armed: false }, 'warn', `disarmed: ${event.reason}`, event.atUnix),
        effects: [{ kind: 'persist' }],
      }

    case 'kill':
      // Halts entries and disarms. Does NOT close positions — a kill switch that dumps
      // everything at market is a different, much more dangerous control, and conflating
      // the two means nobody can stop new entries without also being forced to sell.
      return {
        state: notify({ ...state, armed: false, killed: true }, 'alarm', 'kill switch engaged — entries halted, positions untouched', event.atUnix),
        effects: [
          { kind: 'persist' },
          { kind: 'notify', level: 'alarm', text: 'Kill switch engaged. Entries halted. Open positions are untouched and still exit on their rules.' },
        ],
      }

    // ── entry ────────────────────────────────────────────────────────────────
    case 'pool.observed': {
      const verdict = decideEntry(state, event.pool, event.atUnix)
      const record = recordOf(verdict, event.pool, event.atUnix)

      // Every decision is sealed, including the declines. See seal.ts.
      const effects: Effect[] = [{ kind: 'seal', record }]
      let next: ZeroState = {
        ...state,
        lastArrivalUnix: event.atUnix,
        records: cap([...state.records, record], RECORD_CAP),
      }

      if (!verdict.act) return { state: next, effects }

      // Decide-and-reserve is ONE step. See session.ts: a cap checked against a snapshot
      // and enforced after an await is not a cap.
      const auth = authorise(
        next.ledger,
        next.caps,
        verdict.sizeLamports,
        event.atUnix,
        next.armed,
        event.pool.mint,
      )
      if (!auth.ok) {
        const refused = recordOf(
          { ...verdict, act: false, decline: 'budget-exhausted', sizeLamports: 0, reason: auth.detail },
          event.pool,
          event.atUnix,
        )
        return {
          state: { ...next, records: cap([...next.records, refused], RECORD_CAP) },
          effects: [...effects, { kind: 'seal', record: refused }],
        }
      }

      next = {
        ...next,
        ledger: auth.ledger,
        holds: { ...next.holds, [event.pool.mint]: auth.reservation.id },
        positions: {
          ...next.positions,
          [event.pool.mint]: {
            mint: event.pool.mint,
            symbol: event.pool.symbol,
            state: 'opening',
            spentLamports: verdict.sizeLamports,
            tokensOut: absent('fill not yet observed'),
            entryPriceSol: absent('fill not yet observed'),
            peakPriceSol: absent('no price since entry'),
            lastPriceSol: absent('no price since entry'),
            openedAtUnix: event.atUnix,
            lastEvaluatedUnix: event.atUnix,
          },
        },
      }

      return {
        state: next,
        effects: [
          ...effects,
          { kind: 'subscribe', mint: event.pool.mint },
          {
            kind: 'swap',
            mint: event.pool.mint,
            side: 'buy',
            amount: verdict.sizeLamports,
            signer: 'session',
            reason: verdict.reason,
          },
          { kind: 'persist' },
        ],
      }
    }

    // ── exit ─────────────────────────────────────────────────────────────────
    //
    // THE load-bearing case. Every exit rule is evaluated here, on arrival, because a
    // WebSocket handler runs in a hidden tab where a timer does not.
    case 'vault.changed': {
      const history = pushPrice(
        state.history[event.mint] ?? { mint: event.mint, points: [] },
        event.atUnix,
        event.priceSol,
      )
      let next: ZeroState = {
        ...state,
        lastArrivalUnix: event.atUnix,
        history: { ...state.history, [event.mint]: history },
      }

      const position = state.positions[event.mint]
      if (!position) return { state: next, effects: [] }

      const updated = applyPrice(position, event.priceSol, event.atUnix)
      next = { ...next, positions: { ...next.positions, [event.mint]: updated } }

      // A close already in flight must not be issued twice.
      if (updated.state !== 'open' || updated.closeSignature) {
        return { state: next, effects: [] }
      }

      const decision = evaluateExit(updated, event.priceSol, event.atUnix, state.config)
      if (!decision.exit) return { state: next, effects: [] }

      // NOTE: no coherence check. A degraded feed must never stop you closing risk.
      return {
        state: {
          ...next,
          positions: { ...next.positions, [event.mint]: { ...updated, state: 'closing' } },
        },
        effects: [
          {
            kind: 'swap',
            mint: event.mint,
            side: 'sell',
            amount: updated.tokensOut.measured ? updated.tokensOut.value : 0,
            signer: 'session',
            reason: `${decision.reason}: ${decision.detail}`,
          },
          { kind: 'notify', level: 'info', text: `exiting ${updated.symbol}: ${decision.detail}` },
          { kind: 'persist' },
        ],
      }
    }

    // ── fills ────────────────────────────────────────────────────────────────
    case 'fill.observed': {
      const position = state.positions[event.mint]
      if (!position) return { state, effects: [] }
      const holdId = state.holds[event.mint]

      if (position.state === 'opening') {
        // Three distinguishable outcomes, and collapsing any two of them loses money.
        //
        //   null  — it landed and we could not read what it produced. UNMEASURED, so
        //           `exits.ts` refuses to price the position rather than inventing one.
        //   0     — it landed and produced nothing. A measured, real, terrible fill.
        //   n > 0 — a fill, and therefore an entry price.
        const tokensOut =
          event.outAmount === null
            ? absent('the fill landed but its output could not be read')
            : measured(event.outAmount)
        const entryPriceSol =
          event.outAmount === null
            ? absent('no observed output, so no entry price')
            : event.outAmount > 0
              ? measured(position.spentLamports / event.outAmount)
              : absent('the fill produced zero tokens')

        return {
          state: {
            ...state,
            ledger: holdId ? settle(state.ledger, holdId, position.spentLamports) : state.ledger,
            positions: {
              ...state.positions,
              [event.mint]: {
                ...position,
                // A landed buy whose output is unreadable is not `open` — it is a position
                // Zero holds and cannot price, which is exactly what `unknown` means.
                state: event.outAmount === null ? 'unknown' : 'open',
                tokensOut,
                entryPriceSol,
                openSignature: event.signature,
              },
            },
          },
          effects:
            event.outAmount === null
              ? [
                  {
                    kind: 'notify',
                    level: 'alarm',
                    text: `Bought ${position.symbol} and could not read the fill. The position is held but unpriceable — Zero will not sell it on a percentage it did not compute. Signature ${event.signature}`,
                  },
                  { kind: 'persist' },
                ]
              : [{ kind: 'persist' }],
        }
      }

      // A close landed. Realised PnL comes from the OBSERVED output, never from a quote —
      // and an output nobody could read produces NO outcome rather than a zero one. A
      // fabricated 0% would enter the edge estimate and size every subsequent trade.
      const realised =
        event.outAmount !== null && position.spentLamports > 0
          ? ((event.outAmount - position.spentLamports) / position.spentLamports) * 100
          : null

      const { [event.mint]: _dropped, ...rest } = state.positions
      const { [event.mint]: _hold, ...holds } = state.holds
      return {
        state: {
          ...state,
          positions: rest,
          holds,
          outcomes: realised === null ? state.outcomes : [...state.outcomes, { pnlPct: realised }],
        },
        effects: [{ kind: 'unsubscribe', mint: event.mint }, { kind: 'persist' }],
      }
    }

    case 'fill.failed': {
      const position = state.positions[event.mint]
      const holdId = state.holds[event.mint]
      // Positive evidence of failure: nothing moved, so the hold is released.
      const ledger = holdId ? release(state.ledger, holdId) : state.ledger
      if (!position) return { state: { ...state, ledger }, effects: [{ kind: 'persist' }] }

      if (position.state === 'opening') {
        const { [event.mint]: _d, ...rest } = state.positions
        const { [event.mint]: _h, ...holds } = state.holds
        return {
          state: notify({ ...state, ledger, positions: rest, holds }, 'warn', `entry failed: ${event.reason}`, Date.now() / 1000),
          effects: [{ kind: 'unsubscribe', mint: event.mint }, { kind: 'persist' }],
        }
      }
      // A close failed: the position is still open and its rules still apply.
      return {
        state: {
          ...state,
          ledger,
          positions: { ...state.positions, [event.mint]: { ...position, state: 'open', closeSignature: undefined } },
        },
        effects: [{ kind: 'notify', level: 'warn', text: `exit failed for ${position.symbol}; still holding` }, { kind: 'persist' }],
      }
    }

    case 'fill.unknown': {
      const position = state.positions[event.mint]
      const holdId = state.holds[event.mint]
      // The hold STAYS. Releasing it is how a paid trade gets paid again.
      const ledger = holdId ? strand(state.ledger, holdId) : state.ledger
      if (!position) return { state: { ...state, ledger }, effects: [{ kind: 'persist' }] }

      return {
        state: notify(
          {
            ...state,
            ledger,
            positions: { ...state.positions, [event.mint]: { ...position, state: 'unknown' } },
          },
          'alarm',
          `fill unobserved for ${position.symbol} — signature ${event.signature}`,
          event.atUnix,
        ),
        effects: [
          {
            kind: 'notify',
            level: 'alarm',
            text: `A swap for ${position.symbol} was submitted but could not be observed. It may have landed. Zero has NOT retried. Signature ${event.signature}`,
          },
          { kind: 'persist' },
        ],
      }
    }
  }
}

/** Fold a batch. Convenience for tests and for replaying a persisted event log. */
export function run(state: ZeroState, events: ZeroEvent[]): Step {
  const effects: Effect[] = []
  let s = state
  for (const e of events) {
    const out = step(s, e)
    s = out.state
    effects.push(...out.effects)
  }
  return { state: s, effects }
}
