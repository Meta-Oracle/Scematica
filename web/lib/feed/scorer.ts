// TypeScript port of the sniper's pool scorer and its cheap filters.
//
// ⚠️  PARITY CONTRACT — read before editing.
//
// `crates/scematica-sniper/src/pool_scorer.rs` and `filters.rs` remain authoritative:
// they run against real vault balances over a WebSocket feed and decide real money.
// This module is a *preview* scorer over a public REST feed. Its job is to show an
// honest approximation of what the bot would think, not to be a second brain.
//
// Two implementations of the same edge logic will drift unless that is managed, so:
//   • The likelihood-ratio ladders and the sigmoid below are copied verbatim from
//     pool_scorer.rs. Change them here only when changing them there.
//   • Every filter declares `parity`: 'port' means a faithful port of a Rust filter,
//     'approx' means the Rust input does not exist in the feed and this is a
//     documented substitute. Never quietly promote an 'approx' to a 'port'.
//   • `__fixtures` at the bottom pins the numbers the Rust unit tests assert, so a
//     drifting port fails a test rather than silently mis-scoring live pools.
//
// Known input gaps vs. Rust (all 'approx' or omitted here):
//   • buy-pressure ratio — needs quote_vault/base_vault; the feed has neither.
//   • LP burn / vault emptiness — needs an RPC read.
//   • deployer rug EMA — needs the local reputation ledger.

import type { FeedPool } from './jupiter'

// ── thresholds (mirror config.toml) ──────────────────────────────────────────

export const MIN_POOL_SIZE_SOL = 10.0   // config.toml [filters] min_pool_size
export const MAX_POOL_SIZE_SOL = 150.0  // config.toml [filters] max_pool_size
export const MIN_POOL_SCORE    = 65     // config.toml min_pool_score

// ── scorer ───────────────────────────────────────────────────────────────────

export interface ScoreInput {
  sizeSol: number
  /** Seconds since pool creation, or null when the age is unknown. */
  ageSecs: number | null
  /** quote_vault/base_vault ratio. Undefined when vault data is unavailable. */
  buyPressureRatio?: number
  /** Pre-graduation momentum score, 0 when unknown. */
  pumpfunScore?: number
}

/**
 * Predictive pool score, 0–100. Verbatim port of `PoolScorer::score`:
 * an empirical-Bayes product of likelihood ratios mapped through a logistic.
 */
export function scorePool(input: ScoreInput): number {
  const { sizeSol, ageSecs } = input

  // ── Hard rejects: pools empirically proven to never be profitable ──────────
  if (sizeSol <= 0) return 0    // ghost pool
  if (sizeSol < 1.0) return 1   // sub-1 SOL: ~0% win rate, always rug
  if (sizeSol < 3.0) return 8   // 1-3 SOL: empirically unprofitable

  const prior = 0.1
  let p = prior

  // Signal 1: pool size (most discriminating)
  const sizeLr =
    sizeSol < 3.0   ? 0.05 :
    sizeSol < 5.0   ? 0.15 :
    sizeSol < 10.0  ? 0.35 :
    sizeSol < 20.0  ? 0.85 :
    sizeSol <= 45.0 ? 4.2  :
    sizeSol <= 85.0 ? 3.2  :
    sizeSol <= 120.0? 1.5  :
    sizeSol <= 150.0? 0.70 : 0.30
  p *= sizeLr

  // Signal 2: pool age (exponential decay of opportunity)
  const age = ageSecs
  const ageLr =
    age === null   ? 0.60 :
    age <= 7       ? 2.80 :
    age <= 20      ? 1.90 :
    age <= 40      ? 1.10 :
    age <= 90      ? 0.55 :
    age <= 210     ? 0.20 : 0.05
  p *= ageLr

  // Signal 3: SOL inflow velocity. With a real creation timestamp this is a genuine
  // measurement; with an unknown age Rust falls back to a size-proportional guess.
  let velocitySolPerSec = 0
  let velocityLr: number
  if (age !== null) {
    velocitySolPerSec = sizeSol / Math.max(age, 1)
    velocityLr =
      velocitySolPerSec >= 5.0 ? 3.50 :
      velocitySolPerSec >= 2.0 ? 2.80 :
      velocitySolPerSec >= 0.8 ? 1.80 :
      velocitySolPerSec >= 0.2 ? 1.20 : 0.65
  } else {
    velocityLr =
      sizeSol >= 20.0 ? 1.80 :
      sizeSol >= 10.0 ? 1.40 :
      sizeSol >= 5.0  ? 1.10 : 0.80
  }
  p *= velocityLr

  // Signal 4: AMM buy-pressure ratio. The feed cannot supply it, so this normally
  // takes the same 0.80 penalty Rust applies when the base vault read fails.
  const ratio = input.buyPressureRatio
  if (ratio !== undefined && ratio > 0) {
    const pressureLr =
      ratio >= 0.0002  ? 2.20 :
      ratio >= 0.0001  ? 1.60 :
      ratio >= 0.00005 ? 1.20 :
      ratio >= 0.00002 ? 1.00 : 0.85
    p *= pressureLr
  } else {
    p *= 0.80
  }

  // Signal 5: expected value from the constant-product formula
  if (velocitySolPerSec > 0 && sizeSol > 0) {
    const noPumpSecs = 15.0 // matches config no_pump_timeout_secs
    const p2x = Math.min((velocitySolPerSec * noPumpSecs) / sizeSol, 1.0)
    p *= 1.0 + 1.5 * p2x
  }

  // Signal 6: pump.fun pre-graduation momentum
  const pf = input.pumpfunScore ?? 0
  if (pf > 0) {
    const pfLr = pf >= 90 ? 3.5 : pf >= 80 ? 2.8 : pf >= 70 ? 2.0 : 1.3
    p *= pfLr
  }

  // Map posterior to 0–100 via logistic sigmoid
  const k = 28.0
  const x0 = 0.09
  const score = 100 / (1 + Math.exp(-k * (p - x0)))
  return Math.min(100, Math.max(0, score))
}

/** Additive social boost, matching `PoolScorer::score_with_socials`. */
export function socialBoost(socialCount: number): number {
  return socialCount <= 0 ? -4 : socialCount === 1 ? 2 : socialCount === 2 ? 5 : socialCount === 3 ? 8 : 10
}

/** Additive live-inflow boost, matching `PoolScorer::score_full`. */
export function inflowBoost(solPerSec: number): number {
  return solPerSec >= 3.0 ? 18 : solPerSec >= 1.5 ? 12 : solPerSec >= 0.5 ? 7 : solPerSec >= 0.2 ? 3 : 0
}

/** Score a feed pool. Age is always known from the feed's creation timestamp. */
export function scoreFeedPool(pool: FeedPool): number {
  return scorePool({
    sizeSol: pool.sizeSol,
    ageSecs: pool.createdAtUnix > 0 ? pool.ageSecs : null,
    // buyPressureRatio intentionally omitted — see the parity note above.
    pumpfunScore: 0,
  })
}

// ── filters ──────────────────────────────────────────────────────────────────

/** Known scam/rug keywords — copied from `SCAM_WORDS` in filters.rs. */
const SCAM_WORDS = [
  'test', 'rug', 'scam', 'free', 'airdrop', 'safe', 'moon100x', '1000x', 'elon',
  'trump', 'biden', 'shib2', 'pepe2', 'honeypot', 'drain', 'presale', 'stealth',
  'fair launch', 'dev wallet',
]

export type Parity = 'port' | 'approx'

export interface FilterVerdict {
  name: string
  passed: boolean
  reason: string
  /** 'port' = faithful port of the Rust filter; 'approx' = documented substitute. */
  parity: Parity
}

interface FilterDef {
  name: string
  parity: Parity
  check: (pool: FeedPool, score: number) => { passed: boolean; reason: string }
}

const FILTERS: FilterDef[] = [
  {
    name: 'MintRenounced',
    parity: 'port',
    check: p => p.mintRenounced
      ? { passed: true, reason: 'Mint authority revoked' }
      : { passed: false, reason: 'Mint authority not renounced' },
  },
  {
    name: 'NotFreezable',
    parity: 'port',
    check: p => p.freezeDisabled
      ? { passed: true, reason: 'No freeze authority' }
      : { passed: false, reason: 'Token has freeze authority' },
  },
  {
    name: 'PoolSize',
    parity: 'port',
    check: p => {
      if (p.sizeSol < MIN_POOL_SIZE_SOL) {
        return { passed: false, reason: `Pool too small: ${p.sizeSol.toFixed(2)} SOL` }
      }
      if (p.sizeSol > MAX_POOL_SIZE_SOL) {
        return { passed: false, reason: `Pool too large: ${p.sizeSol.toFixed(2)} SOL` }
      }
      return { passed: true, reason: `${p.sizeSol.toFixed(2)} SOL` }
    },
  },
  {
    name: 'NameFilter',
    parity: 'port',
    check: p => {
      const hay = `${p.name} ${p.symbol}`.toLowerCase()
      const hit = SCAM_WORDS.find(w => hay.includes(w))
      return hit
        ? { passed: false, reason: `Name contains "${hit}"` }
        : { passed: true, reason: 'Name clean' }
    },
  },
  {
    name: 'DeployerReputation',
    // Rust reads an EMA rug ledger built from this bot's own sell outcomes. The feed
    // has no such history, so this substitutes the deployer's mint→migration ratio:
    // many launches that almost never graduate is the serial-rugger signature.
    parity: 'approx',
    check: p => {
      if (p.devMints < 5) return { passed: true, reason: `${p.devMints} prior mints` }
      const migrationRate = p.devMints > 0 ? p.devMigrations / p.devMints : 1
      return migrationRate < 0.1
        ? {
            passed: false,
            reason: `Serial deployer: ${p.devMints} mints, ${p.devMigrations} migrations`,
          }
        : { passed: true, reason: `${p.devMints} mints, ${p.devMigrations} migrations` }
    },
  },
  {
    name: 'DevHoldings',
    // No Rust counterpart — the feed exposes the deployer's remaining supply share,
    // which the on-chain pipeline does not currently read.
    parity: 'approx',
    check: p => p.devBalancePct > 15
      ? { passed: false, reason: `Deployer holds ${p.devBalancePct.toFixed(1)}% of supply` }
      : { passed: true, reason: `Deployer holds ${p.devBalancePct.toFixed(1)}%` },
  },
  {
    name: 'PoolScore',
    parity: 'port',
    check: (_p, score) => score >= MIN_POOL_SCORE
      ? { passed: true, reason: `Score ${score.toFixed(1)}` }
      : { passed: false, reason: `Score ${score.toFixed(1)} < ${MIN_POOL_SCORE}` },
  },
]

/** Every filter name, in pipeline order — used to seed the rejection table. */
export const FILTER_NAMES = FILTERS.map(f => f.name)

export interface Evaluation {
  pool: FeedPool
  score: number
  decision: 'accepted' | 'rejected'
  /** Name of the first failing filter, or 'passed'. */
  stage: string
  reason: string
  verdicts: FilterVerdict[]
}

/**
 * Run the pipeline over one pool. Unlike the Rust pipeline this evaluates *every*
 * filter rather than short-circuiting, so the UI can show the full picture; `stage`
 * still reports the first failure, which is what the bot would have logged.
 */
export function evaluatePool(pool: FeedPool): Evaluation {
  const score = scoreFeedPool(pool)
  const verdicts: FilterVerdict[] = FILTERS.map(f => {
    const r = f.check(pool, score)
    return { name: f.name, passed: r.passed, reason: r.reason, parity: f.parity }
  })
  const firstFail = verdicts.find(v => !v.passed)
  return {
    pool,
    score,
    decision: firstFail ? 'rejected' : 'accepted',
    stage: firstFail?.name ?? 'passed',
    reason: firstFail?.reason ?? 'All filters passed',
    verdicts,
  }
}

/** Aggregate rejection counts across a batch, shaped like the Rust FilterStats file. */
export function aggregateStats(evals: Evaluation[]): {
  pools_seen: number
  pools_passed: number
  rejections: Record<string, number>
} {
  const rejections: Record<string, number> = {}
  let passed = 0
  for (const e of evals) {
    if (e.decision === 'accepted') passed++
    else rejections[e.stage] = (rejections[e.stage] ?? 0) + 1
  }
  return { pools_seen: evals.length, pools_passed: passed, rejections }
}

// ── parity fixtures ──────────────────────────────────────────────────────────

/**
 * The cases `pool_scorer.rs`'s unit tests assert, expressed against this port.
 * Exported so a test run fails when the ladders drift out of sync with Rust.
 */
export const __fixtures: Array<{
  name: string
  input: ScoreInput
  expect: (score: number) => boolean
}> = [
  { name: 'ghost_pool_scores_zero',      input: { sizeSol: 0, ageSecs: null },   expect: s => s === 0 },
  { name: 'sub_1_sol_hard_reject',       input: { sizeSol: 0.5, ageSecs: null }, expect: s => s < 5 },
  { name: 'tiny_pool_3sol_low_score',    input: { sizeSol: 2, ageSecs: null },   expect: s => s < 20 },
  {
    name: 'perfect_pool_scores_high',
    input: { sizeSol: 30, ageSecs: 5, buyPressureRatio: 30 / 1 },
    expect: s => s >= 85,
  },
  { name: 'stale_pool_scores_low',       input: { sizeSol: 15, ageSecs: 300 },   expect: s => s < 30 },
  {
    name: 'old_established_pool_low_score',
    input: { sizeSol: 200, ageSecs: null },
    expect: s => s < 25,
  },
]
