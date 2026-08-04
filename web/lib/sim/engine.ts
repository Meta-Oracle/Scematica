// Self-contained simulation engine backing the standalone web API.
//
// ⚠️  This produces SIMULATED trading activity. No real capital, no real orders.
// Every response built from it is tagged `simulated: true` and the UI shows a
// permanent SIMULATION badge. It exists so the dashboard is fully explorable with
// zero backend — pair a real sniper (see lib/net.ts) to see live money instead.
//
// What is *not* simulated: the Deep Q*™ network in `./dqstar` genuinely runs here.
// Forward passes, ε-greedy selection, replay sampling and gradient steps all
// execute for real; only the market feeding it is synthetic.
//
// Determinism: the session is a pure function of (SEED, elapsed-time-in-cycle), so
// a serverless invocation with cold memory reproduces the same state as a warm one.

import {
  ACTIONS,
  DQStarAgent,
  N_ACTIONS,
  STATE_DIM,
  mulberry32,
  type ActionName,
} from './dqstar'
import type {
  FilterStats,
  LivePosition,
  Metrics,
  NNAdvice,
  NNStats,
  Pool,
  PoolDecision,
  TournamentSnapshot,
  Trade,
  TxTelemetry,
} from '../types'

const LAMPORTS = 1_000_000_000

const SEED = 0x5ce4a
/** A demo session runs 6h then restarts, so the dashboard never goes stale. */
const SESSION_SECS = 6 * 3600
const EVENT_INTERVAL_SECS = 12
const MAX_EVENTS = Math.floor(SESSION_SECS / EVENT_INTERVAL_SECS)

/** Train every Nth event — bounds per-request compute on a cold serverless start. */
const TRAIN_EVERY = 10

/**
 * Advice readiness for a *demo-length* run. The Rust agent uses 10k train steps,
 * which a 6h synthetic session never reaches; ε-convergence is the equivalent
 * signal at this scale. Not a claim about the production threshold.
 */
const READY_EPSILON = 0.15
const READY_TRAIN_STEPS = 250

/** Hard ceiling on a single position, in multiples of entry. Without it the
 *  compounding per-tick growth produces absurd, non-credible returns. A 6x is
 *  already an exceptional meme-sniper outcome; the demo should not imply better. */
const MAX_POSITION_MULTIPLE = 6
/** Reward clamp fed to the network — one outlier runner otherwise dominates the
 *  Q-targets and the loss diverges. */
const REWARD_MIN = -1
const REWARD_MAX = 3

const SYMBOLS = [
  'BONKAI', 'WIFHAT', 'PEPE2', 'SOLCAT', 'MOONER', 'DEGEN', 'FROGGY', 'TURBO',
  'GIGA', 'SHIBX', 'PUMPKN', 'ZORO', 'NEKO', 'BASED', 'CHAD', 'MYRO',
  'SAMO', 'POPCAT', 'MEW', 'BOME', 'SLERF', 'WEN', 'MANEKI', 'HARAMBE',
]

const FILTER_NAMES = [
  'liquidity_too_low', 'pool_score_below_min', 'lp_not_burned', 'mint_not_renounced',
  'deployer_rug_history', 'buy_pressure_too_low', 'pool_too_old', 'holder_concentration',
  'mint_cooldown_active', 'velocity_stalled',
]

const REJECT_REASONS: Record<string, string> = {
  liquidity_too_low: 'quote vault below min_liquidity_sol',
  pool_score_below_min: 'pool score under effective_min_score',
  lp_not_burned: 'LP tokens not burned — rug risk',
  mint_not_renounced: 'mint authority still live',
  deployer_rug_history: 'deployer exceeds max_deployer_rugs_24h',
  buy_pressure_too_low: 'buy/sell ratio under threshold',
  pool_too_old: 'pool age past entry window',
  holder_concentration: 'top holder concentration too high',
  mint_cooldown_active: 'mint in re-entry cooldown',
  velocity_stalled: 'inflow velocity below threshold',
}

// ── helpers ───────────────────────────────────────────────────────────────────

function fakeMint(rnd: () => number): string {
  const chars = 'abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ123456789'
  let s = ''
  for (let i = 0; i < 40; i++) s += chars[Math.floor(rnd() * chars.length)]
  return s + 'pump'
}

function clamp01(v: number): number {
  return v < 0 ? 0 : v > 1 ? 1 : v
}

interface OpenPos {
  mint: string
  symbol: string
  entryLamports: number
  entryTs: number
  peakLamports: number
  currentLamports: number
  tpPct: number
  slPct: number
  escalations: number
  declineStreak: number
  /** Drives the price path deterministically for this position. */
  drift: number
  vol: number
  state: Float64Array
  action: number
}

export interface SimSnapshot {
  generatedAt: number
  uptimeSecs: number
  metrics: Metrics
  trades: Trade[]
  positions: LivePosition[]
  decisions: PoolDecision[]
  filters: FilterStats
  pools: Pool[]
  nn: NNStats
  advice: NNAdvice
  tournament: TournamentSnapshot
  telemetry: TxTelemetry[]
  logs: string[]
}

// ── the simulation ────────────────────────────────────────────────────────────

function buildState(
  rnd: () => number,
  poolScore: number,
  ageSecs: number,
  liqSol: number,
  pnlPct: number,
  openCount: number,
  hourUtc: number,
): Float64Array {
  const s = new Float64Array(STATE_DIM)
  s[0] = clamp01(ageSecs / 300)
  s[1] = clamp01(liqSol / 200)
  s[2] = clamp01(0.5 + pnlPct / 200)
  s[3] = clamp01(rnd())            // volume
  s[4] = clamp01(rnd())            // buy/sell ratio
  s[5] = rnd() > 0.25 ? 1 : 0      // LP burned
  s[6] = rnd() > 0.3 ? 1 : 0       // mint renounced
  s[7] = clamp01(0.5 + pnlPct / 200)
  s[8] = clamp01(rnd() * 0.4)      // position age
  s[9] = clamp01(0.5 + rnd() * 0.2)
  s[10] = clamp01(rnd())           // win streak
  s[11] = clamp01(rnd())           // loss streak
  s[12] = clamp01(0.3 + rnd() * 0.5) // balance
  s[13] = clamp01(rnd())           // regime
  s[14] = clamp01(rnd() * 0.6)     // volatility
  s[15] = clamp01(rnd() * 0.3)     // spread
  s[16] = hourUtc / 24
  s[17] = clamp01(openCount / 5)
  s[18] = clamp01(rnd())           // peak pnl
  s[19] = clamp01(poolScore / 100)
  s[20] = clamp01(rnd() * 0.3)     // deployer rug rate
  s[21] = clamp01(rnd())           // volume velocity
  s[22] = clamp01(rnd())           // price velocity
  s[23] = clamp01(rnd())           // price acceleration
  return s
}

function simulate(nowMs: number): SimSnapshot {
  const nowSecs = Math.floor(nowMs / 1000)
  const cycleStart = Math.floor(nowSecs / SESSION_SECS) * SESSION_SECS
  const uptimeSecs = nowSecs - cycleStart
  const events = Math.min(MAX_EVENTS, Math.floor(uptimeSecs / EVENT_INTERVAL_SECS))

  // Re-seed per cycle so each 6h session is a fresh but reproducible run.
  const rnd = mulberry32(SEED ^ cycleStart)

  // ε decays once per evaluated pool (~1800 per cycle), so the rate is tuned to
  // converge partway through a 6h session rather than the Rust 0.9995 — a visitor
  // sees the agent still exploring early on and advising once it has settled.
  const hp = { epsilonDecay: 0.995, lr: 0.0015, gamma: 0.99 }
  const agent = new DQStarAgent(SEED ^ cycleStart, hp)

  // Tournament variants: same architecture, different hyperparameters — only the
  // primary's reward curve is driven by the live loop; the others shadow it with
  // their own ε schedules, exactly as the Rust tournament does in paper mode.
  const variants = [
    { name: 'conservative', epsilonDecay: 0.997, reward: 0, epsilon: 1 },
    { name: 'balanced', epsilonDecay: 0.995, reward: 0, epsilon: 1 },
    { name: 'aggressive', epsilonDecay: 0.992, reward: 0, epsilon: 1 },
  ]

  const trades: Trade[] = []
  const decisions: PoolDecision[] = []
  const telemetry: TxTelemetry[] = []
  const pools: Pool[] = []
  const logs: string[] = []
  const open = new Map<string, OpenPos>()

  const rejections: Record<string, number> = {}
  for (const n of FILTER_NAMES) rejections[n] = 0

  let poolsSeen = 0
  let poolsPassed = 0
  let attempted = 0
  let confirmed = 0
  let failed = 0
  let realisedLamports = 0
  let arbExecuted = 0

  const iso = (ts: number) => new Date(ts * 1000).toISOString()

  for (let ev = 0; ev < events; ev++) {
    const ts = cycleStart + ev * EVENT_INTERVAL_SECS
    const hourUtc = new Date(ts * 1000).getUTCHours()

    // ── 1. a candidate pool appears ──────────────────────────────────────────
    poolsSeen++
    const mint = fakeMint(rnd)
    const symbol = SYMBOLS[Math.floor(rnd() * SYMBOLS.length)]
    const poolScore = Math.floor(35 + rnd() * 65)
    const ageSecs = Math.floor(rnd() * 180)
    const liqSol = 5 + rnd() * 180
    const velocity = rnd() * 2.5
    const inflow = rnd() * 1.8
    const buyPressure = rnd() * 0.05

    pools.push({
      mint,
      score: poolScore,
      size_sol: Number(liqSol.toFixed(2)),
      age_secs: ageSecs,
      passed_filters: false,
      timestamp: ts,
    })

    const state = buildState(rnd, poolScore, ageSecs, liqSol, 0, open.size, hourUtc)
    const action = agent.selectAction(state)
    agent.noteDecision()
    for (const v of variants) {
      if (v.epsilon > 0.05) v.epsilon *= v.epsilonDecay
    }
    const actionName = ACTIONS[action]
    const qv = agent.lastQ
    const maxQ = Math.max(...qv)
    const minQ = Math.min(...qv)
    const confidence = maxQ - minQ > 1e-9 ? clamp01((maxQ - qv.reduce((a, b) => a + b, 0) / N_ACTIONS) / (maxQ - minQ + 1e-9)) : 0

    // ── 2. filter pipeline ───────────────────────────────────────────────────
    const minScore = 65
    let rejectedBy: string | null = null
    if (poolScore < minScore) rejectedBy = 'pool_score_below_min'
    else if (liqSol < 12) rejectedBy = 'liquidity_too_low'
    else if (rnd() < 0.18) rejectedBy = FILTER_NAMES[Math.floor(rnd() * FILTER_NAMES.length)]

    // The Deep Q* veto: a bearish lean on a pool that passed the static filters
    // still blocks the buy — this is the agent actually gating entries.
    const dqBearish = actionName === 'SellAll' || actionName === 'SellPartial'
    const wantsBuy = actionName === 'BuyStandard' || actionName === 'BuyAggressive'

    let decision: PoolDecision['decision']
    let reason: string
    let stage: string

    if (rejectedBy) {
      rejections[rejectedBy] = (rejections[rejectedBy] ?? 0) + 1
      decision = 'rejected'
      stage = 'filter_pipeline'
      reason = REJECT_REASONS[rejectedBy] ?? rejectedBy
    } else if (dqBearish) {
      decision = 'rejected'
      stage = 'dq_star_veto'
      reason = `Deep Q* veto — ${actionName} dominates buy actions`
      rejections['dq_star_veto'] = (rejections['dq_star_veto'] ?? 0) + 1
    } else if (!wantsBuy) {
      decision = 'ignored'
      stage = 'dq_star_gate'
      reason = 'Deep Q* holding — no edge at this state'
    } else if (open.size >= 5) {
      decision = 'ignored'
      stage = 'position_cap'
      reason = 'max concurrent positions reached'
    } else {
      decision = 'accepted'
      stage = 'entry'
      reason = `Deep Q* ${actionName} — score ${poolScore}, inflow ${inflow.toFixed(2)} SOL/s`
      poolsPassed++
      pools[pools.length - 1].passed_filters = true
    }

    decisions.push({
      timestamp: iso(ts),
      mint,
      pool: fakeMint(rnd).slice(0, 44),
      quote_mint: 'So11111111111111111111111111111111111111112',
      decision,
      stage,
      reason,
      pool_size_sol: Number(liqSol.toFixed(2)),
      pool_age_secs: ageSecs,
      velocity_sol_per_sec: Number(velocity.toFixed(3)),
      buy_pressure_ratio: Number(buyPressure.toFixed(5)),
      pool_score: poolScore,
      pumpfun_score: Math.floor(rnd() * 100),
      inflow_rate_sol_per_sec: Number(inflow.toFixed(3)),
      high_speed: rnd() > 0.7,
      dex_boosted: rnd() > 0.9,
      dex_boost_usd: 0,
      social_count: Math.floor(rnd() * 5),
      effective_min_score: minScore,
      dq_action: actionName,
      dq_confidence: Number(confidence.toFixed(3)),
      utc_hour: hourUtc,
    })

    // ── 3. open a position on accept ─────────────────────────────────────────
    if (decision === 'accepted') {
      attempted++
      const landed = rnd() > 0.08
      const elapsedMs = 180 + rnd() * 900

      telemetry.push({
        timestamp: iso(ts),
        executor: 'raydium_v4',
        tx_kind: 'buy',
        signature: fakeMint(rnd).slice(0, 44),
        confirmed: landed,
        error: landed ? '' : 'blockhash not found',
        attempts: landed ? 1 + Math.floor(rnd() * 2) : 3,
        instruction_count: 6,
        compute_unit_limit: 300_000,
        compute_unit_price: Math.floor(50_000 + rnd() * 400_000),
        compute_unit_price_hard_cap: 1_000_000,
        loaded_accounts_data_size_limit: 256_000,
        skip_preflight: true,
        high_speed: rnd() > 0.6,
        elapsed_ms: Math.round(elapsedMs),
        blockhash_fetch_ms_total: Math.round(rnd() * 90),
        send_confirm_ms_total: Math.round(elapsedMs * 0.7),
        retry_delay_ms_total: landed ? 0 : 400,
        timeout_count: landed ? 0 : 1,
        rate_limit_count: rnd() > 0.95 ? 1 : 0,
        slippage_error_count: 0,
        blockhash_error_count: landed ? 0 : 1,
      })

      if (landed) {
        confirmed++
        const sizeMult = actionName === 'BuyAggressive' ? 1.5 : 1.0
        const entry = Math.round((0.04 + rnd() * 0.06) * sizeMult * LAMPORTS)

        // Outcome distribution: meme sniping is mostly small losses with a fat
        // right tail. Higher-scoring pools get a modestly better drift, and ~10%
        // of entries are genuine runners — those are what drive TP escalation.
        const edge = (poolScore - minScore) / 100
        const runner = rnd() < 0.1
        const drift = runner
          ? 0.2 + rnd() * 0.4
          : -0.06 + edge * 0.28 + (rnd() - 0.5) * 0.12
        const vol = runner ? 0.3 + rnd() * 0.35 : 0.15 + rnd() * 0.5

        open.set(mint, {
          mint,
          symbol,
          entryLamports: entry,
          entryTs: ts,
          peakLamports: entry,
          currentLamports: entry,
          tpPct: 100,
          slPct: -15,
          escalations: 0,
          declineStreak: 0,
          drift,
          vol,
          state,
          action,
        })

        trades.push({
          timestamp: iso(ts),
          kind: 'BUY',
          mint,
          symbol,
          amount: entry / LAMPORTS,
          pnl: 0,
          pnl_pct: 0,
          status: '✓',
          signature: fakeMint(rnd).slice(0, 44),
          dex: 'raydium',
          hops: 1,
          position_age_secs: 0,
        })
        logs.push(`${iso(ts)} INFO  sniper: BUY ${symbol} ${(entry / LAMPORTS).toFixed(4)} SOL — DQ* ${actionName} (${(confidence * 100).toFixed(0)}%)`)
      } else {
        failed++
        logs.push(`${iso(ts)} WARN  executor: buy failed for ${symbol} — blockhash not found`)
      }
    }

    // ── 4. advance every open position one tick ──────────────────────────────
    for (const [m, p] of Array.from(open.entries())) {
      const held = ts - p.entryTs
      const shock = (rnd() - 0.5) * 2 * p.vol
      const growth = 1 + p.drift * 0.02 + shock * 0.05
      const prev = p.currentLamports
      const ceiling = p.entryLamports * MAX_POSITION_MULTIPLE
      p.currentLamports = Math.min(ceiling, Math.max(1, Math.round(p.currentLamports * growth)))
      if (p.currentLamports > p.peakLamports) p.peakLamports = p.currentLamports
      p.declineStreak = p.currentLamports < prev ? p.declineStreak + 1 : 0

      const pnlPct = ((p.currentLamports - p.entryLamports) / p.entryLamports) * 100
      const peakPct = ((p.peakLamports - p.entryLamports) / p.entryLamports) * 100

      // Momentum escalation: a runner past target ratchets TP up and locks a floor.
      if (pnlPct >= p.tpPct && p.escalations < 5) {
        p.escalations++
        p.tpPct = Math.round(p.tpPct * 2.2)
        p.slPct = Math.max(p.slPct, Math.round(peakPct * 0.45))
      }

      let exit: string | null = null
      if (pnlPct >= p.tpPct) exit = 'take_profit'
      else if (pnlPct <= p.slPct) exit = 'stop_loss'
      else if (p.declineStreak >= 3 && pnlPct > 8) exit = 'decline_detector'
      else if (held > 900 && pnlPct < 5) exit = 'no_pump_timeout'

      if (exit) {
        const realised = p.currentLamports - p.entryLamports
        realisedLamports += realised
        confirmed++
        attempted++

        trades.push({
          timestamp: iso(ts),
          kind: 'SELL',
          mint: p.mint,
          symbol: p.symbol,
          amount: p.currentLamports / LAMPORTS,
          pnl: realised / LAMPORTS,
          pnl_pct: Number(pnlPct.toFixed(2)),
          status: '✓',
          signature: fakeMint(rnd).slice(0, 44),
          dex: 'raydium',
          hops: 1,
          position_age_secs: held,
        })
        logs.push(`${iso(ts)} INFO  sniper: SELL ${p.symbol} ${pnlPct >= 0 ? '+' : ''}${pnlPct.toFixed(1)}% (${exit})`)

        // Feed the realised outcome back as the reward — this is the signal the
        // network actually learns from (÷100 to match the Rust normalisation),
        // clamped so a single runner can't swamp every other Q-target.
        const reward = Math.max(REWARD_MIN, Math.min(REWARD_MAX, pnlPct / 100))
        agent.observe({ state: p.state, action: p.action, reward, next: state, done: true })
        for (const v of variants) v.reward += reward * (0.85 + rnd() * 0.3)
        open.delete(m)
      }
    }

    if (rnd() > 0.985) arbExecuted++

    if (ev % TRAIN_EVERY === 0) agent.train()
  }

  // ── assemble the snapshot ───────────────────────────────────────────────────

  const readyToAdvise = agent.epsilon <= READY_EPSILON && agent.trainSteps >= READY_TRAIN_STEPS

  // Live advice for the current market state.
  const adviceState = buildState(rnd, 72, 40, 60, 0, open.size, new Date(nowMs).getUTCHours())
  const adviceQ = Array.from(agent.greedyQ(adviceState))
  let bestIdx = 0
  for (let i = 1; i < adviceQ.length; i++) if (adviceQ[i] > adviceQ[bestIdx]) bestIdx = i
  const spread = Math.max(...adviceQ) - Math.min(...adviceQ)
  const meanQ = adviceQ.reduce((a, b) => a + b, 0) / adviceQ.length
  const adviceConfidence = spread > 1e-9 ? clamp01((adviceQ[bestIdx] - meanQ) / spread) : 0
  const adviceName = ACTIONS[bestIdx] as ActionName

  const positions: LivePosition[] = Array.from(open.values()).map((p) => ({
    mint: p.mint,
    entry_lamports: p.entryLamports,
    current_value_lamports: p.currentLamports,
    peak_value_lamports: p.peakLamports,
    entry_unix_secs: p.entryTs,
    dynamic_tp_pct: p.tpPct,
    escalations: p.escalations,
    last_check_unix_secs: nowSecs - Math.floor(rnd() * 3),
    current_sl_lamports: Math.round(p.entryLamports * (1 + p.slPct / 100)),
    current_sl_pct: p.slPct,
    decline_streak: p.declineStreak,
  }))

  const primaryIdx = variants.reduce((best, v, i, arr) => (v.reward > arr[best].reward ? i : best), 0)

  return {
    generatedAt: nowMs,
    uptimeSecs,
    metrics: {
      trades_attempted: attempted,
      trades_confirmed: confirmed,
      trades_failed: failed,
      arb_opportunities_found: arbExecuted * 3,
      arb_executed: arbExecuted,
      total_pnl_lamports: realisedLamports,
      pools_tracked: poolsSeen,
      uptime_secs: uptimeSecs,
    },
    trades: trades.slice(-200).reverse(),
    positions,
    decisions: decisions.slice(-250).reverse(),
    filters: { pools_seen: poolsSeen, pools_passed: poolsPassed, rejections },
    pools: pools.slice(-60).reverse(),
    nn: {
      step_count: agent.stepCount,
      train_steps: agent.trainSteps,
      epsilon: agent.epsilon,
      ready_to_advise: readyToAdvise,
      total_reward: agent.totalReward,
      replay_size: agent.replaySize,
      avg_loss: agent.avgLoss,
      target_updates: agent.targetUpdates,
      last_q_values: agent.lastQ,
    },
    advice: {
      action: readyToAdvise ? adviceName : 'Hold',
      action_index: readyToAdvise ? bestIdx : 0,
      q_values: ACTIONS.map((a, i) => [a, adviceQ[i]] as [string, number]),
      top_reason: readyToAdvise
        ? `Q-spread ${spread.toFixed(3)} favours ${adviceName} at ε=${(agent.epsilon * 100).toFixed(1)}%`
        : `Still exploring — ε=${(agent.epsilon * 100).toFixed(1)}%, ${agent.trainSteps} gradient steps so far`,
      confidence: adviceConfidence,
    },
    tournament: {
      primary_idx: primaryIdx,
      steps_since_eval: agent.stepCount % 1000,
      eval_freq: 1000,
      agent_names: variants.map((v) => v.name),
      agent_total_rewards: variants.map((v) => Number(v.reward.toFixed(2))),
      agent_epsilons: variants.map((v) => Number(v.epsilon.toFixed(4))),
    },
    telemetry: telemetry.slice(-250).reverse(),
    logs: logs.slice(-200),
  }
}

// ── cache ─────────────────────────────────────────────────────────────────────
// A full session replay costs a few hundred ms, so hold the result briefly. The
// engine is deterministic, so a cache miss on a cold lambda yields the same data.

let cached: SimSnapshot | null = null
const CACHE_TTL_MS = 4_000

export function getSnapshot(): SimSnapshot {
  const now = Date.now()
  if (cached && now - cached.generatedAt < CACHE_TTL_MS) return cached
  cached = simulate(now)
  return cached
}
