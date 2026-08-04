'use client'

import { useMemo } from 'react'
import { useFeed } from './queries'
import { useDataSource } from './useDataSource'
import { evaluatePool, aggregateStats, type Evaluation } from './feed/scorer'
import type { FilterStats, Pool, PoolDecision } from './types'

// Discovery data for the pool panels.
//
// Source precedence:
//   1. A paired/local sniper, when one is answering — it sees real vaults over a
//      WebSocket and its verdicts are the ones that actually spent money.
//   2. Otherwise the public Jupiter feed scored by the ported pipeline. Real mints,
//      real sizes, real filter verdicts — just a slower, thinner view than the bot's.
//
// Note what is deliberately absent: there is no third "make something up" branch.
// When neither source has data the panels render empty.

export type DiscoverySource = 'live' | 'feed'

export interface Discovery {
  source: DiscoverySource
  /** True while the very first fetch is outstanding. */
  loading: boolean
  evaluations: Evaluation[]
  pools: Pool[]
  stats: FilterStats
  decisions: PoolDecision[]
}

const EMPTY_STATS: FilterStats = { pools_seen: 0, pools_passed: 0, rejections: {} }

/** Shape one evaluation like the sniper's Pool row. */
function toPool(e: Evaluation): Pool {
  return {
    mint: e.pool.mint,
    score: e.score,
    size_sol: e.pool.sizeSol,
    age_secs: e.pool.ageSecs,
    passed_filters: e.decision === 'accepted',
    timestamp: e.pool.createdAtUnix,
  }
}

/**
 * Shape one evaluation like a PoolDecision row. Fields the feed cannot supply are
 * left at zero rather than invented — an empty INFLOW column is the truth here.
 */
function toDecision(e: Evaluation): PoolDecision {
  return {
    timestamp: new Date(e.pool.createdAtUnix * 1000).toISOString(),
    mint: e.pool.mint,
    pool: '',
    quote_mint: '',
    decision: e.decision,
    stage: e.stage,
    reason: e.reason,
    pool_size_sol: e.pool.sizeSol,
    pool_age_secs: e.pool.ageSecs,
    velocity_sol_per_sec: e.pool.ageSecs > 0 ? e.pool.sizeSol / e.pool.ageSecs : 0,
    buy_pressure_ratio: 0,
    pool_score: e.score,
    pumpfun_score: 0,
    inflow_rate_sol_per_sec: 0,
    high_speed: false,
    dex_boosted: false,
    dex_boost_usd: 0,
    social_count: 0,
    effective_min_score: 0,
    dq_action: '',
    dq_confidence: 0,
    utc_hour: new Date(e.pool.createdAtUnix * 1000).getUTCHours(),
  }
}

export function useDiscovery(): Discovery {
  const dataSource = useDataSource()
  const live = dataSource === 'live'

  // Subscribing unconditionally would keep the public feed polling even when a real
  // bot is answering, which wastes someone else's rate limit for data we discard.
  const feed = useFeed()
  const feedPools = live ? null : feed.data

  return useMemo(() => {
    const evaluations = (feedPools ?? []).map(evaluatePool)
    return {
      source: live ? 'live' : 'feed',
      // On the live branch the panels read their own `/api/*` snapshot, so this hook
      // has nothing outstanding of its own.
      loading: live ? false : feed.loading,
      evaluations,
      pools: evaluations.map(toPool),
      stats: feedPools ? aggregateStats(evaluations) : EMPTY_STATS,
      decisions: evaluations.map(toDecision),
    }
  }, [feedPools, live, feed.loading])
}
