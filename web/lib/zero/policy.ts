// Zero's policy — a pinned DQ* checkpoint that never trains.
//
// ── The training question, answered ──────────────────────────────────────────
//
// A browser-side net could learn from its own fills. It must not, and the reason is not
// that training is hard here — it is that a policy which trains per tab is a policy
// nobody can reproduce. Two operators on the same pool would get different advice, an
// operator on two tabs would get different advice from each, and a sealed decision
// record citing "the DQ* said sell" would name weights that exist nowhere and can never
// be re-derived. That destroys the one thing Zero is for: a decision checkable by
// somebody who was not there.
//
// So: **run a pinned checkpoint, log transitions, train nowhere.** `POLICY_ID` is
// committed into every record, transitions are recorded for offline training against
// the Rust agent, and `DuelingNet.trainStep` is never called from this module. A test
// asserts that last part by source scan, because it is one line away from being
// reintroduced by somebody being helpful.
//
// The checkpoint is deterministic: `DuelingNet` seeds every weight from `mulberry32`,
// so a fixed seed IS a fixed set of weights, byte for byte, in every browser. That is
// what makes `POLICY_ID` meaningful without shipping a weights file.

import { DuelingNet, ACTIONS, N_ACTIONS, STATE_DIM, mulberry32, type ActionName } from '../sim/dqstar.ts'
import { type Coverage, type Term, absent, measured } from './types.ts'

/**
 * The pinned checkpoint. Changing this number is a policy change and must be treated as
 * one: it invalidates comparison with every record sealed before it, which is exactly
 * why the id is committed rather than merely logged.
 */
export const POLICY_SEED = 0x5ce_3a11

/** Stamped into every DecisionRecord. Bump deliberately, never incidentally. */
export const POLICY_ID = `dqstar-dueling-${STATE_DIM}x128x64-seed${POLICY_SEED.toString(16)}-v1`

// ── the state vector ─────────────────────────────────────────────────────────

/**
 * Feature order, copied from `crates/scematica-nn/src/state.rs`. **Rust is
 * authoritative** and `check:zero` reads that file to pin both this list and the
 * neutral table below, so a feature added there fails here rather than silently
 * shifting every subsequent index — the same positional hazard as an Anchor account
 * list, with a neural net instead of a token program on the other end.
 */
export const FEATURES = [
  'pool_age_secs',
  'initial_liquidity_sol',
  'price_change_pct',
  'volume_5min_sol',
  'buy_sell_ratio',
  'lp_burned',
  'mint_renounced',
  'current_pnl_pct',
  'position_age_secs',
  'daily_pnl_sol',
  'consecutive_wins',
  'consecutive_losses',
  'sol_balance_sol',
  'regime',
  'volatility',
  'spread_pct',
  'time_of_day_norm',
  'open_positions',
  'peak_pnl_pct',
  'pool_score_norm',
  'deployer_rug_rate',
  'volume_velocity',
  'price_velocity',
  'price_acceleration',
] as const

export type FeatureName = (typeof FEATURES)[number]

/**
 * What an unmeasured feature becomes. A port of `state.rs::NEUTRAL`.
 *
 * **This is not a blanket 0.5, and that is the entire point.** `price_change_pct` is
 * encoded as `clamp(-1,3)/3`, which puts a 0% change at 0.0 and the midpoint at +150%;
 * `buy_sell_ratio` is `/5`, so a balanced book is 0.2. A uniform midpoint would replace
 * "I do not know" with "strongly bullish" on precisely the two features where that costs
 * the most money.
 *
 * The failure this prevents is measured, not theoretical: `pool_age_secs` is non-zero in
 * 0 of 8,422 recorded decisions because `pool.open_time` is essentially never populated,
 * and 0.0 normalises to the BOTTOM of its band — a pool zero seconds old, the most
 * bullish value it can take.
 */
export const NEUTRAL: number[] = [
  0.5, // pool_age_secs
  0.5, // initial_liquidity_sol
  0.0, // price_change_pct — 0% change is 0.0 here, not the midpoint
  0.5, // volume_5min_sol
  0.2, // buy_sell_ratio — a balanced book is 1.0, which /5 puts at 0.2
  0.5, // lp_burned — between the two truths, not either of them
  0.5, // mint_renounced
  0.5, // current_pnl_pct
  0.5, // position_age_secs
  0.5, // daily_pnl_sol
  0.0, // consecutive_wins — an unknown streak is no streak
  0.0, // consecutive_losses
  0.5, // sol_balance_sol
  0.5, // regime — sideways
  0.5, // volatility
  0.5, // spread_pct
  0.5, // time_of_day_norm
  0.0, // open_positions
  0.0, // peak_pnl_pct
  0.5, // pool_score_norm
  0.5, // deployer_rug_rate
  0.5, // volume_velocity
  0.5, // price_velocity
  0.5, // price_acceleration
]

/** A raw feature value, or the admission that nobody measured it. */
export type FeatureInput = Partial<Record<FeatureName, number>>

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v))
const capped = (v: number, by: number) => Math.min(v / by, 1)

/**
 * The encoding, ported from `state.rs::to_vec_raw`. Kept separate from neutral
 * substitution for the same reason Rust does: so a test can show what the substitution
 * changed, and so there is one encoding rather than two that drift.
 */
function encodeRaw(name: FeatureName, v: number): number {
  switch (name) {
    case 'pool_age_secs': return capped(v, 3600)
    case 'initial_liquidity_sol': return capped(v, 100)
    case 'price_change_pct': return clamp(v, -1, 3) / 3
    case 'volume_5min_sol': return capped(v, 50)
    case 'buy_sell_ratio': return capped(v, 5)
    case 'lp_burned': return v ? 1 : 0
    case 'mint_renounced': return v ? 1 : 0
    case 'current_pnl_pct': return clamp(v, -1, 2) / 2 + 0.5
    case 'position_age_secs': return capped(v, 3600)
    case 'daily_pnl_sol': return clamp(v, -2, 2) / 2 + 0.5
    case 'consecutive_wins': return capped(v, 10)
    case 'consecutive_losses': return capped(v, 10)
    case 'sol_balance_sol': return capped(v, 10)
    case 'regime': return (v + 1) / 2
    case 'volatility': return clamp(v, 0, 1)
    case 'spread_pct': return capped(v, 0.1)
    case 'time_of_day_norm': return clamp(v, 0, 1)
    case 'open_positions': return capped(v, 5)
    case 'peak_pnl_pct': return clamp(v, 0, 5) / 5
    case 'pool_score_norm': return clamp(v, 0, 1)
    case 'deployer_rug_rate': return clamp(v, 0, 1)
    case 'volume_velocity': return clamp(v, -1, 1) * 0.5 + 0.5
    case 'price_velocity': return clamp(v, -1, 1) * 0.5 + 0.5
    case 'price_acceleration': return clamp(v, -1, 1) * 0.5 + 0.5
  }
}

export interface EncodedState {
  vector: Float64Array
  /** Which indices were substituted. */
  unmeasured: FeatureName[]
  coverage: Coverage
}

/**
 * Encode, substituting the neutral element for anything absent.
 *
 * A feature present in `input` but not finite is treated as ABSENT rather than encoded.
 * A NaN reaching the net produces NaN Q-values, which compare false against everything
 * and silently make the argmax index 0 — `Hold` — a veto nobody decided.
 */
export function encode(input: FeatureInput): EncodedState {
  const vector = new Float64Array(STATE_DIM)
  const unmeasured: FeatureName[] = []

  FEATURES.forEach((name, i) => {
    const raw = input[name]
    if (raw === undefined || !Number.isFinite(raw)) {
      vector[i] = NEUTRAL[i]
      unmeasured.push(name)
    } else {
      vector[i] = encodeRaw(name, raw)
    }
  })

  return {
    vector,
    unmeasured,
    coverage: { measuredCount: STATE_DIM - unmeasured.length, total: STATE_DIM },
  }
}

// ── the net ──────────────────────────────────────────────────────────────────

let net: DuelingNet | null = null

/** The pinned checkpoint, built once. Deterministic from `POLICY_SEED`. */
export function policyNet(): DuelingNet {
  if (!net) net = new DuelingNet(mulberry32(POLICY_SEED))
  return net
}

/** Reset for tests. Never called in the loop — the checkpoint must not change mid-session. */
export function __resetNet(): void {
  net = null
}

// ── advice ───────────────────────────────────────────────────────────────────

export type Lean = 'buy-aggressive' | 'buy' | 'neutral' | 'bearish' | 'veto'

export interface Advice {
  lean: Lean
  /** Multiplies the Kelly size. 0 when vetoed. */
  sizeMultiplier: number
  q: number[]
  action: ActionName
  coverage: Coverage
  unmeasured: FeatureName[]
  reason: string
  /** Absent when coverage is too thin for the advice to mean anything. */
  confidence: Term
}

/**
 * The veto margin, from `sniper.rs`'s `NN_VETO_REL_MARGIN`.
 *
 * A bearish lean only fully suppresses a buy when it beats the best buy action by this
 * much. A weaker lean downgrades the size instead — a partially-converged net must not
 * be able to silently kill a live edge, and in a browser the net is *always* partially
 * converged because it never trains.
 */
export const VETO_REL_MARGIN = 0.15

/**
 * Coverage below this and the advice is not used at all.
 *
 * Zero measures fewer features than the Rust bot: position-side features (`peak_pnl_pct`,
 * `position_age_secs`, streaks) simply do not exist at entry time. Some substitution is
 * normal and expected. But past a point the net is reading a vector it invented, and five
 * finite Q-values with a clean argmax look exactly the same either way — there is no
 * channel in the output that reports how much of the input was real, so the threshold has
 * to be here.
 */
export const MIN_ADVICE_COVERAGE = 0.5

export function advise(input: FeatureInput): Advice {
  const { vector, unmeasured, coverage } = encode(input)
  const fraction = coverage.measuredCount / coverage.total

  const q = Array.from(policyNet().forward(vector))
  let best = 0
  for (let i = 1; i < N_ACTIONS; i++) if (q[i] > q[best]) best = i
  const action = ACTIONS[best]

  if (fraction < MIN_ADVICE_COVERAGE) {
    return {
      lean: 'neutral',
      sizeMultiplier: 1,
      q,
      action,
      coverage,
      unmeasured,
      confidence: absent(`coverage ${coverage.measuredCount}/${coverage.total}`),
      reason: `policy not consulted — only ${coverage.measuredCount}/${coverage.total} features measured`,
    }
  }

  // ACTIONS = Hold, BuyStandard, BuyAggressive, SellPartial, SellAll
  const qHold = q[0]
  const bestBuy = Math.max(q[1], q[2])
  const bestBear = Math.max(q[3], q[4])

  // Dispersion, not the argmax. Five nearly-equal Q-values have a clear argmax and mean
  // nothing; this is the DQ*-veto lesson — value dispersion is not action dispersion.
  const spread = Math.max(...q) - Math.min(...q)
  const confidence = measured(spread)

  if (bestBear > bestBuy * (1 + VETO_REL_MARGIN) && bestBear > qHold) {
    return {
      lean: 'veto',
      sizeMultiplier: 0,
      q, action, coverage, unmeasured, confidence,
      reason: `policy vetoes: bearish Q ${bestBear.toFixed(3)} exceeds best buy ${bestBuy.toFixed(3)} by more than ${(VETO_REL_MARGIN * 100).toFixed(0)}%`,
    }
  }
  if (bestBear > bestBuy) {
    return {
      lean: 'bearish',
      sizeMultiplier: 0.5,
      q, action, coverage, unmeasured, confidence,
      reason: `policy leans bearish but within the veto margin — size halved`,
    }
  }
  if (action === 'BuyAggressive') {
    return {
      lean: 'buy-aggressive',
      sizeMultiplier: 1.5,
      q, action, coverage, unmeasured, confidence,
      reason: 'policy favours an aggressive entry',
    }
  }
  if (action === 'BuyStandard') {
    return {
      lean: 'buy', sizeMultiplier: 1, q, action, coverage, unmeasured, confidence,
      reason: 'policy favours a standard entry',
    }
  }
  return {
    lean: 'neutral', sizeMultiplier: 0.5, q, action, coverage, unmeasured, confidence,
    reason: 'policy is neutral — size halved',
  }
}

// ── transitions ──────────────────────────────────────────────────────────────

/**
 * Recorded, never learned from here.
 *
 * These exist so a session's experience can be exported and replayed against the Rust
 * agent, which is the only place training may happen — one policy, reproducible, with a
 * checkpoint anybody can name.
 */
export interface LoggedTransition {
  atUnix: number
  mint: string
  state: number[]
  action: number
  /** Absent until the position resolves; a declined branch never resolves at all. */
  reward: number | null
  coverage: Coverage
  policyId: string
}

export function logTransition(
  atUnix: number,
  mint: string,
  encoded: EncodedState,
  action: number,
): LoggedTransition {
  return {
    atUnix,
    mint,
    state: Array.from(encoded.vector),
    action,
    reward: null,
    coverage: encoded.coverage,
    policyId: POLICY_ID,
  }
}
