'use client'

import { api } from './api'
import { fetchRecentPools, type FeedPool } from './feed/jupiter'
import { usePoll, type Snapshot } from './store'
import type {
  ControlState,
  FilterStats,
  HealthStatus,
  LivePosition,
  Metrics,
  NNAdvice,
  NNStats,
  Pool,
  PoolDecision,
  TournamentSnapshot,
  Trade,
  TxTelemetry,
} from './types'

// The canonical endpoint → cadence table. Every panel polls through these hooks rather
// than owning a timer, so a key is fetched exactly once no matter how many components
// want it. Keep new panels on an existing key where possible; a new key means a new
// upstream request every interval, forever.
//
// Cadences are deliberately slower than the old per-component values for the endpoints
// that several panels shared — deduplication already recovered far more headroom than
// the extra latency costs.

export const POLL_MS = {
  metrics:    3_000,
  positions:  3_000,
  controls:   3_000,
  logs:       3_000,
  pools:      4_000,
  trades:     5_000,
  filters:    5_000,
  health:     5_000,
  decisions:  6_000,
  telemetry:  6_000,
  nn:        10_000,
  tournament:10_000,
  /** Public API with rate limits and a slow-moving payload — poll gently. */
  feed:      10_000,
} as const

/** One trades fetch serves every consumer; panels slice the list they need. */
const TRADES_LIMIT = 200

export const useMetrics    = (): Snapshot<Metrics> =>
  usePoll('metrics', api.metrics, POLL_MS.metrics)

export const useHealth     = (): Snapshot<HealthStatus> =>
  usePoll('health', api.health, POLL_MS.health)

export const useControls   = (): Snapshot<ControlState> =>
  usePoll('controls', api.controls, POLL_MS.controls)

export const usePositions  = (): Snapshot<LivePosition[]> =>
  usePoll('positions', api.positions, POLL_MS.positions)

export const useNN         = (): Snapshot<NNStats> =>
  usePoll('nn', api.nn, POLL_MS.nn)

export const useNNAdvice   = (): Snapshot<NNAdvice> =>
  usePoll('nn-advice', api.nnAdvice, POLL_MS.nn)

export const useTournament = (): Snapshot<TournamentSnapshot> =>
  usePoll('tournament', api.tournament, POLL_MS.tournament)

export const useFilterStats = (): Snapshot<FilterStats> =>
  usePoll('filters', api.filters, POLL_MS.filters)

export const usePools      = (): Snapshot<{ pools: Pool[]; total: number }> =>
  usePoll('pools', () => api.pools(30), POLL_MS.pools)

export const useTrades     = (): Snapshot<{ trades: Trade[] }> =>
  usePoll('trades', () => api.trades(TRADES_LIMIT), POLL_MS.trades)

export const useDecisions  = (): Snapshot<{ decisions: PoolDecision[] }> =>
  usePoll('decisions', () => api.decisions(60), POLL_MS.decisions)

export const useTelemetry  = (): Snapshot<{ telemetry: TxTelemetry[] }> =>
  usePoll('telemetry', () => api.txTelemetry(60), POLL_MS.telemetry)

export const useLogs       = (): Snapshot<{ lines: string[] }> =>
  usePoll('logs', () => api.logs(80), POLL_MS.logs)

/**
 * Live new-mint feed straight from Jupiter — no bot, no proxy, no API key.
 * This is the one key that does not go through `/api/*`.
 */
export const useFeed       = (): Snapshot<FeedPool[]> =>
  usePoll('feed', fetchRecentPools, POLL_MS.feed)
