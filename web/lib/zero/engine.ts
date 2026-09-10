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
import { DUST_LAMPORTS } from './types.ts'
import {
  type CoherenceState,
  evaluate as evaluateCoherence,
  newCoherence,
  record as recordRead,
  recordPoolSeen,
} from './sniper/coherence.ts'
import {
  type LadderState,
  evaluate as evaluateLadder,
  newLadder,
  valueOf,
} from './sniper/exit-ladder.ts'
import { type KellyTrade, computeMultiplier, kellySizer } from './sniper/kelly.ts'
import { scorePool } from '../feed/scorer.ts'
import { advise } from './policy.ts'
import { POLICY_ID } from './policy.ts'
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
  armed: boolean
  killed: boolean
  lease: LeaseState
  socketOpen: boolean
  lastArrivalUnix: number | null
  coherence: CoherenceState
  positions: Record<string, Position>
  ledger: SessionLedger
  caps: SessionCaps
  /**
   * Settled trades, newest last, for the Kelly estimate.
   *
   * Bounded to `kellyLookback`, which is the sniper's own window — an unbounded history
   * would make the multiplier a claim about the whole session rather than about recent
   * behaviour, and the tournament promotion bug in `scematica-nn` is the same mistake:
   * a comparison over a lifetime sum cannot change its mind.
   */
  outcomes: KellyTrade[]
  /**
   * Wallet balance in SOL, when the host has read one.
   *
   * `null` selects profit-first mode's wider rug-only stop. See
   * `exit-ladder.ts::effectiveStopLossPct` for why the wider one is the safe default.
   */
  walletSol: number | null
  /** Reservation id per mint, so a resolution can find its hold. */
  holds: Record<string, string>
  /** Sealed decisions, newest last. Bounded — see `RECORD_CAP`. */
  records: DecisionRecord[]
  notices: Array<{ level: 'info' | 'warn' | 'alarm'; text: string; atUnix: number }>
}

/** A browser tab may run for hours. An unbounded array is how a page dies quietly. */
export const RECORD_CAP = 500
export const NOTICE_CAP = 50

/**
 * `startedUnix` is when Zero began watching, and it is required rather than defaulted.
 *
 * The coherence breaker ages its feed from this point until the first pool arrives, so a
 * listener that never connects eventually reads as stalled instead of as permanently
 * fresh (`coherence.rs::feed_age_secs`). A default of 0 would make every fresh session
 * look decades stale; a default of "now" would need a clock, and this module has none.
 */
export function initialState(
  config: ZeroConfig,
  caps: SessionCaps,
  startedUnix: number,
): ZeroState {
  return {
    config,
    armed: false,
    killed: false,
    lease: 'follower',
    socketOpen: false,
    lastArrivalUnix: null,
    coherence: newCoherence(startedUnix),
    positions: {},
    ledger: newLedger(),
    caps,
    outcomes: [],
    walletSol: null,
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
  // `max_concurrent_positions = 0` means unlimited in Rust, and 0 is what `config.rs`
  // defaults to. Treating it as "no positions allowed" would make the default config a
  // bot that never trades.
  const maxOpen = state.config.maxConcurrentPositions
  if (maxOpen > 0 && open.length >= maxOpen) {
    return decline('max-positions', `${open.length} positions open (max ${maxOpen})`)
  }

  // Coherence. Entries only — `vault.changed` never consults this.
  const coh = evaluateCoherence(state.coherence, atUnix, state.config.coherenceBreaker)
  if (!coh.entriesAllowed) {
    return decline('coherence-degraded', coh.reason, { psi: coh.psi })
  }

  // ── the pool score, from the sniper's own ladder ──────────────────────────
  //
  // `lib/feed/scorer.ts` is a verbatim port of `PoolScorer::score` and is pinned against
  // Rust by both `check:parity` and `check:zero`. Zero used to carry its own
  // depth-weighted heuristic here — `50 + mid*30`, plus eight points for a renounced mint
  // — which was a different function with a different range that happened to return a
  // number between 0 and 100 and be compared against the sniper's 65 floor.
  const score = poolScore(pool)
  if (!score.measured) {
    return decline('filters-rejected', `pool not scoreable: ${score.note}`, { score, psi: coh.psi })
  }
  if (score.value < state.config.minPoolScore) {
    return decline(
      'score-below-floor',
      `score ${score.value.toFixed(1)} below the floor ${state.config.minPoolScore}`,
      { score, psi: coh.psi },
    )
  }

  // The depth band, `[filters] min_pool_size` / `max_pool_size`. A separate refusal from
  // the score even though the scorer also penalises depth, because the sniper enforces
  // both and they answer different questions: the ladder RANKS a thin pool, the band
  // REFUSES one.
  if (pool.sizeSol.measured) {
    if (pool.sizeSol.value < state.config.minPoolSize) {
      return decline('filters-rejected', `${pool.sizeSol.value.toFixed(1)} SOL is below min_pool_size ${state.config.minPoolSize}`, { score, psi: coh.psi })
    }
    if (state.config.maxPoolSize > 0 && pool.sizeSol.value > state.config.maxPoolSize) {
      return decline('filters-rejected', `${pool.sizeSol.value.toFixed(1)} SOL is above max_pool_size ${state.config.maxPoolSize}`, { score, psi: coh.psi })
    }
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

  // ── size ─────────────────────────────────────────────────────────────────
  //
  // The sniper's own sizer: `quote_amount` SOL as the base, times `KellySizer`'s
  // multiplier over the last `kelly_lookback` settled trades, times the policy's own
  // multiplier. Kelly returns a MULTIPLIER in [0.25, 3.0], so a strong measured edge can
  // size up — the previous implementation could only ever scale the base down, which made
  // every good run size like a bad one.
  //
  // `kelly_sizing` is a config flag and it is `false` in the shipped config. Honouring it
  // rather than always applying Kelly is the difference between porting the bot and
  // porting the parts of it somebody liked.
  const kelly = state.config.kellySizing
    ? computeMultiplier(
        kellySizer(state.config.kellyFraction),
        state.outcomes.slice(-state.config.kellyLookback),
      )
    : { multiplier: 1.0, reason: 'kelly_sizing is off — base size' }

  const baseLamports = Math.floor(state.config.quoteAmount * 1e9)
  let lamports = Math.floor(baseLamports * kelly.multiplier * advice.sizeMultiplier)

  // The session caps clamp afterwards. Two gates, different questions: the sizer says what
  // the edge is worth, the ledger says what may be afforded. Merging them lets a strong
  // edge argue its way past a cap.
  const budget = remaining(state.ledger, state.caps)
  const clamps: string[] = []
  if (lamports > state.caps.maxPerTradeLamports) {
    lamports = state.caps.maxPerTradeLamports
    clamps.push('clamped to the per-trade cap')
  }
  if (lamports > budget) {
    lamports = budget
    clamps.push('clamped to the remaining session budget')
  }

  if (lamports < DUST_LAMPORTS) {
    const reason: DeclineReason = budget < DUST_LAMPORTS ? 'budget-exhausted' : 'size-below-dust'
    return decline(
      reason,
      `${lamports} lamports is below the dust floor ${DUST_LAMPORTS} — the fee would dominate`,
      { score, psi: coh.psi, coverage: advice.coverage, q: advice.q },
    )
  }

  const sizeReason = [`base ${state.config.quoteAmount} SOL`, kelly.reason, advice.reason, ...clamps]
    .filter(Boolean)
    .join('; ')

  return {
    act: true,
    sizeLamports: lamports,
    reason: `score ${score.value.toFixed(1)} | ${sizeReason}`,
    coverage: advice.coverage,
    score,
    psi: coh.psi,
    q: advice.q,
  }
}

/**
 * The pool's score, from the sniper's ladder.
 *
 * `scorePool` in `lib/feed/scorer.ts` is a verbatim port of `PoolScorer::score` — the
 * empirical-Bayes product of likelihood ratios through a logistic — and `check:zero`
 * asserts it reproduces Rust on every case in the parity fixture. Nothing is computed
 * here; this function only decides what may be handed to it.
 *
 * Depth is the one input with no honest substitute: unmeasured depth is an UNSCORED pool,
 * never a low-scoring one, and the caller declines on absence rather than treating it as
 * a failing grade. Age is different — `null` is a real arm of the Rust ladder (the
 * pump.fun migration whose `open_time` is 0, which is almost every pool it sees) and gets
 * its own likelihood ratio rather than being an error.
 */
function poolScore(pool: ObservedPool): Term {
  if (!pool.sizeSol.measured) return absent('pool depth was not read')
  return measured(
    scorePool({
      sizeSol: pool.sizeSol.value,
      ageSecs: pool.ageSecs.measured ? pool.ageSecs.value : null,
      // The buy-pressure ratio is quote_vault / base_vault. Zero reads both vaults to
      // price the position anyway, so unlike the public feed it can supply this — and
      // when it cannot, `undefined` takes Rust's own 0.80 "no confirmation" penalty
      // rather than a neutral 1.0.
      buyPressureRatio: pool.buyPressure.measured ? pool.buyPressure.value : undefined,
      pumpfunScore: 0,
    }),
  )
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

    // The coherence window is 120 SECONDS in Rust, not a fixed sample count, so a read
    // has to say when it happened. A sample arriving after a long silence opens a new
    // window rather than landing in a stale one.
    case 'read.resolved':
      return { state: { ...state, coherence: recordRead(state.coherence, true, event.atUnix) }, effects: [] }

    case 'read.failed':
      return { state: { ...state, coherence: recordRead(state.coherence, false, event.atUnix) }, effects: [] }

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
        // `CoherenceBreaker::record_pool_seen`. Recorded on arrival, before any decision,
        // so a pool the filters reject still counts as the feed being alive — a breaker
        // that only saw the pools it liked would read a strict config as a dead feed.
        coherence: recordPoolSeen(state.coherence, event.atUnix),
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
      let next: ZeroState = { ...state, lastArrivalUnix: event.atUnix }

      const position = state.positions[event.mint]
      if (!position) return { state: next, effects: [] }

      // A close already in flight must not be issued twice, and a position whose fill was
      // never observed has no entry value — so the ladder has nothing to anchor to and
      // `newLadder` was never built for it. Both are surfaced to the operator elsewhere;
      // neither may produce a swap.
      if (position.state !== 'open' || position.closeSignature || !position.ladder) {
        return { state: next, effects: [] }
      }
      if (!position.tokensOut.measured) return { state: next, effects: [] }

      // The ladder works in lamports of position value, exactly as the Rust monitor does.
      const value = valueOf(position.tokensOut.value, event.priceSol)
      const { state: ladder, decision } = evaluateLadder(
        position.ladder,
        { valueLamports: value, atUnix: event.atUnix, quoteVaultLamports: event.quoteVaultLamports ?? null },
        state.config,
      )

      const updated: Position = {
        ...position,
        ladder,
        lastPriceSol: measured(event.priceSol),
        peakPriceSol:
          position.peakPriceSol.measured && position.peakPriceSol.value >= event.priceSol
            ? position.peakPriceSol
            : measured(event.priceSol),
        lastEvaluatedUnix: event.atUnix,
      }
      next = { ...next, positions: { ...next.positions, [event.mint]: updated } }

      // NOTE: no coherence check on this path, ever. A degraded feed is a reason to stop
      // opening new risk and never a reason to stop closing existing risk.

      // A tiered partial sells part of the position and leaves it open. It is a different
      // effect from an exit and must not set `closing`, or the next arrival finds a
      // position it refuses to evaluate and the remaining tokens are never sold.
      if (decision.partial) {
        const amount = Math.floor(position.tokensOut.value * decision.partial.fraction)
        if (amount <= 0) return { state: next, effects: [] }
        return {
          state: next,
          effects: [
            {
              kind: 'swap',
              mint: event.mint,
              side: 'sell',
              amount,
              signer: 'session',
              reason: decision.partial.detail,
            },
            { kind: 'notify', level: 'info', text: `${updated.symbol}: ${decision.partial.detail}` },
            { kind: 'persist' },
          ],
        }
      }

      if (!decision.exit) return { state: next, effects: [] }

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
            amount: position.tokensOut.value,
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
                // The sell monitor's loop-locals, anchored to what was ACTUALLY spent
                // rather than to what was requested. An unreadable fill gets no ladder at
                // all — see `Position.ladder`.
                ladder:
                  event.outAmount === null
                    ? undefined
                    : newLadder(position.spentLamports, event.atUnix, state.config, state.walletSol),
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
      // `KellySizer` averages the MAGNITUDE of realised PnL in SOL, and takes the
      // win/loss verdict separately. Percentages agree with SOL only while every position
      // is the same size, which is exactly what Kelly sizing stops being true.
      const realised: KellyTrade | null =
        event.outAmount !== null && position.spentLamports > 0
          ? {
              profitable: event.outAmount >= position.spentLamports,
              pnlSol: (event.outAmount - position.spentLamports) / 1e9,
            }
          : null

      const { [event.mint]: _dropped, ...rest } = state.positions
      const { [event.mint]: _hold, ...holds } = state.holds
      return {
        state: {
          ...state,
          positions: rest,
          holds,
          // Bounded to the sniper's own lookback. An unbounded history makes the
          // multiplier a claim about the whole session rather than about recent trades.
          outcomes:
            realised === null
              ? state.outcomes
              : cap([...state.outcomes, realised], state.config.kellyLookback),
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
