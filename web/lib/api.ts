import type {
  FilterStats,
  HealthStatus,
  IntelligenceSnapshot,
  NNAdvice,
  Metrics,
  NNStats,
  Pool,
  PoolDecision,
  Trade,
  TxTelemetry,
} from './types'

import { apiFetch } from './net'

// On web, requests go to the same-origin Next.js proxy (/api/*), which forwards
// server-side to the Rust API. On a paired mobile build, `apiFetch` retargets them at
// the operator's own instance and attaches the bearer token (see lib/net.ts).
async function get<T>(path: string, params?: Record<string, string>): Promise<T | null> {
  try {
    const qs = params ? '?' + new URLSearchParams(params).toString() : ''
    const res = await apiFetch(path + qs)
    if (!res.ok) return null
    return res.json()
  } catch {
    return null
  }
}

export const api = {
  metrics:   () => get<Metrics>('/api/metrics'),
  filters:   () => get<FilterStats>('/api/filters'),
  nn:        () => get<NNStats>('/api/nn'),
  nnAdvice:  () => get<NNAdvice>('/api/nn-advice'),
  intelligence: (limit = 60) => get<IntelligenceSnapshot>('/api/intelligence', { limit: String(limit) }),
  health:    () => get<HealthStatus>('/api/health'),
  pools:     (limit = 30) => get<{ pools: Pool[]; total: number }>('/api/pools', { limit: String(limit) }),
  logs:      (lines = 80) => get<{ lines: string[] }>('/api/logs', { lines: String(lines) }),
  trades:    (limit = 20) => get<{ trades: Trade[] }>('/api/trades', { limit: String(limit) }),
  decisions: (limit = 40) => get<{ decisions: PoolDecision[] }>('/api/decisions', { limit: String(limit) }),
  txTelemetry: (limit = 40) => get<{ telemetry: TxTelemetry[] }>('/api/tx-telemetry', { limit: String(limit) }),
}
