#!/usr/bin/env node
// Pin Scematica Zero's invariants. See docs/SCEMATICA-ZERO.md §4.
//
// Most of these cannot be tested any other way. A stop-loss firing, a spend cap refusing
// two simultaneous entries, a fill nobody could observe — reaching those against mainnet
// means losing money on purpose, repeatedly, at times you cannot schedule. The reducer is
// pure precisely so they are reachable here.
//
//   node --experimental-strip-types scripts/check-zero.mjs

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

import { DEFAULT_CONFIG, configProblem, absent, measured, cell, coverage } from '../lib/zero/types.ts'
import { newCoherence, record as recordRead, evaluate as evalCoherence, liveness } from '../lib/zero/gate.ts'
import { FEATURES, NEUTRAL, encode, advise, POLICY_ID, MIN_ADVICE_COVERAGE } from '../lib/zero/policy.ts'
import { pushPrice, pullback, continuation, scoredEntry, changePct } from '../lib/zero/strategy.ts'
import { evaluateExit, applyPrice, pnlPct } from '../lib/zero/exits.ts'
import { estimateEdge, sizeEntry } from '../lib/zero/size.ts'
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
  buyPressure: measured(1.4),
  lpBurned: measured(1),
  ...over,
})

const caps = defaultCaps(1_800_000_000)
const armed = (over = {}) => ({
  ...initialState(DEFAULT_CONFIG, caps),
  armed: true,
  lease: 'writer',
  socketOpen: true,
  ...over,
})

// ── Z-1: no timer may decide money ───────────────────────────────────────────

section('Z-1  no timer may decide money')

{
  // The headline assertion of the whole design. A `tick` in EVERY reachable state must
  // never produce a swap — because in a hidden tab a timer fires roughly once a minute
  // and a polled stop-loss silently stops checking a position it is still holding.
  const states = [
    initialState(DEFAULT_CONFIG, caps),
    armed(),
    armed({ killed: true }),
    armed({ lease: 'follower' }),
    armed({
      positions: {
        M: {
          mint: 'M', symbol: 'X', state: 'open', spentLamports: 1e7,
          tokensOut: measured(1000), entryPriceSol: measured(1),
          peakPriceSol: measured(3), lastPriceSol: measured(0.1),
          openedAtUnix: 1_800_000_000, lastEvaluatedUnix: 1_800_000_000,
        },
      },
    }),
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
  const holding = armed({
    positions: {
      M: {
        mint: 'M', symbol: 'X', state: 'open', spentLamports: 1e7,
        tokensOut: measured(1000), entryPriceSol: measured(1),
        peakPriceSol: measured(1), lastPriceSol: measured(1),
        openedAtUnix: 1_800_000_000, lastEvaluatedUnix: 1_800_000_000,
      },
    },
  })
  const arrival = step(holding, {
    kind: 'vault.changed', mint: 'M', priceSol: 0.5, slot: 1, atUnix: 1_800_000_001,
  })
  check('the same position exits on a chain ARRIVAL', arrival.effects.some(e => e.kind === 'swap' && e.side === 'sell'))
}

// ── exits ────────────────────────────────────────────────────────────────────

section('exit ladder')

const held = (over = {}) => ({
  mint: 'M', symbol: 'X', state: 'open', spentLamports: 1e7,
  tokensOut: measured(1000), entryPriceSol: measured(1),
  peakPriceSol: measured(1), lastPriceSol: measured(1),
  openedAtUnix: 1_800_000_000, lastEvaluatedUnix: 1_800_000_000, ...over,
})

check('stop-loss fires at the threshold', evaluateExit(held(), 0.85, 1_800_000_001, DEFAULT_CONFIG).reason === 'stop-loss')
check('take-profit fires at the threshold', evaluateExit(held(), 2.0, 1_800_000_001, DEFAULT_CONFIG).reason === 'take-profit')
// Order matters: a position past BOTH must cut, never take profit on a number it is
// below. Constructing that needs a peak, so it is the stop that must win.
check('a holding inside both thresholds holds', !evaluateExit(held(), 1.05, 1_800_000_001, DEFAULT_CONFIG).exit)
{
  // Pullback's firing region is narrower than it looks, and finding it is the point of
  // this fixture. Take-profit is evaluated FIRST, so a position still above +100% exits
  // as take-profit however far it has fallen from its peak. Pullback is what catches the
  // position that peaked high and has since dropped BACK THROUGH the take-profit line:
  // peak +145% (over the 140 floor), now +95% (under the 100 target), gave back 50%.
  const p = held({ peakPriceSol: measured(2.45) })
  const d = evaluateExit(p, 1.95, 1_800_000_005, DEFAULT_CONFIG)
  check('pullback fires once the peak clears the momentum floor', d.reason === 'pullback')

  // The same position while still above the target is a take-profit, not a pullback.
  // Both exit — only the recorded reason differs, and take-profit is the truer label.
  check(
    'a position still above the target exits as take-profit, not pullback',
    evaluateExit(p, 2.1, 1_800_000_005, DEFAULT_CONFIG).reason === 'take-profit',
  )
}
{
  // A peak BELOW the momentum floor must not arm the pullback rule.
  const p = held({ peakPriceSol: measured(1.3) })
  const d = evaluateExit(p, 1.02, 1_800_000_005, DEFAULT_CONFIG)
  check('a peak below the momentum floor does not arm pullback', d.reason !== 'pullback')
}
check(
  'no-pump exits a flat position, measured from the ARRIVAL time',
  evaluateExit(held(), 1.0, 1_800_000_000 + DEFAULT_CONFIG.noPumpTimeoutSecs, DEFAULT_CONFIG).reason === 'no-pump',
)
check(
  'the same flat position before the timeout holds',
  !evaluateExit(held(), 1.0, 1_800_000_000 + 5, DEFAULT_CONFIG).exit,
)
// The config relationship that has been broken in Rust before.
check('the shipped config satisfies the pullback arithmetic', configProblem(DEFAULT_CONFIG) === null)
check(
  'a config whose pullback can never fire is refused',
  configProblem({ ...DEFAULT_CONFIG, momentumMinPeakPct: 100 }) !== null,
)
// An unpriceable position must not be sold on a percentage nobody computed.
check(
  'a position with no observed entry is never exited on a price rule',
  !evaluateExit(held({ entryPriceSol: absent('never observed') }), 0.01, 1_800_009_999, DEFAULT_CONFIG).exit,
)
check(
  'an unobserved fill is refused explicitly, not silently held',
  evaluateExit(held({ state: 'unknown' }), 0.01, 1_800_009_999, DEFAULT_CONFIG).reason === 'unknown-position',
)
check('PnL against an unobserved entry is unmeasured, not 0', !pnlPct(held({ entryPriceSol: absent('x') }), 2).measured)
{
  const p = applyPrice(held(), 3, 1_800_000_002)
  const q = applyPrice(p, 1.5, 1_800_000_003)
  check('the peak only ever rises', q.peakPriceSol.value === 3)
}

// ── the measured / unmeasured rule ───────────────────────────────────────────

section('unmeasured is not zero')

check('an unmeasured term renders as an em dash', cell(absent('x')) === '—')
check('a MEASURED zero renders as 0.00', cell(measured(0)) === '0.00')
check('coverage is a count, not a ratio', coverageMeter({ measuredCount: 2, total: 5 }) !== coverageMeter({ measuredCount: 4, total: 10 }))
check('an empty coverage is ∅, never an empty meter', coverageMeter({ measuredCount: 0, total: 0 }) === '∅')
check('an unscored pool is not a low-scoring one', !scoredEntry(absent('not read'), DEFAULT_CONFIG).fires)
check('...and it says WHY rather than reporting a score', scoredEntry(absent('depth not read'), DEFAULT_CONFIG).reason.includes('unmeasured'))
check('two prints have no velocity', !changePct({ mint: 'M', points: [[0, 1]] }, 60, 60).measured)
check('an unmeasured edge is not a zero edge', !estimateEdge([{ pnlPct: 5 }]).winRate.measured)
check('a measured edge with no wins reports a MEASURED zero win rate', estimateEdge(Array(8).fill({ pnlPct: -5 })).winRate.value === 0)
check('...and its average win is absent, since there is no average of nothing', !estimateEdge(Array(8).fill({ pnlPct: -5 })).avgWinPct.measured)

// ── Ψ / coherence ────────────────────────────────────────────────────────────

section('coherence gate')

{
  let c = newCoherence()
  const v0 = evalCoherence(c, 12, 0.55)
  check('Ψ is UNMEASURED before the minimum samples', !v0.psi.measured)
  // The trap this repo has hit three times: an unmeasured gate that pins itself shut.
  check('...and entries are ALLOWED while it is unmeasured', v0.entriesAllowed)

  for (let i = 0; i < 12; i++) c = recordRead(c, i % 4 !== 0) // 9/12 = 0.75
  const v1 = evalCoherence(c, 12, 0.55)
  check('Ψ is measured once there are samples', v1.psi.measured && Math.abs(v1.psi.value - 0.75) < 1e-9)
  check('a healthy Ψ allows entries', v1.entriesAllowed)

  let bad = newCoherence()
  for (let i = 0; i < 20; i++) bad = recordRead(bad, i % 5 === 0) // 0.2
  check('a degraded Ψ halts entries', !evalCoherence(bad, 12, 0.55).entriesAllowed)

  // The rule that matters most: degraded ENTRIES, never degraded exits.
  const s = armed({ coherence: bad, positions: { M: held() } })
  const out = step(s, { kind: 'vault.changed', mint: 'M', priceSol: 0.5, slot: 1, atUnix: 1_800_000_002 })
  check('a degraded feed never stops an EXIT', out.effects.some(e => e.kind === 'swap' && e.side === 'sell'))
  check('...but does stop an ENTRY', decideEntry(s, pool(), 1_800_000_100).decline === 'coherence-degraded')
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
  const s = armed({ lease: 'follower' })
  check('a follower tab declines entries', decideEntry(s, pool(), 1_800_000_100).decline === 'no-lease')
  // A follower must still track positions — the writer is the one acting on them.
  const out = step(s, { kind: 'vault.changed', mint: 'M', priceSol: 2, slot: 1, atUnix: 1_800_000_002 })
  check('...but still ingests prices, so its display is live', out.state.history.M.points.length === 1)
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

section('entry strategies')

{
  let h = { mint: 'M', points: [] }
  const t0 = 1_800_000_000
  // A run-up then a give-back: 1 -> 2 (+100%), back to 1.6 (-20% from peak).
  ;[1, 1.3, 1.7, 2.0, 1.8, 1.5].forEach((p, i) => { h = pushPrice(h, t0 + i * 10, p) })
  check('pullback fires on a real run-up and give-back', pullback(h, t0 + 60).fires)

  let crash = { mint: 'M', points: [] }
  ;[1, 1.5, 2.0, 0.6, 0.5, 0.45].forEach((p, i) => { crash = pushPrice(crash, t0 + i * 10, p) })
  check('pullback refuses a decline that is not a pullback', !pullback(crash, t0 + 60).fires)

  let flat = { mint: 'M', points: [] }
  ;[1, 1.01, 1.0, 0.99, 1.0].forEach((p, i) => { flat = pushPrice(flat, t0 + i * 10, p) })
  check('pullback refuses without a prior run-up', !pullback(flat, t0 + 60).fires)

  let trend = { mint: 'M', points: [] }
  ;[1, 1.1, 1.2, 1.3, 1.4, 1.5].forEach((p, i) => { trend = pushPrice(trend, t0 + i * 30, p) })
  check('continuation fires on a monotone rise', continuation(trend, t0 + 180).fires)

  // Net-positive but not a trend: the whole reason the step test exists.
  let chop = { mint: 'M', points: [] }
  ;[1.0, 0.9, 0.85, 0.8, 0.75, 1.4].forEach((p, i) => { chop = pushPrice(chop, t0 + i * 30, p) })
  const chopSignal = continuation(chop, t0 + 180)
  check('continuation refuses a net rise that is not a trend', !chopSignal.fires)

  // History is bounded — a tab may run for hours across dozens of mints.
  let long = { mint: 'M', points: [] }
  for (let i = 0; i < 500; i++) long = pushPrice(long, t0 + i, 1)
  check('price history is bounded', long.points.length <= 64)
}

// ── sizing ───────────────────────────────────────────────────────────────────

section('sizing')

{
  const noEdge = estimateEdge([])
  const s1 = sizeEntry(1e8, noEdge, 1, 1, 1e9, 1e9, 2e6)
  check('an unmeasured edge sizes DEFENSIVELY rather than fully', s1.lamports === 5e7)
  check('...and names the reason', s1.applied[0].includes('unmeasured'))

  const losing = estimateEdge([...Array(6).fill({ pnlPct: -10 }), ...Array(2).fill({ pnlPct: 5 })])
  const s2 = sizeEntry(1e8, losing, 1, 1, 1e9, 1e9, 2e6)
  check('a measured negative edge sizes to zero', s2.lamports === 0)

  const s3 = sizeEntry(1e8, noEdge, 1, 1, 1e7, 1e9, 2e6)
  check('the per-trade cap clamps', s3.lamports === 1e7)
  const s4 = sizeEntry(1e8, noEdge, 1, 1, 1e9, 3e6, 2e6)
  check('the remaining budget clamps', s4.lamports === 3e6)
  const s5 = sizeEntry(1e8, noEdge, 1, 1, 1e9, 1e6, 2e6)
  check('a size below dust is refused rather than sent', s5.lamports === 0)
  const s6 = sizeEntry(1e8, noEdge, 1, 0, 1e9, 1e9, 2e6)
  check('a policy veto multiplier sizes to zero', s6.lamports === 0)
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

  const unarmed = step(initialState(DEFAULT_CONFIG, caps), { kind: 'pool.observed', pool: pool(), atUnix: 1_800_000_100 })
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
    evalCoherence(newCoherence(), 12, 0.55),
    liveness(false, null, 100),
    { armed: false, balanceLamports: absent('x'), committedLamports: 0, remainingLamports: 0, budgetLamports: 1e8, secsUntilExpiry: absent('not armed'), strandedCount: 0, warning: 'w' },
    'follower',
    evaluateGate(null),
    2,
    0.55,
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
  const files = ['types', 'gate', 'policy', 'strategy', 'exits', 'size', 'session', 'observe', 'seal', 'gatekeep', 'engine', 'readout']
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

console.log(`\n${failed === 0 ? 'ALL PASS' : `${failed} FAILED`}`)
process.exit(failed === 0 ? 0 : 1)
