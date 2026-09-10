// The sell monitor's exit ladder — a port of the loop in `sniper.rs`.
//
// ⚠️  PORT, and the weakest one in this directory. Say so plainly:
//
// Everything else here is pinned value-for-value against a fixture the sniper emits.
// This is not, because the Rust original is four hundred lines inline in an async loop
// (`crates/scematica-sniper/src/sniper.rs`, the `monitor_and_sell` body) rather than a
// callable function. Extracting it would be a large diff in a live buy-and-sell path,
// which is how a trading bug gets introduced by a diagnostic — so it stays where it is
// and this is a hand port of it.
//
// What that gap is and is not: every THRESHOLD the ladder branches on comes from
// `sniper/config.ts`, which IS pinned, so a rule here cannot fire at the wrong level.
// What is unpinned is ORDER and CONDITION — whether the trailing stop ratchets before the
// pullback rule reads the peak, whether the dump detector is gated on the profit floor.
// Those are asserted in `check:zero` against the behaviour described below, which is a
// weaker guarantee than a fixture and is stated rather than glossed. Same posture as
// `scema-lean`'s README on the gap between a model and the code it models.
//
// ── Why this file has no timers in it ────────────────────────────────────────
//
// The Rust monitor polls: 30 checks at a fast interval, then `price_check_interval_ms`.
// A browser cannot. Chrome throttles `setInterval` in a hidden tab to roughly once a
// minute and tightens further after five, and announces none of it — so a polled
// stop-loss does not fail loudly when the operator switches tabs, it *keeps holding the
// position and stops checking it*.
//
// So `evaluate` is a pure function of (ladder state, arriving value, arrival time),
// driven by a vault WebSocket notification, because a message handler is delivered to a
// hidden tab where a timer is not. `checks` counts ARRIVALS rather than polls, which is
// the one place the port cannot be faithful: Rust's `fast_phase_checks` is 30 polls at a
// known cadence, and 30 arrivals is a different amount of time. It is named
// `FAST_PHASE_CHECKS` and used identically; what it means is different, and the
// difference is the host's, not a choice.
//
// ── Everything is a value in lamports, never a price ─────────────────────────
//
// Rust compares `current_value` (what the position is worth if sold now) against
// `entry_amount_raw` (what was spent), and every percentage in the ladder derives from
// that pair. Porting onto a per-token price would agree only while the token balance is
// constant — which stops being true the moment a tiered partial fires. So the ladder
// takes a value, and `valueOf` is the one place a price becomes one.

import type { SniperConfig } from './config.ts'

/** `fast_phase_checks` in `sniper.rs`. See the header on what it counts here. */
export const FAST_PHASE_CHECKS = 30

/** `profit_lock_floor = entry × 0.98` — near-breakeven, not breakeven. */
export const PROFIT_LOCK_FACTOR = 0.98

export type ExitReason =
  | 'take_profit'
  | 'stop_loss'
  | 'trailing_stop'
  | 'velocity_decay'
  | 'dump_detected'
  | 'no_pump_timeout'
  | 'volume_exhaustion'
  | 'max_hold'

/** A partial sale. The position stays open; only some of it leaves. */
export interface PartialExit {
  /** Fraction of the REMAINING token balance, as Rust computes it. */
  fraction: number
  level: number
  detail: string
}

export interface ExitDecision {
  exit: boolean
  reason?: ExitReason
  /** Fires alongside `exit: false` — a tier sells part and keeps the position. */
  partial?: PartialExit
  detail: string
  pnlPct: number
  /** The stop and target as they stand AFTER this evaluation. For the UI. */
  stopLossLamports: number
  targetProfitLamports: number
  dynamicTpPct: number
  escalations: number
  peakLamports: number
}

/**
 * Everything the Rust loop keeps in local variables between polls.
 *
 * It is state, not derivation, and that is the point: `stopLossAmount` only ever ratchets
 * up, `tieredFired` only ever fills, `dynamicTpPct` only ever escalates. Recomputing any
 * of them from the current price would silently un-ratchet a trailing stop the moment the
 * price dipped, which is the opposite of what a trailing stop is.
 */
export interface LadderState {
  entryLamports: number
  openedAtUnix: number
  /** Arrivals evaluated. Rust's `checks`. */
  checks: number
  peakValue: number
  prevValue: number
  /** Ratchets up only. Initialised from the effective stop-loss percentage. */
  stopLossLamports: number
  /** Escalates only. Initialised from `take_profit_pct`. */
  targetProfitLamports: number
  dynamicTpPct: number
  escalations: number
  declineStreak: number
  profitLockCounter: number
  /** Per-check delta as a percentage of entry. Rust's `velocity_window`. */
  velocityWindow: number[]
  decayWindow: number[]
  /** Which tiered-partial levels have fired. */
  tieredFired: boolean[]
  /** Quote vault at entry and on the previous check. 0 = never read. */
  entryQuote: number
  prevQuote: number
}

/**
 * `effective_stop_loss_pct` — profit-first mode's wider rug-only floor.
 *
 * While the wallet is below `wallet_target_sol` the stop widens to
 * `profit_first_floor_pct`, so the bot does not bleed out small losses on dips that would
 * have recovered. Zero cannot see the wallet's realised daily PnL the way the sniper's
 * `session_start + daily_pnl` approximation does, so it takes the balance from the caller
 * — and a caller that cannot read one passes `null`, which selects the WIDER floor.
 *
 * That default is the safe direction here and it is worth being explicit about why, since
 * "wider stop" does not sound safe: the narrow stop is the one that acts, and acting on an
 * unknown wallet state is what profit-first mode exists to prevent. Guessing the tight
 * stop would sell a recoverable dip on the strength of a balance nobody read.
 */
export function effectiveStopLossPct(config: SniperConfig, walletSol: number | null): number {
  if (!config.profitFirstMode) return config.stopLossPct
  if (config.walletTargetSol <= 0) return config.stopLossPct
  if (walletSol === null) return config.profitFirstFloorPct
  return walletSol < config.walletTargetSol ? config.profitFirstFloorPct : config.stopLossPct
}

export function newLadder(
  entryLamports: number,
  openedAtUnix: number,
  config: SniperConfig,
  walletSol: number | null,
): LadderState {
  const slPct = effectiveStopLossPct(config, walletSol)
  return {
    entryLamports,
    openedAtUnix,
    checks: 0,
    peakValue: entryLamports,
    prevValue: entryLamports,
    // `(entry as f64 * (1.0 - pct/100.0)) as u64` — a Rust `as u64` cast TRUNCATES toward
    // zero, so `Math.trunc`, not `Math.round`. A rounded stop sits up to one lamport above
    // the real one; irrelevant in size and wrong in a way that would show up nowhere else.
    stopLossLamports: Math.trunc(entryLamports * (1.0 - slPct / 100.0)),
    targetProfitLamports: Math.trunc(entryLamports * (1.0 + config.takeProfitPct / 100.0)),
    dynamicTpPct: config.takeProfitPct,
    escalations: 0,
    declineStreak: 0,
    profitLockCounter: 0,
    velocityWindow: [],
    decayWindow: [],
    tieredFired: config.tieredPartialTpLevels.map(() => false),
    entryQuote: 0,
    prevQuote: 0,
  }
}

/** What a token balance and a per-token SOL price are worth, in lamports. */
export function valueOf(tokens: number, priceSol: number): number {
  return Math.trunc(tokens * priceSol * 1e9)
}

export interface Arrival {
  /** What the position is worth if sold right now. */
  valueLamports: number
  atUnix: number
  /** Quote-vault balance, when the read resolved. `null` gates the vault-based rules off. */
  quoteVaultLamports?: number | null
}

const push = (win: number[], v: number, cap: number): number[] => {
  const out = [...win, v]
  while (out.length > cap) out.shift()
  return out
}

/**
 * One pass of the ladder.
 *
 * The order below is the Rust loop's order, and it is load-bearing in three places that
 * are easy to get wrong by rearranging:
 *
 *   1. The peak updates BEFORE the trailing stop reads it, so a new high ratchets the
 *      stop on the same arrival rather than one late.
 *   2. `exit_gate_met` is computed from the RAW pnl once, at the top, and every momentum
 *      and timing rule is gated on it. Recomputing it after a tier fires would let a
 *      partial sale change whether the remaining position may exit.
 *   3. The stop-loss and take-profit comparison is LAST, against the stop and target as
 *      every rule above has left them. Putting it first — which reads as "protective rule
 *      first, always" — would use a stop that this arrival's trailing ratchet had not yet
 *      raised, and give back the gain the ratchet exists to keep.
 */
export function evaluate(
  state: LadderState,
  arrival: Arrival,
  config: SniperConfig,
): { state: LadderState; decision: ExitDecision } {
  const value = arrival.valueLamports
  const entry = state.entryLamports
  let s: LadderState = { ...state, checks: state.checks + 1 }

  const pnlPct = ((value - entry) / entry) * 100
  const heldSecs = arrival.atUnix - state.openedAtUnix

  // The exit gate: block ALL momentum and timing exits below the initial take-profit, so
  // every profitable exit clears the floor. The hard stop-loss and the no-pump timeout are
  // exempt — they protect against rugs, and gating them would be gating the protection on
  // the profit.
  const exitGateMet = pnlPct >= config.takeProfitPct

  const done = (reason: ExitReason, detail: string): { state: LadderState; decision: ExitDecision } => ({
    state: s,
    decision: {
      exit: true, reason, detail, pnlPct,
      stopLossLamports: s.stopLossLamports,
      targetProfitLamports: s.targetProfitLamports,
      dynamicTpPct: s.dynamicTpPct,
      escalations: s.escalations,
      peakLamports: s.peakValue,
    },
  })

  // ── hard position time cap ─────────────────────────────────────────────────
  //
  // FIRST, as in Rust: it is the top of the loop body, before any price rule runs. Order
  // matters here because profit-first mode can extend the watch window indefinitely, and
  // this is what stops capital being locked in a dead pool by that extension. A rule that
  // exists to override the others cannot be evaluated after them.
  if (config.maxPositionHoldMins > 0 && Math.trunc(heldSecs / 60) >= config.maxPositionHoldMins) {
    return done('max_hold', `held ${Math.trunc(heldSecs / 60)} minutes — freeing capital`)
  }

  // ── decline streak, for the dump detector ──────────────────────────────────
  s.declineStreak = value < s.prevValue ? s.declineStreak + 1 : 0

  // ── vault-derived rules ────────────────────────────────────────────────────
  //
  // Both are disabled in the shipped config (`whale_exit_vault_drop_pct` and
  // `volume_exhaustion_pct` are 0.0) and both are ported anyway. A quote vault Zero could
  // not read is `null`, and a null must not act as a drop to zero — that is the
  // measured/unmeasured rule at its most expensive, since a failed read would otherwise
  // read as a hundred-percent vault drain and sell the position.
  const q = arrival.quoteVaultLamports ?? null
  if (q !== null) {
    if (s.entryQuote === 0) s.entryQuote = q

    if (
      config.whaleExitVaultDropPct > 0 &&
      s.prevQuote > 0 &&
      q < s.prevQuote &&
      (pnlPct < 0 || exitGateMet)
    ) {
      const dropPct = ((s.prevQuote - q) / s.prevQuote) * 100
      if (dropPct >= config.whaleExitVaultDropPct) {
        s.prevQuote = q
        s.prevValue = value
        return done('dump_detected', `quote vault fell ${dropPct.toFixed(1)}% in one check`)
      }
    }

    if (
      config.volumeExhaustionPct > 0 &&
      s.entryQuote > 0 &&
      value > entry &&
      exitGateMet
    ) {
      const floor = Math.trunc(s.entryQuote * (1.0 - config.volumeExhaustionPct / 100.0))
      if (q < floor) {
        s.prevQuote = q
        s.prevValue = value
        return done('volume_exhaustion', `quote vault ${q} below the exhaustion floor ${floor}`)
      }
    }
    s.prevQuote = q
  }

  // ── peak, then everything that reads it ────────────────────────────────────
  if (value > s.peakValue) s.peakValue = value

  // Profit floor: once the position reaches the initial take-profit, raise the stop to
  // that level so every subsequent exit is at least a take-profit-sized win.
  const minProfitFloor = Math.trunc(entry * (1.0 + config.takeProfitPct / 100.0))
  if (exitGateMet && s.stopLossLamports < minProfitFloor) {
    s.stopLossLamports = minProfitFloor
  }

  // Trailing stop: activates only at or above the take-profit, so it can never pull an
  // exit below entry. Wide by design — it is the backstop for a token still climbing past
  // the pullback threshold, not a second pullback rule.
  if (config.trailingStopLossPct > 0 && exitGateMet) {
    const trail = Math.trunc(s.peakValue * (1.0 - config.trailingStopLossPct / 100.0))
    if (trail > s.stopLossLamports) s.stopLossLamports = trail
  }

  // ── flash crash ────────────────────────────────────────────────────────────
  //
  // A single check showing a drop of `flash_crash_pct` of ENTRY (not of the previous
  // value) after at least three stabilising checks. Fires before the three-decline
  // counter can accumulate, which is what catches a vertical dump.
  if (
    config.flashCrashPct > 0 &&
    s.checks >= 3 &&
    s.prevValue > value &&
    (pnlPct < 0 || exitGateMet)
  ) {
    const dropPct = ((s.prevValue - value) / entry) * 100
    if (dropPct >= config.flashCrashPct) {
      s.prevValue = value
      return done('dump_detected', `flash crash: ${dropPct.toFixed(1)}% of entry in one check`)
    }
  }

  // ── profit lock ────────────────────────────────────────────────────────────
  //
  // After N consecutive checks above entry, raise the stop to near-breakeven so a
  // sustained winner cannot round-trip. Disabled in the shipped config (0).
  if (config.profitLockChecks > 0) {
    if (value > entry) {
      s.profitLockCounter += 1
      const floor = Math.trunc(entry * PROFIT_LOCK_FACTOR)
      if (s.profitLockCounter >= config.profitLockChecks && s.stopLossLamports < floor) {
        s.stopLossLamports = floor
      }
    } else {
      s.profitLockCounter = 0
    }
  }

  // ── momentum block ─────────────────────────────────────────────────────────
  if (config.momentumHold) {
    const deltaPct = ((value - s.prevValue) / entry) * 100
    s.velocityWindow = push(s.velocityWindow, deltaPct, config.momentumWindowChecks)
    const avgVelocity =
      s.velocityWindow.length === 0
        ? 0
        : s.velocityWindow.reduce((a, b) => a + b, 0) / s.velocityWindow.length

    const peakPnlPct = ((s.peakValue - entry) / entry) * 100
    const pullbackPct = peakPnlPct - pnlPct

    // (A) Take-profit escalation. One sample is enough, deliberately: a fast pump reaches
    //     the target on the first check, and waiting for a full window means the sell
    //     fires before escalation ever runs.
    const singleJump = pnlPct >= s.dynamicTpPct + 50.0
    const velocityOk = avgVelocity > config.momentumEscalationThresholdPct || singleJump
    if (
      pnlPct >= s.dynamicTpPct &&
      velocityOk &&
      s.escalations < config.momentumMaxEscalations &&
      s.velocityWindow.length > 0
    ) {
      s.dynamicTpPct = s.dynamicTpPct * config.momentumEscalationFactor
      s.escalations += 1
      // Refresh the target immediately, so the take-profit comparison at the BOTTOM of
      // this same pass uses the new threshold. Without this the bot escalates to 315% and
      // then sells at 175% on a stale target — a real bug, fixed in Rust, and exactly the
      // kind a port reintroduces by moving one line.
      s.targetProfitLamports = Math.trunc(entry * (1.0 + s.dynamicTpPct / 100.0))
    }

    // (B) Pullback from the peak, adaptive when enabled:
    //         θ_eff = base × sqrt(1 + peak/100)
    //     Bigger winners get more room before the exit fires, so a parabolic move is not
    //     dumped on a normal wiggle.
    const pullbackEff = config.adaptivePullback
      ? config.momentumPullbackExitPct * Math.sqrt(1.0 + Math.max(peakPnlPct, 0) / 100.0)
      : config.momentumPullbackExitPct
    if (exitGateMet && peakPnlPct >= config.momentumMinPeakPct && pullbackPct >= pullbackEff) {
      s.prevValue = value
      return done(
        'trailing_stop',
        `peaked at +${peakPnlPct.toFixed(1)}%, gave back ${pullbackPct.toFixed(1)}% ` +
        `(limit ${pullbackEff.toFixed(1)}%)`,
      )
    }

    // (C) Velocity decay — the momentum inflection, one or two ticks before the pullback
    //     or trailing stop would fire. Acts on the second derivative.
    //
    //     The window fills ONLY while the position is in profit past
    //     `velocity_decay_min_pnl_pct`, which is Rust's structure and not an optimisation:
    //     a window filled during a loss and read during a gain would compare velocities
    //     from two different regimes.
    if (config.velocityDecayExit && exitGateMet && pnlPct >= config.velocityDecayMinPnlPct) {
      const cap = config.velocityDecayWindow * 2
      s.decayWindow = push(s.decayWindow, deltaPct, cap)
      if (s.decayWindow.length === cap) {
        const half = config.velocityDecayWindow
        const prevHalf = s.decayWindow.slice(0, half).reduce((a, b) => a + b, 0) / half
        const recentHalf = s.decayWindow.slice(half).reduce((a, b) => a + b, 0) / half
        const drop = prevHalf - recentHalf
        // `prev_half_avg > 0` stops this firing after a recovery from a dip, where
        // velocity has "fallen" only because it was negative to begin with.
        if (drop >= config.velocityDecayDropThreshold && prevHalf > 0) {
          s.prevValue = value
          return done(
            'velocity_decay',
            `velocity fell ${drop.toFixed(2)} pts (${prevHalf.toFixed(2)} → ${recentHalf.toFixed(2)})`,
          )
        }
      }
    }
  }

  // ── no-pump timeout ────────────────────────────────────────────────────────
  //
  // Exempt from the exit gate — it protects capital rather than profit. Note what it
  // reads: the PEAK against `no_pump_min_gain_pct`, not the current PnL against a band.
  // "Never got above 8%" and "is within ±5% right now" are different questions, and only
  // the first one identifies a position that never went anywhere.
  //
  // Evaluated ON ARRIVAL with the elapsed time read from the event — never assumed to
  // have fired on schedule. A position that has gone quiet has unevaluated exits, which
  // `coherence.ts::liveness` reports rather than papering over.
  if (config.noPumpTimeoutSecs > 0 && heldSecs >= config.noPumpTimeoutSecs) {
    const peakPnlPct = ((s.peakValue - entry) / entry) * 100
    if (peakPnlPct < config.noPumpMinGainPct) {
      s.prevValue = value
      return done(
        'no_pump_timeout',
        `${Math.round(heldSecs)}s held and the peak only reached ` +
        `+${peakPnlPct.toFixed(2)}% (needs ${config.noPumpMinGainPct}%)`,
      )
    }
  }

  // ── tiered partial take-profit ─────────────────────────────────────────────
  //
  // Each level sells a fraction of the REMAINING position, and each fires at most once.
  // Only the first eligible level fires per arrival — Rust loops, but it also re-reads the
  // remaining balance between sales, which a single arrival here cannot do. Firing one
  // level per arrival is the conservative reading and is stated rather than silently
  // approximated.
  if (config.tieredPartialTp) {
    for (let i = 0; i < config.tieredPartialTpLevels.length; i++) {
      const [triggerPct, sellPct] = config.tieredPartialTpLevels[i]
      if (s.tieredFired[i] || pnlPct < triggerPct) continue
      s.tieredFired = s.tieredFired.map((f, j) => (j === i ? true : f))
      // Move the stop to breakeven after the FIRST tier fires.
      if (i === 0 && s.stopLossLamports < entry) s.stopLossLamports = entry
      s.prevValue = value
      return {
        state: s,
        decision: {
          exit: false,
          partial: {
            fraction: sellPct / 100,
            level: i + 1,
            detail: `tier ${i + 1} at +${pnlPct.toFixed(1)}% — selling ${sellPct}% of the remainder`,
          },
          detail: `tiered partial ${i + 1}`,
          pnlPct,
          stopLossLamports: s.stopLossLamports,
          targetProfitLamports: s.targetProfitLamports,
          dynamicTpPct: s.dynamicTpPct,
          escalations: s.escalations,
          peakLamports: s.peakValue,
        },
      }
    }
  }

  // ── dump detection ─────────────────────────────────────────────────────────
  //
  // Three consecutive declining checks after the fast phase. In profit-first mode this is
  // suppressed unless the position is already at or past the rug floor — otherwise the bot
  // bails on every dip and never books a win.
  const dumpEligible = config.profitFirstMode ? value <= s.stopLossLamports : true
  if (
    s.declineStreak >= 3 &&
    s.checks >= FAST_PHASE_CHECKS &&
    dumpEligible &&
    (pnlPct < 0 || exitGateMet)
  ) {
    s.prevValue = value
    return done('dump_detected', `${s.declineStreak} consecutive declining checks`)
  }

  // ── take-profit / stop-loss, last, against the ratcheted values ────────────
  if (value >= s.targetProfitLamports || value <= s.stopLossLamports) {
    const profitable = value >= s.targetProfitLamports
    s.prevValue = value
    return done(
      profitable ? 'take_profit' : 'stop_loss',
      profitable
        ? `${pnlPct.toFixed(1)}% reached the target +${s.dynamicTpPct.toFixed(0)}%`
        : `${pnlPct.toFixed(1)}% hit the stop at ${((s.stopLossLamports / entry - 1) * 100).toFixed(1)}%`,
    )
  }

  s.prevValue = value
  return {
    state: s,
    decision: {
      exit: false,
      detail:
        `holding at ${pnlPct.toFixed(1)}% (${Math.round(heldSecs)}s, ` +
        `target ${s.dynamicTpPct.toFixed(0)}%, ${s.escalations} escalations)`,
      pnlPct,
      stopLossLamports: s.stopLossLamports,
      targetProfitLamports: s.targetProfitLamports,
      dynamicTpPct: s.dynamicTpPct,
      escalations: s.escalations,
      peakLamports: s.peakValue,
    },
  }
}
