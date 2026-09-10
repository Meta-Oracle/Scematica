#!/usr/bin/env node
// Pin Scematica Zero's invariants. See docs/SCEMATICA-ZERO.md §4.
//
// Most of these cannot be tested any other way. A stop-loss firing, a spend cap refusing
// two simultaneous entries, a fill nobody could observe — reaching those against mainnet
// means losing money on purpose, repeatedly, at times you cannot schedule. The reducer is
// pure precisely so they are reachable here.
//
//   node --experimental-strip-types scripts/check-zero.mjs

import { existsSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

import { DEFAULT_CONFIG, configProblem, absent, measured, cell, coverage, DUST_LAMPORTS } from '../lib/zero/types.ts'
import { SNIPER_CONFIG, RATE_MODES, ACTIVE_MODE_NAME, withRateMode } from '../lib/zero/sniper/config.ts'
import {
  newCoherence, record as recordRead, recordPoolSeen, evaluate as evalCoherence, liveness,
  WINDOW_SECS, MIN_SAMPLES, FEED_STALL_SECS,
} from '../lib/zero/sniper/coherence.ts'
import { masterEquation, gateOf, GO_THRESHOLD, CAUTION_THRESHOLD, PSI_MAX } from '../lib/zero/sniper/psi.ts'
import {
  newLadder, evaluate as evalLadder, valueOf, effectiveStopLossPct,
  FAST_PHASE_CHECKS,
} from '../lib/zero/sniper/exit-ladder.ts'
import {
  kellySizer, computeMultiplier, DEFAULT_FRACTION, DEFAULT_MIN_TRADES, CLAMP_MIN, CLAMP_MAX,
} from '../lib/zero/sniper/kelly.ts'
import { scorePool } from '../lib/feed/scorer.ts'
import { FEATURES, NEUTRAL, encode, advise, POLICY_ID, MIN_ADVICE_COVERAGE } from '../lib/zero/policy.ts'
import {
  newLedger, authorise, settle, release, strand, committed, remaining, defaultCaps, LAMPORTS_PER_SOL,
} from '../lib/zero/session.ts'
import { mayTrade, leaseNote } from '../lib/zero/lease.ts'
import { interpret, pollPlan, BLOCKHASH_VALID_SECS } from '../lib/zero/observe.ts'
import { seal, verify, calibrate, COMMITTED_FIELDS } from '../lib/zero/seal.ts'
import { evaluateGate, REQUIRED_BASE_UNITS } from '../lib/zero/gatekeep.ts'
import { initialState, step, run, decideEntry } from '../lib/zero/engine.ts'
import { buildReadout, coverageMeter } from '../lib/zero/readout.ts'
import {
  isNewPool, decodeTokenAmount, priceFromVaults, vaultEvent, observePool,
  AMM_V4, vaultsFromPool, base58ToBytes, bytesToBase58,
} from '../lib/zero/host/parse.ts'
import { fillAmount } from '../lib/zero/host/fills.ts'
import { endpointProblem } from '../lib/zero/host/rpc.ts'
import { fundingPlan, sweepPlan, sweepBlockedBy, SWEEP_RESERVE_LAMPORTS } from '../lib/zero/funding.ts'


let failed = 0
const check = (name, ok) => {
  if (!ok) failed++
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}`)
}
const section = t => console.log(`\n── ${t} ${'─'.repeat(Math.max(0, 58 - t.length))}`)

/**
 * Source with comments removed.
 *
 * Every scan below asserts the absence of something the file's own header EXPLAINS the
 * absence of — `setInterval`, `trainStep`, `localStorage`. Scanning raw text makes each
 * one match its own documentation and fail, which is the kind of self-defeating check
 * that gets deleted rather than fixed. The scans must read the code, not the prose.
 */
const codeOf = path => readFileSync(path, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '')

const HERE = dirname(fileURLToPath(import.meta.url))
const REPO = join(HERE, '..', '..')

// ── fixtures ─────────────────────────────────────────────────────────────────

const pool = (over = {}) => ({
  mint: 'MintAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA',
  symbol: 'TEST',
  dev: 'DevAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA',
  createdAtUnix: 1_800_000_000,
  sizeSol: measured(45),
  ageSecs: measured(120),
  holderCount: measured(50),
  mintRenounced: measured(1),
  freezeDisabled: measured(1),
  devHoldingPct: measured(3),
  // quote_vault / base_vault, which for a real pump.fun graduation lands around 4e-5.
  // The old fixture used 1.4, four orders of magnitude out — harmless against a scorer
  // that ignored the field, and it would have hidden a broken buy-pressure ladder now
  // that the real one reads it.
  buyPressure: measured(0.00012),
  lpBurned: measured(1),
  ...over,
})

const T0 = 1_800_000_000
const caps = defaultCaps(T0)
const armed = (over = {}) => ({
  ...initialState(DEFAULT_CONFIG, caps, T0),
  armed: true,
  lease: 'writer',
  socketOpen: true,
  ...over,
})

// A held position, with the sell monitor's loop-locals attached.
//
// 1000 tokens bought for 0.01 SOL is an entry price of 1e-5 SOL/token, so the position is
// worth exactly `spentLamports` at that price and every percentage below reads directly.
const ENTRY_LAMPORTS = 10_000_000
const ENTRY_PRICE = 1e-5
const held = (over = {}) => ({
  mint: 'M', symbol: 'X', state: 'open', spentLamports: ENTRY_LAMPORTS,
  tokensOut: measured(1000), entryPriceSol: measured(ENTRY_PRICE),
  peakPriceSol: measured(ENTRY_PRICE), lastPriceSol: measured(ENTRY_PRICE),
  openedAtUnix: T0, lastEvaluatedUnix: T0,
  ladder: newLadder(ENTRY_LAMPORTS, T0, DEFAULT_CONFIG, null),
  ...over,
})
/** Drive one arrival through the ladder at a price. */
const arrive = (ladder, price, atUnix, config = DEFAULT_CONFIG, quote = null) =>
  evalLadder(ladder, { valueLamports: valueOf(1000, price), atUnix, quoteVaultLamports: quote }, config)

// ── Z-1: no timer may decide money ───────────────────────────────────────────

section('Z-1  no timer may decide money')

{
  // The headline assertion of the whole design. A `tick` in EVERY reachable state must
  // never produce a swap — because in a hidden tab a timer fires roughly once a minute
  // and a polled stop-loss silently stops checking a position it is still holding.
  const states = [
    initialState(DEFAULT_CONFIG, caps, T0),
    armed(),
    armed({ killed: true }),
    armed({ lease: 'follower' }),
    armed({ positions: { M: held({ peakPriceSol: measured(ENTRY_PRICE * 3), lastPriceSol: measured(ENTRY_PRICE * 0.1) }) } }),
  ]
  let sawSwap = false
  for (const s of states) {
    // A position deep past its stop-loss, its take-profit, its pullback and its timeout.
    // If any of them could fire on a clock, this is where it would.
    const { effects } = run(s, Array.from({ length: 2000 }, (_, i) => ({
      kind: 'tick', atUnix: 1_800_000_000 + i * 60,
    })))
    if (effects.some(e => e.kind === 'swap')) sawSwap = true
  }
  check('2000 ticks across every reachable state produce no swap', !sawSwap)

  // ...and the same position DOES exit the instant a real arrival carries the price.
  const holding = armed({ positions: { M: held() } })
  const arrival = step(holding, {
    kind: 'vault.changed', mint: 'M', priceSol: ENTRY_PRICE * 0.5, slot: 1, atUnix: T0 + 1,
  })
  check('the same position exits on a chain ARRIVAL', arrival.effects.some(e => e.kind === 'swap' && e.side === 'sell'))
}

// ── exits ────────────────────────────────────────────────────────────────────

section('exit ladder')

// Every threshold below comes from `sniper/config.ts`, which is pinned against Rust in
// the parity section. What is asserted here is ORDER and CONDITION -- the half the fixture
// cannot cover, because the Rust original is inline in an async loop.

{
  const L = () => newLadder(ENTRY_LAMPORTS, T0, DEFAULT_CONFIG, null)
  const cfg = DEFAULT_CONFIG

  // profit-first mode is ON in the shipped config, so a wallet Zero could not read takes
  // the WIDER rug-only floor (25%), not the 10% stop. That is the whole rule.
  check('an unreadable wallet takes the profit-first floor, not the tight stop',
    effectiveStopLossPct(cfg, null) === cfg.profitFirstFloorPct)
  check('a wallet above target takes the configured stop',
    effectiveStopLossPct(cfg, cfg.walletTargetSol + 1) === cfg.stopLossPct)
  check('a wallet below target takes the rug-only floor',
    effectiveStopLossPct(cfg, cfg.walletTargetSol / 2) === cfg.profitFirstFloorPct)

  const stopPrice = ENTRY_PRICE * (1 - cfg.profitFirstFloorPct / 100)
  check('stop-loss fires at the profit-first floor',
    arrive(L(), stopPrice * 0.99, T0 + 1).decision.reason === 'stop_loss')
  check('...and a shallower dip holds',
    !arrive(L(), ENTRY_PRICE * 0.9, T0 + 1).decision.exit)

  const tpPrice = ENTRY_PRICE * (1 + cfg.takeProfitPct / 100)
  {
    // Reaching the target on a SLOW climb takes profit; reaching it in one jump escalates.
    // The two paths differ only in how the position got there, which is the whole point of
    // the escalator — and it is why a first-arrival test cannot show a plain take-profit:
    // on arrival one, the delta from entry IS the entire gain, so velocity is always huge.
    let slow = L()
    let taken = null
    for (let i = 1; i <= 200 && taken === null; i++) {
      const r = arrive(slow, ENTRY_PRICE * (1 + i / 100), T0 + i)
      slow = r.state
      if (r.decision.exit) taken = r.decision.reason
    }
    check('take-profit fires at the target when momentum is not there', taken === 'take_profit')

    const jump = arrive(L(), ENTRY_PRICE * (1 + (cfg.takeProfitPct + 60) / 100), T0 + 1)
    check('a single jump past the target ESCALATES rather than selling',
      !jump.decision.exit && jump.decision.escalations === 1)
    check('...and the new target is the old one times the escalation factor',
      Math.abs(jump.decision.dynamicTpPct - cfg.takeProfitPct * cfg.momentumEscalationFactor) < 1e-9)
    // The bug the comment in `sniper.rs` exists for: escalating to 315% and then
    // immediately selling at 175% because the target was stale.
    check('...and the refreshed target is used on the SAME pass, never a stale one',
      jump.decision.targetProfitLamports > valueOf(1000, tpPrice))
  }

  check('escalation is capped', (() => {
    let l = L()
    for (let i = 0; i < 40; i++) {
      const px = ENTRY_PRICE * (1 + (l.dynamicTpPct + 60) / 100)
      const r = arrive(l, px, T0 + i + 1)
      if (r.decision.exit) break
      l = r.state
    }
    return l.escalations === cfg.momentumMaxEscalations
  })())

  {
    // The trailing stop only arms at or above the take-profit, and it RATCHETS. Both
    // halves matter: arming below the target could pull an exit under entry, and a stop
    // that un-ratchets on a dip is not a trailing stop at all.
    let l = L()
    l = arrive(l, ENTRY_PRICE * 4, T0 + 1).state
    const raised = l.stopLossLamports
    check('the trailing stop ratchets up above the take-profit', raised > ENTRY_LAMPORTS)
    l = arrive(l, ENTRY_PRICE * 3.5, T0 + 2).state
    check('...and never ratchets back down', l.stopLossLamports >= raised)

    const below = arrive(L(), ENTRY_PRICE * 1.5, T0 + 1).state
    check('...and does not arm below the take-profit gate',
      below.stopLossLamports === L().stopLossLamports)
  }

  {
    // The pullback exit needs THREE things: past the take-profit gate, a peak above
    // momentumMinPeakPct, and a give-back past the limit. The shipped config puts the
    // floor at 200% and the give-back at 15 points.
    let l = L()
    l = arrive(l, ENTRY_PRICE * 3.2, T0 + 1).state
    const d = arrive(l, ENTRY_PRICE * 3.0, T0 + 2).decision
    check('pullback fires once the peak clears the floor and the give-back exceeds the limit',
      d.reason === 'trailing_stop')

    let low = L()
    low = arrive(low, ENTRY_PRICE * 2.5, T0 + 1).state
    check('a peak below the momentum floor never arms pullback',
      arrive(low, ENTRY_PRICE * 2.0, T0 + 2).decision.reason !== 'trailing_stop')
  }

  {
    // The adaptive curve gives a bigger winner more room. Off in the shipped config and
    // ported anyway -- a rule that is off in one config is still a rule.
    const adaptive = { ...cfg, adaptivePullback: true }
    let l = newLadder(ENTRY_LAMPORTS, T0, adaptive, null)
    l = arrive(l, ENTRY_PRICE * 3.2, T0 + 1, adaptive).state
    // theta_eff at peak 220% is 15 * sqrt(3.2) ~ 26.8, so a 20-point give-back no longer fires.
    check('the adaptive curve widens the limit on a bigger peak',
      !arrive(l, ENTRY_PRICE * 3.0, T0 + 2, adaptive).decision.exit)
  }

  {
    // The no-pump timeout reads the PEAK against noPumpMinGainPct, not the current PnL
    // against a band. "Never got above 8%" and "is flat right now" are different
    // questions, and only the first identifies a position that went nowhere.
    const flat = arrive(L(), ENTRY_PRICE, T0 + cfg.noPumpTimeoutSecs)
    check('no-pump exits a position whose PEAK never cleared the gain floor',
      flat.decision.reason === 'no_pump_timeout')
    check('...and holds before the timeout',
      !arrive(L(), ENTRY_PRICE, T0 + cfg.noPumpTimeoutSecs - 1).decision.exit)

    let pumped = L()
    pumped = arrive(pumped, ENTRY_PRICE * 1.5, T0 + 1).state
    check('a position that pumped and returned is not a no-pump',
      arrive(pumped, ENTRY_PRICE, T0 + cfg.noPumpTimeoutSecs).decision.reason !== 'no_pump_timeout')
  }

  // Evaluated ON ARRIVAL, never fired by a clock: the elapsed time is read off the event.
  // Deliberately inside the hold cap, which is checked first and would otherwise be the
  // rule that fired.
  check('the timeout is measured from the arrival, not from a timer',
    arrive(L(), ENTRY_PRICE, T0 + 1000).decision.reason === 'no_pump_timeout')

  {
    // The hard hold cap is checked FIRST in Rust, before any price rule -- it exists to
    // override profit-first mode's window extension, and a rule that overrides the others
    // cannot be evaluated after them.
    let l = L()
    l = arrive(l, ENTRY_PRICE * 1.5, T0 + 1).state
    const d = arrive(l, ENTRY_PRICE * 1.5, T0 + cfg.maxPositionHoldMins * 60).decision
    check('the position time cap fires and outranks the price rules', d.reason === 'max_hold')
  }

  {
    // An unreadable quote vault must not read as a drain. Both vault rules are off in the
    // shipped config, so this is checked with one on.
    const whale = { ...cfg, whaleExitVaultDropPct: 30 }
    let l = newLadder(ENTRY_LAMPORTS, T0, whale, null)
    l = arrive(l, ENTRY_PRICE * 0.9, T0 + 1, whale, 100000000).state
    check('a vault drop past the threshold exits',
      arrive(l, ENTRY_PRICE * 0.9, T0 + 2, whale, 50000000).decision.reason === 'dump_detected')
    check('an UNREADABLE vault is not a drop to zero',
      !arrive(l, ENTRY_PRICE * 0.9, T0 + 2, whale, null).decision.exit)
  }

  {
    // The dump detector needs three consecutive declines AND the fast phase to be over.
    const noProfitFirst = { ...cfg, profitFirstMode: false }
    let l = newLadder(ENTRY_LAMPORTS, T0, noProfitFirst, null)
    let px = ENTRY_PRICE
    let fired = null
    for (let i = 0; i < FAST_PHASE_CHECKS + 5; i++) {
      px *= 0.995
      const r = arrive(l, px, T0 + i + 1, noProfitFirst)
      l = r.state
      if (r.decision.exit) { fired = r.decision.reason; break }
    }
    check('a sustained decline exits after the fast phase', fired !== null)

    let early = newLadder(ENTRY_LAMPORTS, T0, noProfitFirst, null)
    let epx = ENTRY_PRICE
    let earlyDump = false
    for (let i = 0; i < 5; i++) {
      epx *= 0.99
      const r = arrive(early, epx, T0 + i + 1, noProfitFirst)
      early = r.state
      if (r.decision.reason === 'dump_detected') earlyDump = true
    }
    check('...but not inside the fast phase', !earlyDump)
  }

  {
    // Tiered partials sell part and KEEP the position open. Marking it closing would make
    // the next arrival skip it, and the remainder would never be sold.
    const tiered = { ...cfg, tieredPartialTp: true, takeProfitPct: 100000 }
    const l = newLadder(ENTRY_LAMPORTS, T0, tiered, null)
    const d = arrive(l, ENTRY_PRICE * 2.5, T0 + 1, tiered).decision
    check('a tiered partial fires as a PARTIAL, not an exit',
      !d.exit && d.partial !== undefined && d.partial.level === 1)
    check('...selling a fraction of the remainder', d.partial.fraction === 0.15)
  }

  // The config relationship that has been broken in Rust before.
  check('the shipped config satisfies the pullback arithmetic', configProblem(DEFAULT_CONFIG) === null)
  check('a config whose pullback can never fire is reported',
    configProblem({ ...DEFAULT_CONFIG, momentumMinPeakPct: 100 }) !== null)

  // An unpriceable position has no ladder at all, and the engine refuses to act on it.
  check('a position with no ladder produces no swap',
    !step(armed({ positions: { M: held({ ladder: undefined }) } }),
      { kind: 'vault.changed', mint: 'M', priceSol: 1e-9, slot: 1, atUnix: T0 + 9999 },
    ).effects.some(e => e.kind === 'swap'))
  check('an unobserved fill produces no swap either',
    !step(armed({ positions: { M: held({ state: 'unknown' }) } }),
      { kind: 'vault.changed', mint: 'M', priceSol: 1e-9, slot: 1, atUnix: T0 + 9999 },
    ).effects.some(e => e.kind === 'swap'))

  {
    let l = L()
    l = arrive(l, ENTRY_PRICE * 3, T0 + 2).state
    const peak = l.peakValue
    l = arrive(l, ENTRY_PRICE * 1.5, T0 + 3).state
    check('the peak only ever rises', l.peakValue === peak)
  }
}

// ── the measured / unmeasured rule ───────────────────────────────────────────

section('unmeasured is not zero')

check('an unmeasured term renders as an em dash', cell(absent('x')) === '—')
check('a MEASURED zero renders as 0.00', cell(measured(0)) === '0.00')
check('coverage is a count, not a ratio', coverageMeter({ measuredCount: 2, total: 5 }) !== coverageMeter({ measuredCount: 4, total: 10 }))
check('an empty coverage is ∅, never an empty meter', coverageMeter({ measuredCount: 0, total: 0 }) === '∅')
{
  // An unscored pool is not a low-scoring one. The engine declines on absent depth with a
  // reason rather than handing the scorer a zero, which would be a real, terrible pool.
  const v = decideEntry(armed(), pool({ sizeSol: absent('depth not read') }), T0 + 100)
  check('an unscored pool is not a low-scoring one', !v.act)
  check('...and it says WHY rather than reporting a score',
    !v.score.measured && v.reason.includes('not scoreable'))
}
{
  // A pool age of `null` is a real arm of the Rust ladder -- almost every pump.fun
  // migration has open_time 0 -- and gets its own likelihood ratio rather than an error.
  // It must NOT score the same as a pool measured at zero seconds old.
  const unknownAge = scorePool({ sizeSol: 30, ageSecs: null })
  const zeroAge = scorePool({ sizeSol: 30, ageSecs: 0 })
  check('an unknown pool age is not an age of zero', unknownAge !== zeroAge)
}
{
  // Kelly's degenerate arms return a bare 1.0 rather than a computed number. A window with
  // no losses has no payoff ratio, and inventing one manufactures an edge from a missing
  // denominator.
  const k = kellySizer()
  const allWins = Array(10).fill({ profitable: true, pnlSol: 0.1 })
  check('a window with no losses has no payoff ratio and stays at base',
    computeMultiplier(k, allWins).multiplier === 1.0)
  check('...and says so rather than reporting an edge',
    computeMultiplier(k, allWins).reason.includes('no payoff ratio'))
  check('an empty history is base, not zero', computeMultiplier(k, []).multiplier === 1.0)
  check('a short history is the warm-up half, not a fitted number',
    computeMultiplier(k, Array(DEFAULT_MIN_TRADES - 1).fill({ profitable: true, pnlSol: 1 })).multiplier === 0.5)
}

// ── Ψ / coherence ────────────────────────────────────────────────────────────

section('coherence gate')

// The port that changed the most. Zero used to report the resolution rate AS psi; the real
// psi is the sentience master equation with the rate threaded through four of its terms,
// and it maxes out at ~0.2055 rather than 1.0. Every value below is pinned against Rust in
// the parity section; what is asserted here is the BEHAVIOUR around those values.

{
  let c = newCoherence(T0)
  const v0 = evalCoherence(c, T0)
  check('psi is UNMEASURED before the minimum samples', !v0.psi.measured)
  // The trap this repo has hit three times: an unmeasured gate that pins itself shut.
  // A gate that blocks the reads that would open it never opens.
  check('...and entries are ALLOWED while it is unmeasured', v0.entriesAllowed)
  check('...and the verdict is explicitly not decisive', !v0.decisive)

  // A healthy window: 90% of reads resolved, feed fresh.
  let good = newCoherence(T0)
  good = recordPoolSeen(good, T0)
  for (let i = 0; i < MIN_SAMPLES; i++) good = recordRead(good, i % 10 !== 0, T0)
  const vg = evalCoherence(good, T0)
  check('psi is measured once there are enough samples', vg.psi.measured)
  check('a healthy psi allows entries', vg.entriesAllowed && !vg.shouldHalt)
  check('...and psi is NOT the resolution rate',
    Math.abs(vg.psi.value - vg.resolutionRate.value) > 0.1)

  // A degraded window: 20% resolved.
  let bad = newCoherence(T0)
  bad = recordPoolSeen(bad, T0)
  for (let i = 0; i < MIN_SAMPLES; i++) bad = recordRead(bad, i % 5 === 0, T0)
  const vb = evalCoherence(bad, T0)
  check('a degraded psi halts entries', !vb.entriesAllowed && vb.gate === 'HOLD')
  check('...and says the pipeline is passing pools it could not verify',
    vb.reason.includes('could not verify'))

  // Only a HOLD halts. CAUTION is reported and not enforced -- a breaker that fired on
  // every yellow reading is one the operator turns off.
  check('a CAUTION gate does not halt', gateOf((GO_THRESHOLD + CAUTION_THRESHOLD) / 2) === 'CAUTION')
  check('psi cannot exceed the equation maximum', PSI_MAX < 1 && PSI_MAX > 0.2)

  // The breaker is a config flag, and it defaults ON in Rust for a stated reason.
  check('the breaker respects its config flag', evalCoherence(bad, T0, false).entriesAllowed)
  check('...and the shipped config has it on', DEFAULT_CONFIG.coherenceBreaker === true)

  // A 120-SECOND rolling window, not a sample count: a read arriving after a long silence
  // opens a new window rather than landing in a stale one.
  const rolled = recordRead(bad, true, T0 + WINDOW_SECS + 1)
  check('the window rolls on time, not on count', rolled.resolved + rolled.unresolved === 1)
  check('...and a rolled window is no longer decisive', !evalCoherence(rolled, T0 + WINDOW_SECS + 1).decisive)

  // A stalled feed degrades psi even when every read resolves, because a pipeline with no
  // pools to filter is not a healthy pipeline.
  let stalled = newCoherence(T0)
  stalled = recordPoolSeen(stalled, T0)
  for (let i = 0; i < MIN_SAMPLES; i++) stalled = recordRead(stalled, true, T0 + FEED_STALL_SECS + 1)
  const vs = evalCoherence(stalled, T0 + FEED_STALL_SECS + 1)
  check('a stalled feed halts even with perfect reads', vs.shouldHalt)
  check('...and names the stall rather than the read rate', vs.reason.includes('stalled'))

  // THE rule: degraded ENTRIES, never degraded exits.
  const s = armed({ coherence: bad, positions: { M: held() } })
  const out = step(s, {
    kind: 'vault.changed', mint: 'M',
    priceSol: ENTRY_PRICE * 0.5, slot: 1, atUnix: T0 + 2,
  })
  check('a degraded feed never stops an EXIT', out.effects.some(e => e.kind === 'swap' && e.side === 'sell'))
  // Checked WITHOUT the open position: `max_concurrent_positions` is 1 in the shipped
  // config, so a state holding one declines on that first and would pass this test for
  // entirely the wrong reason.
  check('...but does stop an ENTRY',
    decideEntry(armed({ coherence: bad }), pool(), T0 + 100).decline === 'coherence-degraded')
}

{
  const l = liveness(true, null, 100)
  check('a socket with no arrivals cannot claim exits are evaluated', !l.exitsEvaluable)
  check('a quiet socket reports its age as MEASURED', liveness(true, 0, 200).secsSinceArrival.measured)
  check('a closed socket reports the age as unmeasured, not 0', !liveness(false, 0, 200).secsSinceArrival.measured)
  check('a live socket is evaluable', liveness(true, 195, 200).exitsEvaluable)
}

// ── the session key ──────────────────────────────────────────────────────────

section('session key caps')

{
  const now = 1_800_000_000
  // The per-trade cap is deliberately below the budget in the shipped defaults, so a
  // single entry can never exhaust it. This fixture raises it to make the budget the
  // binding constraint in one step — the per-trade cap is asserted separately below.
  const c = { ...defaultCaps(now), cooldownSecs: 0, maxPerTradeLamports: LAMPORTS_PER_SOL }
  let led = newLedger()

  check('the SHIPPED caps make a single entry unable to exhaust the budget',
    defaultCaps(now).maxPerTradeLamports < defaultCaps(now).budgetLamports)

  // THE audit finding, in miniature: two entries decided against the same snapshot.
  // `authorise` returns a NEW ledger, so the only way to double-spend is to ignore it.
  const a = authorise(led, c, c.budgetLamports, now, true, 'a')
  check('a full-budget entry is authorised', a.ok)
  const b = authorise(a.ledger, c, 1, now, true, 'b')
  check('a second entry measured against the UPDATED ledger is refused', !b.ok && b.refusal === 'budget-exhausted')
  // And the failure mode if somebody reuses the stale ledger — pinned so the shape of
  // the bug is documented, not so the bug is allowed.
  const stale = authorise(led, c, c.budgetLamports, now, true, 'b')
  check('...whereas the STALE ledger would have allowed it (this is why authorise returns one)', stale.ok)

  check('committed counts reservations, not just settled spend', committed(a.ledger) === c.budgetLamports)
  check('remaining reaches zero on reservation alone', remaining(a.ledger, c) === 0)

  // Resolutions.
  const settled = settle(a.ledger, a.reservation.id, c.budgetLamports)
  check('settling discharges the reservation as it charges', committed(settled) === c.budgetLamports && settled.reserved.length === 0)
  check('settling twice is a no-op', committed(settle(settled, a.reservation.id, 999)) === c.budgetLamports)

  const released = release(a.ledger, a.reservation.id)
  check('an observed failure releases the hold', committed(released) === 0)

  const stranded = strand(a.ledger, a.reservation.id)
  check('an UNOBSERVED outcome keeps the hold', committed(stranded) === c.budgetLamports)
  check('...and is surfaced rather than swallowed', stranded.strandedIds.length === 1)

  // Other caps.
  const shipped = { ...defaultCaps(now), cooldownSecs: 0 }
  check('an over-cap trade is refused', authorise(led, shipped, shipped.maxPerTradeLamports + 1, now, true, 'x').refusal === 'over-per-trade-cap')
  check('an expired session signs nothing', authorise(led, c, 1, c.expiresAtUnix, true, 'x').refusal === 'expired')
  check('an unarmed session signs nothing', authorise(led, c, 1, now, false, 'x').refusal === 'not-armed')
  const cd = { ...c, cooldownSecs: 30 }
  const first = authorise(led, cd, 1000, now, true, 'x')
  check('the cooldown refuses a second entry inside the window', authorise(first.ledger, cd, 1000, now + 5, true, 'y').refusal === 'cooling-down')
  check('...and allows one after it', authorise(first.ledger, cd, 1000, now + 31, true, 'y').ok)
}

{
  // The one-word regression no behavioural test can catch: an `await` inside the
  // critical section lets the event loop interleave two entries between check and write.
  const src = codeOf(join(HERE, '..', 'lib', 'zero', 'session.ts'))
  const body = src.slice(src.indexOf('export function authorise'), src.indexOf('export function settle'))
  check('authorise contains no await — it is the whole critical section', !/\bawait\b/.test(body))
  check('...and is not async', !/export async function authorise/.test(src))
}

// ── multi-tab ────────────────────────────────────────────────────────────────

section('multi-tab lease')

check('only the writer may trade', mayTrade('writer') && !mayTrade('follower'))
// Assuming leadership on an unknown platform lets every tab trade against one budget.
check('an unsupported platform is NOT treated as the writer', !mayTrade('unsupported'))
check('a follower is told why, not left looking broken', leaseNote('follower').length > 40)
{
  const s = armed({ lease: 'follower', positions: { M: held() } })
  check('a follower tab declines entries', decideEntry(armed({ lease: 'follower' }), pool(), T0 + 100).decline === 'no-lease')
  // A follower must still track positions — the writer is the one acting on them — so the
  // arrival still updates the ladder and the displayed price.
  const out = step(s, { kind: 'vault.changed', mint: 'M', priceSol: ENTRY_PRICE * 1.5, slot: 1, atUnix: T0 + 2 })
  check('...but still ingests prices, so its display is live',
    out.state.positions.M.lastPriceSol.measured && out.state.positions.M.ladder.checks === 1)
}

// ── fills ────────────────────────────────────────────────────────────────────

section('fill observation')

check('a confirmed status lands', interpret('s', { slot: 1, confirmationStatus: 'confirmed', err: null }, 1, 90).outcome === 'landed')
check('a chain error is a real failure', interpret('s', { slot: 1, confirmationStatus: 'confirmed', err: { x: 1 } }, 1, 90).outcome === 'failed')
check('absence before the blockhash could expire is UNKNOWN, not failure', interpret('s', null, 5, 90).outcome === 'unknown')
check('absence long after expiry is a failure', interpret('s', null, 200, 90).outcome === 'failed')
check('a merely-processed status is not a landing', interpret('s', { slot: 1, confirmationStatus: 'processed', err: null }, 1, 90).outcome === 'unknown')
check('polling stops eventually rather than forever', !pollPlan(50, BLOCKHASH_VALID_SECS + 60).keepPolling)

{
  // Z-8 end to end: an unobserved fill keeps the hold, marks the position, and does NOT
  // retry. The treasury path paid for every clause of that.
  const s = armed({
    positions: { M: { ...held(), state: 'opening' } },
    holds: { M: 'hold-1' },
    ledger: { ...newLedger(), reserved: [{ id: 'hold-1', lamports: 1e7, atUnix: 0 }] },
  })
  const out = step(s, { kind: 'fill.unknown', mint: 'M', signature: 'SIG', atUnix: 1_800_000_005 })
  check('an unobserved fill marks the position unknown', out.state.positions.M.state === 'unknown')
  check('...keeps the budget reserved', committed(out.state.ledger) === 1e7)
  check('...never retries', !out.effects.some(e => e.kind === 'swap'))
  check('...and tells the operator, with the signature', out.effects.some(e => e.kind === 'notify' && e.text.includes('SIG')))
}
{
  const s = armed({
    positions: { M: { ...held(), state: 'opening' } },
    holds: { M: 'hold-1' },
    ledger: { ...newLedger(), reserved: [{ id: 'hold-1', lamports: 1e7, atUnix: 0 }] },
  })
  const out = step(s, { kind: 'fill.failed', mint: 'M', signature: 'SIG', reason: 'reverted' })
  check('an OBSERVED failure releases the hold', committed(out.state.ledger) === 0)
}

// ── policy ───────────────────────────────────────────────────────────────────

section('policy: pinned, never trained')

{
  const rust = readFileSync(join(REPO, 'crates', 'scematica-nn', 'src', 'state.rs'), 'utf8')

  // Parity with Rust, read from the Rust rather than restated — the same pin that caught
  // the Anchor account-order drift. A feature added there fails here rather than silently
  // shifting every subsequent index into a neural net.
  const block = rust.slice(rust.indexOf('pub const NEUTRAL'), rust.indexOf('];', rust.indexOf('pub const NEUTRAL')))
  const rows = [...block.matchAll(/^\s{4}(-?[\d.]+),\s*\/\/\s*(\w+)/gm)].map(m => [Number(m[1]), m[2]])
  check(`the Rust neutral table was parsed (${rows.length} rows)`, rows.length === NEUTRAL.length)
  check('every neutral value matches Rust', rows.every(([v], i) => v === NEUTRAL[i]))
  check('every feature name matches Rust, in order', rows.every(([, n], i) => n === FEATURES[i]))
  // The two that a blanket 0.5 would get catastrophically wrong.
  check('price_change_pct neutral is 0.0 — a 0% change, not +150%', NEUTRAL[FEATURES.indexOf('price_change_pct')] === 0)
  check('buy_sell_ratio neutral is 0.2 — a balanced book', NEUTRAL[FEATURES.indexOf('buy_sell_ratio')] === 0.2)

  const src = codeOf(join(HERE, '..', 'lib', 'zero', 'policy.ts'))
  check('the policy never trains', !/trainStep|\.train\(/.test(src))
  check('the checkpoint is pinned and named in every record', /POLICY_ID/.test(src) && POLICY_ID.includes('seed'))
}

{
  const e = encode({ initial_liquidity_sol: 50 })
  check('an absent feature is substituted, not zeroed', e.vector[FEATURES.indexOf('price_change_pct')] === NEUTRAL[FEATURES.indexOf('price_change_pct')])
  check('a present feature is encoded', Math.abs(e.vector[1] - 0.5) < 1e-9)
  check('coverage counts what was real', e.coverage.measuredCount === 1 && e.coverage.total === 24)
  // A NaN reaching the net makes every comparison false and silently selects action 0.
  check('a NaN is treated as ABSENT, never encoded', encode({ volatility: NaN }).unmeasured.includes('volatility'))

  const thin = advise({ initial_liquidity_sol: 50 })
  check('the policy is not consulted on a mostly-invented vector', thin.lean === 'neutral' && !thin.confidence.measured)
  check('...and says so rather than returning a confident lean', thin.reason.includes('features measured'))

  // Determinism is what makes POLICY_ID meaningful without shipping a weights file.
  const a = advise({ initial_liquidity_sol: 50, pool_score_norm: 0.8, buy_sell_ratio: 2, volatility: 0.3, price_change_pct: 0.2, lp_burned: 1, mint_renounced: 1, open_positions: 0, time_of_day_norm: 0.5, deployer_rug_rate: 0.1, pool_age_secs: 100, volume_5min_sol: 20, spread_pct: 0.01 })
  const b = advise({ initial_liquidity_sol: 50, pool_score_norm: 0.8, buy_sell_ratio: 2, volatility: 0.3, price_change_pct: 0.2, lp_burned: 1, mint_renounced: 1, open_positions: 0, time_of_day_norm: 0.5, deployer_rug_rate: 0.1, pool_age_secs: 100, volume_5min_sol: 20, spread_pct: 0.01 })
  check('the pinned checkpoint is deterministic', JSON.stringify(a.q) === JSON.stringify(b.q))
  check('coverage rides with the advice', a.coverage.measuredCount >= 12)
}

// ── strategies ───────────────────────────────────────────────────────────────

section('sniper parity: the numbers come from the bot')

// The heart of the whole rewrite.
//
// `fixtures/sniper-parity.json` is emitted by `cargo test -p scematica-sniper zero_parity`
// -- by the sniper itself, not typed out beside it. Every number Zero branches on is
// compared against it here. A threshold edited in `config.toml` fails the Rust test until
// the fixture is regenerated, and then fails this until the port follows.
//
// Before this existed, Zero's take-profit was 100 against the bot's 175, its stop 15
// against 10, its Kelly half against quarter, and its psi a different quantity entirely.
// Nothing failed, because there was nothing to fail.

const FIXTURE = JSON.parse(
  readFileSync(join(HERE, '..', 'lib', 'zero', 'fixtures', 'sniper-parity.json'), 'utf8'),
)

{
  // ── config, field for field ──────────────────────────────────────────────
  const camel = s => s.replace(/_([a-z])/g, (_, c) => c.toUpperCase())
  const rust = FIXTURE.config_toml
  const missing = []
  const differs = []
  for (const [k, v] of Object.entries(rust)) {
    const key = camel(k)
    if (!(key in SNIPER_CONFIG)) { missing.push(k); continue }
    const mine = SNIPER_CONFIG[key]
    const same = Array.isArray(v) ? JSON.stringify(v) === JSON.stringify(mine) : v === mine
    if (!same) differs.push(`${k}: rust=${JSON.stringify(v)} ts=${JSON.stringify(mine)}`)
  }
  check(`every sniper config field is present (${Object.keys(rust).length} fields)`,
    missing.length === 0 || (console.log('    missing:', missing.join(', ')), false))
  check('...and every one matches config.toml',
    differs.length === 0 || (differs.forEach(d => console.log('   ', d)), false))

  // The relationships that decide whether a rule can fire at all.
  check('the take-profit is the sniper’s 175, not a round 100',
    SNIPER_CONFIG.takeProfitPct === rust.take_profit_pct)
  check('the no-pump rule reads a PEAK threshold, which Zero used not to have at all',
    typeof SNIPER_CONFIG.noPumpMinGainPct === 'number' && SNIPER_CONFIG.noPumpMinGainPct > 0)
}

{
  // ── rate modes ───────────────────────────────────────────────────────────
  const rust = FIXTURE.rate_modes
  check(`all ${rust.length} rate modes are present`, RATE_MODES.length === rust.length)
  const bad = []
  rust.forEach((m, i) => {
    const mine = RATE_MODES[i]
    if (!mine) return bad.push(`${m.name}: absent`)
    for (const [rk, tk] of [
      ['name', 'name'], ['order', 'order'], ['quote_amount', 'quoteAmount'],
      ['wallet_pct', 'walletPct'], ['take_profit_pct', 'takeProfitPct'],
      ['stop_loss_pct', 'stopLossPct'], ['momentum_max_escalations', 'momentumMaxEscalations'],
      ['enabled', 'enabled'],
    ]) if (m[rk] !== mine[tk]) bad.push(`${m.name}.${rk}: rust=${m[rk]} ts=${mine[tk]}`)
  })
  check('...and every field of every mode matches',
    bad.length === 0 || (bad.forEach(b => console.log('   ', b)), false))
  check('the active mode is the bot’s', ACTIVE_MODE_NAME === FIXTURE.active_mode_name)

  // Applying a mode overrides four fields and no others. Notably NOT the momentum floor,
  // which is why the pullback rule is unsatisfiable in five of the seven shipped modes --
  // a real property of the bot, reported rather than corrected.
  const micro = withRateMode(SNIPER_CONFIG, 'Micro')
  check('a rate mode overrides the take-profit', micro.takeProfitPct === 50)
  check('...and leaves the momentum floor alone', micro.momentumMinPeakPct === SNIPER_CONFIG.momentumMinPeakPct)

  // Which means the high-take-profit modes carry an unsatisfiable pullback rule: the peak
  // floor stays at 200% while the take-profit gate climbs past it, so nothing can be both
  // past the gate and below the peak requirement. Aggressive, Degen and Moon are all in
  // that state in the shipped config. It is the bot's own behaviour and it is REPORTED
  // rather than corrected — silently rewriting a threshold to make a rule fire would be
  // Zero deciding to trade differently from the sniper, which is the whole thing this
  // rewrite removes.
  const unreachable = RATE_MODES
    .filter(m => configProblem(withRateMode(SNIPER_CONFIG, m.name)) !== null)
    .map(m => m.name)
  check(`the modes whose pullback can never fire are reported: ${unreachable.join(', ') || 'none'}`,
    unreachable.includes('Moon') && unreachable.includes('Degen') && unreachable.includes('Aggressive'))
  check('...and the shipped active mode is not one of them',
    !unreachable.includes(ACTIVE_MODE_NAME))
}

{
  // ── psi, the master equation, 72 cases ───────────────────────────────────
  const c = FIXTURE.coherence
  check('the coherence window matches Rust', WINDOW_SECS === c.window_secs)
  check('the minimum sample count matches Rust', MIN_SAMPLES === c.min_samples)
  check('the feed-stall threshold matches Rust', FEED_STALL_SECS === c.feed_stall_secs)
  check('the gate thresholds match Rust',
    GO_THRESHOLD === c.go_threshold && CAUTION_THRESHOLD === c.caution_threshold)

  let worst = 0
  let wrongGate = 0
  for (const k of c.cases) {
    const feedHealth = Math.min(1, Math.max(0, 1 - k.feed_age_secs / FEED_STALL_SECS))
    const { psi } = masterEquation(feedHealth, k.resolution_rate)
    worst = Math.max(worst, Math.abs(psi - k.psi))
    if (gateOf(psi) !== k.gate) wrongGate++
  }
  // Exact, not approximate. The multiplication order is Rust's, so the last bits agree --
  // and a tolerance here would hide exactly the reordering that a tolerance cannot detect.
  check(`psi is bit-exact across all ${c.cases.length} cases (max delta ${worst})`, worst === 0)
  check('...and every gate verdict matches', wrongGate === 0)
}

{
  // ── Kelly, around every discontinuity ────────────────────────────────────
  const k = FIXTURE.kelly
  check('the default Kelly fraction is Rust’s quarter, not a half', DEFAULT_FRACTION === k.default_fraction)
  check('the warm-up threshold is Rust’s ten, not eight', DEFAULT_MIN_TRADES === k.default_min_trades)
  check('the clamp rails match Rust', CLAMP_MIN === k.clamp_min && CLAMP_MAX === k.clamp_max)

  const bad = []
  for (const c of k.cases) {
    const history = c.history.map(([profitable, pnlSol]) => ({ profitable, pnlSol }))
    const got = computeMultiplier(kellySizer(c.fraction, c.min_trades), history).multiplier
    if (got !== c.multiplier) bad.push(`${c.name}: rust=${c.multiplier} ts=${got}`)
  }
  check(`every Kelly case reproduces Rust exactly (${k.cases.length} cases)`,
    bad.length === 0 || (bad.forEach(b => console.log('   ', b)), false))
}

{
  // ── the adaptive pullback curve ──────────────────────────────────────────
  const a = FIXTURE.adaptive_pullback
  const base = a.base_default
  const bad = []
  for (const c of a.cases) {
    const theta = base * Math.sqrt(1 + Math.max(c.peak_pnl_pct, 0) / 100)
    if (theta !== c.theta_eff) bad.push(`peak ${c.peak_pnl_pct}: rust=${c.theta_eff} ts=${theta}`)
  }
  check('the adaptive pullback curve is bit-exact',
    bad.length === 0 || (bad.forEach(b => console.log('   ', b)), false))
  check('the shipped base matches config.toml', SNIPER_CONFIG.momentumPullbackExitPct === a.base_toml)
}

{
  // ── the pool scorer ──────────────────────────────────────────────────────
  //
  // Zero used to carry its own heuristic here (`50 + mid*30`, plus eight points for a
  // renounced mint) and compare it against the sniper's 65 floor. It is now the same
  // parity-pinned port `/` uses, so this asserts the port rather than a second brain.
  const bad = []
  for (const c of FIXTURE.pool_score.cases) {
    const ratio = c.base_vault_raw > 0 ? (c.size_sol * 1e9) / c.base_vault_raw : undefined
    const got = scorePool({
      sizeSol: c.size_sol,
      ageSecs: c.age_secs,
      buyPressureRatio: ratio,
      pumpfunScore: c.pumpfun_score,
    })
    if (got !== c.score) bad.push(`${c.name}: rust=${c.score} ts=${got}`)
  }
  check(`the pool scorer reproduces Rust exactly (${FIXTURE.pool_score.cases.length} cases)`,
    bad.length === 0 || (bad.forEach(b => console.log('   ', b)), false))
}

{
  // ── no second implementation ─────────────────────────────────────────────
  //
  // The point of the whole exercise: the modules that used to hold Zero's own versions of
  // these rules are gone, and nothing may quietly grow a replacement. A scan, because a
  // convention is not a check.
  const zeroDir = join(HERE, '..', 'lib', 'zero')
  for (const gone of ['gate.ts', 'size.ts', 'strategy.ts', 'exits.ts']) {
    check(`lib/zero/${gone} is gone, not shadowing the port`, !existsSync(join(zeroDir, gone)))
  }
  const engine = codeOf(join(zeroDir, 'engine.ts'))
  check('the engine does not compute a pool score of its own',
    !/function\s+scorePool|50\s*\+\s*mid/.test(engine))
  check('the engine scores through the parity-pinned port',
    engine.includes("from '../feed/scorer.ts'"))
  check('the engine sizes through the ported Kelly',
    engine.includes("from './sniper/kelly.ts'"))
}

// ── sizing ───────────────────────────────────────────────────────────────────

section('sizing')

{
  // Sizing is the sniper's: `quote_amount` as the base, scaled by the Kelly multiplier and
  // the policy's, then clamped by the session caps. Two gates, different questions -- the
  // sizer says what the edge is worth, the ledger says what may be afforded.
  const base = Math.floor(DEFAULT_CONFIG.quoteAmount * 1e9)

  const v = decideEntry(armed(), pool(), T0 + 100)
  check('a passing pool sizes from the sniper’s quote_amount', v.act && v.sizeLamports === base)
  check('...and kelly_sizing is off in the shipped config, so the multiplier is 1',
    DEFAULT_CONFIG.kellySizing === false && v.reason.includes('kelly_sizing is off'))

  {
    // With Kelly on, a measured edge sizes UP -- which the old implementation could never
    // do, because it only ever scaled a base downward.
    const cfg = { ...DEFAULT_CONFIG, kellySizing: true }
    const outcomes = [
      ...Array(14).fill({ profitable: true, pnlSol: 0.30 }),
      ...Array(6).fill({ profitable: false, pnlSol: -0.05 }),
    ]
    const s = { ...armed({ outcomes }), config: cfg }
    const up = decideEntry(s, pool(), T0 + 100)
    check('a measured positive edge sizes ABOVE base', up.act && up.sizeLamports > base)

    const warm = { ...armed({ outcomes: outcomes.slice(0, 5) }), config: cfg }
    const half = decideEntry(warm, pool(), T0 + 100)
    check('a warm-up history sizes at half base', half.act && half.sizeLamports === Math.floor(base * 0.5))
  }

  {
    const tiny = { ...caps, maxPerTradeLamports: 3_000_000 }
    const s = { ...armed(), caps: tiny }
    check('the per-trade cap clamps', decideEntry(s, pool(), T0 + 100).sizeLamports === 3_000_000)
  }
  {
    const broke = { ...caps, budgetLamports: 1_000_000 }
    const s = { ...armed(), caps: broke }
    const d = decideEntry(s, pool(), T0 + 100)
    check('an exhausted budget declines rather than sending dust', !d.act && d.decline === 'budget-exhausted')
  }
  check('the dust floor is a host limit, not a sniper setting',
    DUST_LAMPORTS > 0 && !('dustLamports' in SNIPER_CONFIG))
}

// ── sealing ──────────────────────────────────────────────────────────────────

section('sealed records')

{
  const base = {
    schema: 'scema.zero.decision/1',
    atUnix: 1_800_000_000,
    mint: 'M',
    act: false,
    decline: 'policy-veto',
    reason: 'test',
    score: measured(72),
    psi: measured(0.9),
    coverage: { measuredCount: 12, total: 24 },
    sizeLamports: 0,
    q: [0.1, 0.2, 0.3, 0.4, 0.5],
    world: pool(),
    policyId: POLICY_ID,
  }
  const sealed = await seal(base)
  check('a record seals to a 64-hex commitment', /^[0-9a-f]{64}$/.test(sealed.commitment))
  check('an untouched record verifies', await verify(sealed.record, sealed.commitment))
  check('an edited reason does not', !(await verify({ ...sealed.record, reason: 'other' }, sealed.commitment)))
  check('an edited size does not', !(await verify({ ...sealed.record, sizeLamports: 1 }, sealed.commitment)))
  check('an edited world does not', !(await verify({ ...sealed.record, world: pool({ sizeSol: measured(46) }) }, sealed.commitment)))

  // THE distinction the whole system rests on: flipping measured/unmeasured must move
  // the digest even when the number is identical.
  const flipped = await seal({ ...base, score: { value: 72, measured: false } })
  check('a measured 72 and an unmeasured 72 seal differently', flipped.commitment !== sealed.commitment)
  const zeroM = await seal({ ...base, score: measured(0) })
  const zeroU = await seal({ ...base, score: absent('not read') })
  check('a measured zero and an unmeasured zero seal differently', zeroM.commitment !== zeroU.commitment)

  // A decline that is deleted must not hash like an action.
  const acted = await seal({ ...base, act: true, decline: undefined })
  check('an acted record and a declined one seal differently', acted.commitment !== sealed.commitment)

  // Every record field is covered by the commitment, or an edit to it is undetectable.
  const recordKeys = Object.keys(base).filter(k => k !== 'id')
  check('every record field is committed', recordKeys.every(k => COMMITTED_FIELDS.includes(k)))
}

{
  // Calibration: the rule that makes it honest.
  const c0 = calibrate([{ act: false, sizeLamports: 0 }, { act: false, sizeLamports: 0 }], [])
  check('a decline never resolves, so MAE is null and never 0.00', c0.meanAbsErrorPct === null)
  check('...and the declines are COUNTED', c0.declined === 2)
  const c1 = calibrate(
    [{ act: true, sizeLamports: 1e7 }, { act: true, sizeLamports: 1e7 }],
    [{ predictedPct: 10, realisedPct: 5 }, { predictedPct: 10, realisedPct: 15 }],
  )
  check('a constant-size policy is flagged as a base rate, not skill', c1.actionNeverVaried && c1.verdict.includes('base rate'))
}

// ── token gate ───────────────────────────────────────────────────────────────

section('token gate')

{
  const open = evaluateGate(REQUIRED_BASE_UNITS)
  check('holding the threshold opens arming', open.verdict === 'open' && open.mayArm)
  const short = evaluateGate('1000000')
  check('holding too little refuses arming', short.verdict === 'insufficient' && !short.mayArm)
  const unread = evaluateGate(null)
  // An RPC timeout is not a fact about the holder — the vault service's 503-not-403 rule.
  check('an unreadable balance is UNKNOWN, never insufficient', unread.verdict === 'unknown')
  check('...and says it is not an accusation', unread.reason.includes('could not ask'))
  check('...and still fails closed on arming', !unread.mayArm)
  check('reading is never gated', [open, short, unread].every(g => g.mayRead))
  check('an attended swap is never gated', [open, short, unread].every(g => g.mayExecuteAttended))
}

// ── the loop ─────────────────────────────────────────────────────────────────

section('engine')

{
  const s = armed()
  const out = step(s, { kind: 'pool.observed', pool: pool(), atUnix: 1_800_000_100 })
  check('a good pool produces a buy', out.effects.some(e => e.kind === 'swap' && e.side === 'buy'))
  check('...subscribes so exits become arrival-driven', out.effects.some(e => e.kind === 'subscribe'))
  check('...and seals a record', out.effects.some(e => e.kind === 'seal'))
  check('...reserving the budget in the same step', committed(out.state.ledger) > 0)

  const declined = step(s, { kind: 'pool.observed', pool: pool({ sizeSol: measured(2) }), atUnix: 1_800_000_100 })
  check('a rejected pool still seals a record', declined.effects.some(e => e.kind === 'seal'))
  check('...and moves no money', !declined.effects.some(e => e.kind === 'swap'))
  check('...naming which decline it was', declined.state.records[0].decline !== undefined)

  const unarmed = step(initialState(DEFAULT_CONFIG, caps, T0), { kind: 'pool.observed', pool: pool(), atUnix: 1_800_000_100 })
  check('an unarmed Zero signs nothing', !unarmed.effects.some(e => e.kind === 'swap'))

  const killed = step(armed({ killed: true }), { kind: 'pool.observed', pool: pool(), atUnix: 1_800_000_100 })
  check('the kill switch stops entries', !killed.effects.some(e => e.kind === 'swap'))
  // A kill switch that dumps at market is a different and far more dangerous control.
  const killStep = step(armed({ positions: { M: held() } }), { kind: 'kill', atUnix: 1_800_000_100 })
  check('the kill switch does NOT liquidate open positions', !killStep.effects.some(e => e.kind === 'swap'))
  check('...and its position keeps its exit rules', killStep.state.positions.M.state === 'open')

  // A dropped socket is not a reason to sell.
  const dropped = step(armed({ positions: { M: held() } }), { kind: 'socket.closed', atUnix: 1_800_000_100, reason: 'network' })
  check('a dropped feed never liquidates', !dropped.effects.some(e => e.kind === 'swap'))
  check('...but is reported as an alarm', dropped.effects.some(e => e.kind === 'notify' && e.level === 'alarm'))

  // One close, not two.
  const closing = armed({ positions: { M: held({ state: 'closing' }) } })
  const twice = step(closing, { kind: 'vault.changed', mint: 'M', priceSol: 0.1, slot: 1, atUnix: 1_800_000_200 })
  check('a position already closing is not sold twice', !twice.effects.some(e => e.kind === 'swap'))

  // Bounded memory: a tab may run for hours.
  const many = run(armed(), Array.from({ length: 900 }, (_, i) => ({
    kind: 'pool.observed', pool: pool({ mint: `M${i}`, sizeSol: measured(2) }), atUnix: 1_800_000_000 + i,
  })))
  check('records are bounded', many.state.records.length <= 500)
}

// ── readout ──────────────────────────────────────────────────────────────────

section('readout')

{
  const r = buildReadout(
    evalCoherence(newCoherence(T0), T0),
    liveness(false, null, 100),
    { armed: false, balanceLamports: absent('x'), committedLamports: 0, remainingLamports: 0, budgetLamports: 1e8, secsUntilExpiry: absent('not armed'), strandedCount: 0, warning: 'w' },
    'follower',
    evaluateGate(null),
    2,
  )
  check('open positions with a dead feed produce an ALARM headline', r.headline.role === 'alarm')
  check('...naming the count', r.headline.text.includes('2 position'))
  const unmeasuredGauges = r.gauges.filter(g => g.role === 'unmeasured')
  check('an unmeasured gauge has a null fill, not a zero one', unmeasuredGauges.every(g => g.fill === null))
  check('...and renders an em dash', unmeasuredGauges.every(g => g.text === '—'))
  check('an unmeasured Ψ is explained rather than shown as zero', r.notes.some(n => n.includes('not a Ψ of zero')))
}

// ── source discipline ────────────────────────────────────────────────────────

section('core purity')

{
  // Every module of the pure core, including the ported sniper. The ports are held to the
  // same rule as the rest: they must run in a page and in an extension's offscreen
  // document without change, which is the only reason phase 3 is a new shell rather than a
  // rewrite.
  const files = [
    'types', 'policy', 'session', 'observe', 'seal', 'gatekeep', 'engine', 'readout',
    'lease', 'funding',
    'sniper/psi', 'sniper/coherence', 'sniper/kelly', 'sniper/config', 'sniper/exit-ladder',
  ]
  const src = Object.fromEntries(files.map(f => [f, codeOf(join(HERE, '..', 'lib', 'zero', `${f}.ts`))]))

  // The core must be hostable by a page AND an extension offscreen document without
  // change, or phase 3 is a rewrite rather than a new shell.
  const impure = files.filter(f => /\bdocument\.|\bwindow\.|chrome\.\w|\bfetch\(|localStorage|sessionStorage/.test(src[f]))
  check(`the core touches no DOM, no chrome.*, no fetch, no storage${impure.length ? ` — ${impure.join(', ')}` : ''}`, impure.length === 0)

  // Z-1 as a source rule as well as a behavioural one: the money path may not schedule.
  const timerful = ['engine', 'exits', 'strategy', 'session', 'policy', 'size']
    .filter(f => /setInterval|setTimeout|requestAnimationFrame/.test(src[f]))
  check(`no timer appears anywhere in the decision path${timerful.length ? ` — ${timerful.join(', ')}` : ''}`, timerful.length === 0)

  // Zero must have no path by which a simulated figure reaches a decision. The DQ* net
  // is imported from lib/sim/dqstar, which is arithmetic; lib/sim/engine is the fabricator.
  const simmed = files.filter(f => /sim\/engine/.test(src[f]))
  check(`the core never imports the simulation engine${simmed.length ? ` — ${simmed.join(', ')}` : ''}`, simmed.length === 0)
}


section('host: parsing an arrival')

{
  const tokenAccount = amount => {
    const b = new Uint8Array(165)
    new DataView(b.buffer).setBigUint64(64, BigInt(amount), true)
    return b
  }

  check('a token amount decodes from offset 64', decodeTokenAmount(tokenAccount(12345n)) === 12345n)
  // A short buffer is not an empty account. Returning 0 would price a position on it.
  check('a truncated account decodes to null, never 0', decodeTokenAmount(new Uint8Array(40)) === null)
  // Token-2022 accounts carry extensions and exceed 165 bytes; rejecting them on an
  // equality check would make every Token-2022 position unpriceable.
  check('a Token-2022 account with extensions still decodes', decodeTokenAmount(new Uint8Array(300)) === 0n)

  check('a price needs both legs', priceFromVaults(100n, null) === null && priceFromVaults(null, 100n) === null)
  // An empty base vault makes the price infinite, and an infinite price arriving at
  // evaluateExit reads as a take-profit.
  check('an empty base vault yields no price, not an infinite one', priceFromVaults(100n, 0n) === null)
  check('two legs yield a price', priceFromVaults(200n, 100n) === 2)

  check('an unpriceable arrival produces NO event', vaultEvent('M', tokenAccount(100n), null, 1, 2) === null)
  check('a priceable arrival produces one', vaultEvent('M', tokenAccount(200n), tokenAccount(100n), 1, 2)?.priceSol === 2)

  check('a failed initialize2 is not a pool', !isNewPool({ signature: 's', err: { x: 1 }, logs: ['initialize2'] }))
  check('a successful one is', isNewPool({ signature: 's', err: null, logs: ['Program log: initialize2'] }))
  check('a swap is not', !isNewPool({ signature: 's', err: null, logs: ['Program log: ray_log'] }))
}

{
  // THE boundary where "the feed did not say" would otherwise become "the value is zero".
  const o = observePool({ mint: 'M', sizeSol: 40 }, 'test-feed')
  check('a provided field is measured', o.sizeSol.measured && o.sizeSol.value === 40)
  check('an omitted field is ABSENT, not zero', !o.ageSecs.measured)
  check('...and names the source that did not provide it', o.ageSecs.note.includes('test-feed'))
  // The two safest readings of the two strongest safety signals must never be assumed.
  check('an unstated mint authority is not "renounced"', !o.mintRenounced.measured)
  check('an unstated LP burn is not "burned"', !o.lpBurned.measured)
  check('a NaN is absent rather than encoded', !observePool({ mint: 'M', sizeSol: NaN }, 'f').sizeSol.measured)
}

section('host: the RPC key never escapes')

{
  const rpcSrc = codeOf(join(HERE, '..', 'lib', 'zero', 'host', 'rpc.ts'))
  // An endpoint URL with ?api-key= in it is the likeliest way for a secret to escape a
  // page that is otherwise careful, so every message that can reach a log or a record
  // goes through redact().
  check('errors are redacted before they can reach a notice', /redact\(/.test(rpcSrc))
  check('the key is never posted to our own origin', !/'\/api\//.test(rpcSrc))
}


section('host: raydium layout and base58')

{
  // A wrong offset yields a valid-looking pubkey belonging to a different account, and
  // Zero would price every exit against a balance from another pool. It does not throw.
  const wsol = base58ToBytes('So11111111111111111111111111111111111111112')
  check('base58 round-trips a mint', bytesToBase58(wsol) === 'So11111111111111111111111111111111111111112')
  check('base58 preserves leading zero bytes', bytesToBase58(base58ToBytes('1111111111111111111111111111111')).length === 31)

  const pool = (baseMint, quoteMint) => {
    const d = new Uint8Array(752)
    d.set(new Uint8Array(32).fill(0xa1), AMM_V4.BASE_VAULT)
    d.set(new Uint8Array(32).fill(0xb2), AMM_V4.QUOTE_VAULT)
    d.set(baseMint, AMM_V4.BASE_MINT)
    d.set(quoteMint, AMM_V4.QUOTE_MINT)
    return d
  }
  const token = new Uint8Array(32).fill(0x0c)

  const normal = vaultsFromPool(pool(token, wsol), wsol)
  check('a SOL-quoted pool keeps its orientation', normal?.quoteVault[0] === 0xb2 && normal?.baseVault[0] === 0xa1)

  // Raydium does not guarantee which leg is SOL. Assuming it inverts the price on half
  // of all pools, and an inverted price makes every exit rule fire backwards.
  const inverted = vaultsFromPool(pool(wsol, token), wsol)
  check('a SOL-BASED pool has its legs swapped, not assumed', inverted?.quoteVault[0] === 0xa1 && inverted?.baseVault[0] === 0xb2)

  check('a pool with no SOL leg is refused rather than priced in an unknown unit',
    vaultsFromPool(pool(token, token), wsol) === null)
  check('a short account is refused', vaultsFromPool(new Uint8Array(100), wsol) === null)
}


section('host: reading what actually filled')

{
  const OWNER = 'Owner11111111111111111111111111111111111111'
  const MINT = 'Mint111111111111111111111111111111111111111'
  const bal = (owner, mint, amount) => ({
    accountIndex: 1, mint, owner, uiTokenAmount: { amount: String(amount), decimals: 6 },
  })
  const meta = over => ({
    fee: 5000, err: null,
    preBalances: [1_000_000_000, 0], postBalances: [900_000_000, 0],
    preTokenBalances: [], postTokenBalances: [],
    ...over,
  })
  const keys = [OWNER, 'Other11111111111111111111111111111111111111']

  // A first buy has no PRE entry: the account did not exist. That is a genuine zero, not
  // an unknown — getting it backwards makes every first purchase unpriceable.
  check('a first buy with no pre-balance reads the full post amount',
    fillAmount(meta({ postTokenBalances: [bal(OWNER, MINT, 5000)] }), keys, OWNER, MINT, 'buy') === 5000)
  check('a top-up reads the delta, not the total',
    fillAmount(meta({
      preTokenBalances: [bal(OWNER, MINT, 2000)],
      postTokenBalances: [bal(OWNER, MINT, 5000)],
    }), keys, OWNER, MINT, 'buy') === 3000)

  // The distinction the whole design rests on: unreadable is not zero.
  check('a missing POST entry is UNREADABLE, never a zero fill',
    fillAmount(meta({}), keys, OWNER, MINT, 'buy') === null)
  check('a fill of zero tokens is a MEASURED zero',
    fillAmount(meta({ postTokenBalances: [bal(OWNER, MINT, 0)] }), keys, OWNER, MINT, 'buy') === 0)
  check('another wallet\'s balance is not our fill',
    fillAmount(meta({ postTokenBalances: [bal('Someone', MINT, 5000)] }), keys, OWNER, MINT, 'buy') === null)
  check('another mint\'s balance is not our fill',
    fillAmount(meta({ postTokenBalances: [bal(OWNER, 'OtherMint', 5000)] }), keys, OWNER, MINT, 'buy') === null)

  // A reverted transaction moved nothing; reading its balances reports the fee as a fill.
  check('a reverted transaction has no fill',
    fillAmount(meta({ err: { x: 1 }, postTokenBalances: [bal(OWNER, MINT, 5000)] }), keys, OWNER, MINT, 'buy') === null)

  // Base units past 2^53 lose precision as a JS number, and a wrong tokensOut becomes a
  // wrong entry price and every exit after it.
  check('a token amount past MAX_SAFE_INTEGER is refused rather than rounded',
    fillAmount(meta({ postTokenBalances: [bal(OWNER, MINT, '9007199254740993')] }), keys, OWNER, MINT, 'buy') === null)

  // Sells are measured in lamports, net of the fee: the fee is money that left on this
  // trade, and adding it back reports proceeds nobody received.
  check('a sell reads the lamport delta',
    fillAmount(meta({ preBalances: [100, 0], postBalances: [900, 0] }), keys, OWNER, MINT, 'sell') === 800)
  check('an owner absent from the account keys has no readable fill',
    fillAmount(meta({}), keys, 'NotInTx', MINT, 'sell') === null)
  // jsonParsed returns objects where json returns strings; both must work.
  check('jsonParsed account keys resolve',
    fillAmount(meta({ preBalances: [100, 0], postBalances: [900, 0] }), [{ pubkey: OWNER }], OWNER, MINT, 'sell') === 800)
}

{
  // End to end through the reducer: a landed buy whose output cannot be read must NOT
  // become a priced position. It becomes one Zero holds and refuses to trade.
  const opening = armed({
    positions: { M: { ...held(), state: 'opening', tokensOut: absent('pending'), entryPriceSol: absent('pending') } },
    holds: { M: 'h1' },
    ledger: { ...newLedger(), reserved: [{ id: 'h1', lamports: 1e7, atUnix: 0 }] },
  })

  const unreadable = step(opening, {
    kind: 'fill.observed', mint: 'M', signature: 'S', outAmount: null, side: 'buy', atUnix: 1_800_000_005,
  })
  check('a landed buy with an unreadable fill becomes UNKNOWN, not open', unreadable.state.positions.M.state === 'unknown')
  check('...with no invented entry price', !unreadable.state.positions.M.entryPriceSol.measured)
  check('...and the operator is told', unreadable.effects.some(e => e.kind === 'notify' && e.level === 'alarm'))
  // And it gets no ladder at all, which is the payoff of the whole distinction: a ladder
  // needs an entry VALUE, and a position whose fill nobody could read has none. Every
  // percentage rule would otherwise fire against a number nobody measured.
  check('...and is given no exit ladder', unreadable.state.positions.M.ladder === undefined)
  check('...so no exit rule can fire on it',
    !step(unreadable.state, {
      kind: 'vault.changed', mint: 'M', priceSol: 1e-9, slot: 1, atUnix: T0 + 99999,
    }).effects.some(e => e.kind === 'swap'))

  const readable = step(opening, {
    kind: 'fill.observed', mint: 'M', signature: 'S', outAmount: 5_000_000, side: 'buy', atUnix: 1_800_000_005,
  })
  check('a readable fill opens the position', readable.state.positions.M.state === 'open')
  check('...with an entry price derived from what arrived',
    Math.abs(readable.state.positions.M.entryPriceSol.value - 1e7 / 5_000_000) < 1e-12)
  // The ladder anchors to what was ACTUALLY spent, not to what was requested.
  check('...and a ladder anchored to the lamports actually spent',
    readable.state.positions.M.ladder.entryLamports === readable.state.positions.M.spentLamports)

  // A sell whose proceeds cannot be read produces NO outcome. A fabricated 0% would enter
  // the edge estimate and size every subsequent trade.
  const closing = armed({ positions: { M: held({ state: 'closing' }) } })
  const blindSell = step(closing, {
    kind: 'fill.observed', mint: 'M', signature: 'S', outAmount: null, side: 'sell', atUnix: 1_800_000_009,
  })
  check('an unreadable sell records no outcome', blindSell.state.outcomes.length === 0)
  const goodSell = step(closing, {
    kind: 'fill.observed', mint: 'M', signature: 'S', outAmount: 2e7, side: 'sell', atUnix: 1_800_000_009,
  })
  check('a readable sell records a realised outcome', goodSell.state.outcomes.length === 1)
  // In SOL and with a separate win/loss verdict, because that is what `KellySizer` reads.
  // A percentage agrees with SOL only while every position is the same size, which is
  // exactly what Kelly sizing stops being true.
  check('...computed from the observed proceeds, in SOL',
    Math.abs(goodSell.state.outcomes[0].pnlSol - (2e7 - ENTRY_LAMPORTS) / 1e9) < 1e-12)
  check('...carrying the win/loss verdict separately', goodSell.state.outcomes[0].profitable === true)
  {
    const lossSell = step(closing, {
      kind: 'fill.observed', mint: 'M', signature: 'S', outAmount: 5e6, side: 'sell', atUnix: T0 + 9,
    })
    check('...and a loss is marked as one', lossSell.state.outcomes[0].profitable === false)
  }
  {
    // Bounded to the sniper's own lookback: an unbounded history makes the multiplier a
    // claim about the whole session rather than about recent trades. The same mistake the
    // NN tournament made with a lifetime reward sum.
    let s = closing
    for (let i = 0; i < DEFAULT_CONFIG.kellyLookback + 20; i++) {
      s = step({ ...s, positions: { M: held({ state: 'closing' }) } }, {
        kind: 'fill.observed', mint: 'M', signature: 'S', outAmount: 2e7, side: 'sell', atUnix: T0 + 9 + i,
      }).state
    }
    check('settled outcomes are bounded to the sniper’s kelly_lookback',
      s.outcomes.length === DEFAULT_CONFIG.kellyLookback)
  }
}

section('funding the session key')

{
  const CAP = 500_000_000

  const ok = fundingPlan(0, 100_000_000, CAP, 1_000_000_000)
  check('a first funding within the cap is allowed', ok.ok && ok.lamports === 100_000_000)

  // The cap is on the RESULTING balance, so repeated top-ups cannot walk past it one
  // increment at a time — the same shape as the spend ledger's `committed`.
  const walk = fundingPlan(450_000_000, 100_000_000, CAP, 1_000_000_000)
  check('a top-up that would exceed the cap is refused', !walk.ok && walk.refusal === 'over-balance-cap')
  check('...and says how much room is left', !walk.ok && walk.detail.includes('50000000'))
  check('a top-up filling exactly the cap is allowed', fundingPlan(400_000_000, 100_000_000, CAP, 1e9).ok)

  check('a dust top-up is refused', fundingPlan(0, 1000, CAP, 1e9).refusal === 'below-minimum')
  // A funder who cannot cover the fee as well produces a transaction that fails after the
  // wallet has already been asked to sign.
  check('a funder who cannot cover amount plus fee is refused',
    fundingPlan(0, 100_000_000, CAP, 100_000_000).refusal === 'insufficient-funder')
  check('no connected wallet is its own refusal', fundingPlan(0, 100_000_000, CAP, null).refusal === 'no-funder')

  const sweep = sweepPlan(100_000_000)
  check('a sweep returns everything but the fee reserve', sweep.ok && sweep.lamports === 100_000_000 - SWEEP_RESERVE_LAMPORTS)
  check('an empty key has nothing to sweep', sweepPlan(0).refusal === 'nothing-to-sweep')
  // A sweep that under-reserves fails, and a failed sweep leaves the whole balance in a
  // key the operator has already decided to stop trusting.
  check('a balance below the reserve is refused rather than attempted',
    sweepPlan(SWEEP_RESERVE_LAMPORTS - 1).refusal === 'nothing-to-sweep')
  check('a balance exactly at the reserve is refused too', sweepPlan(SWEEP_RESERVE_LAMPORTS).refusal === 'nothing-to-sweep')

  // Sweeping the SOL out from under an open position leaves it owned by a key that cannot
  // pay a sell fee.
  check('a sweep with open positions is blocked, with a reason', (sweepBlockedBy(2) ?? '').includes('sell fee'))
  check('a sweep with none is not blocked', sweepBlockedBy(0) === null)
}

{
  // The destination is read from storage, never taken as an argument — a sweep that took
  // an address would be a one-click drain of the hot key to anywhere.
  const src = codeOf(join(HERE, '..', 'lib', 'zero', 'host', 'treasury.ts'))
  const body = src.slice(src.indexOf('export async function sweepSession'))
  check('sweepSession takes no destination address', !/to:\s*\w+Address|destination/i.test(body))
  check('...and refuses when no funder is on record', /no funding wallet on record/.test(body))
}


{
  // The preview must refuse exactly where the payer refuses. The escrow path paid for
  // this once: `quote` swallowed the ledger read's throw and priced a claim against an
  // empty ledger, while `settle` refused the identical request.
  const panel = codeOf(join(HERE, '..', 'components', 'zero', 'SessionPanel.tsx'))
  check('the funding preview does not default an unread balance to zero',
    !/fundingPlan\(balance \?\? 0/.test(panel))
  check('...and refuses on an unread balance instead', /balance === null/.test(panel))
  // Guessing the funder's balance would make the button promise what the transfer refuses.
  check('the funder balance is read, not assumed', !/Number\.MAX_SAFE_INTEGER/.test(panel))

  const treasury = codeOf(join(HERE, '..', 'lib', 'zero', 'host', 'treasury.ts'))
  check('funding refuses against an unread session balance',
    /sessionBalance === null/.test(treasury))
}


section('host: the endpoint is validated, not trusted')

{
  // Found in the wild. An npm token was pasted into the endpoint field; `wsUrl` only
  // rewrites a leading `http`, so it passed through unchanged, and a bare string is a
  // RELATIVE WebSocket url — the browser resolved it against the page origin and
  // reconnected to `wss://www.scematica.org/npm_...` on a backoff forever, printing the
  // token into the console every time. Nothing left the browser; everything else about
  // it was wrong.
  // A SYNTHETIC token, shaped like the real thing and belonging to nobody.
  //
  // The first version of this test pasted in the actual token from the report, which put
  // a live credential in the repository and in git history — turning a value that had
  // only ever been in one browser into one that is committed. A fixture never needs the
  // real secret: `endpointProblem` matches on the prefix, so the shape is the whole test.
  const TOKEN = 'npm_exampleTokenNotRealDoNotUse000000000'
  check('a bare token is refused', endpointProblem(TOKEN) !== null)
  // ...and named as a token, because "that is not a URL" does not tell somebody their
  // credential is now on their clipboard and in their console.
  check('...and identified as a token, with the advice to rotate it',
    /token/i.test(endpointProblem(TOKEN)) && /rotate/i.test(endpointProblem(TOKEN)))
  check('other common credential shapes are caught too',
    ['ghp_abc', 'sk-abc', 'xoxb-abc', 'AKIAIOSFODNN7EXAMPLE', 'glpat-abc']
      .every(t => endpointProblem(t) !== null))

  check('a bare host with no scheme is refused', endpointProblem('mainnet.helius-rpc.com') !== null)
  check('a ws:// endpoint is refused — the http one is what is stored',
    endpointProblem('wss://x.example/y') !== null)
  check('an empty endpoint is refused', endpointProblem('  ') !== null)
  check('a real endpoint passes', endpointProblem('https://mainnet.helius-rpc.com/?api-key=abc') === null)
  check('surrounding whitespace is tolerated — people paste with it',
    endpointProblem('  https://x.example/  ') === null)

  // The reconnect loop is only reachable through a value that got past the check, so the
  // check has to run on the way OUT of storage too — a value saved before it existed, or
  // written by hand, must not resurrect it.
  const src = codeOf(join(HERE, '..', 'lib', 'zero', 'host', 'rpc.ts'))
  const load = src.slice(src.indexOf('export function loadRpc'), src.indexOf('export function saveRpc'))
  check('loadRpc validates what it reads back', /endpointProblem/.test(load))
  check('...and drops an unusable one rather than returning it', /removeItem/.test(load))
  const save = src.slice(src.indexOf('export function saveRpc'))
  check('saveRpc refuses rather than storing', /endpointProblem/.test(save))
}

console.log(`\n${failed === 0 ? 'ALL PASS' : `${failed} FAILED`}`)
process.exit(failed === 0 ? 0 : 1)
