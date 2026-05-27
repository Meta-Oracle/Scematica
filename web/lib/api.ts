import type {
  FilterStats,
  HealthStatus,
  Metrics,
  NNStats,
  Pool,
  PoolDecision,
  Trade,
  TxTelemetry,
} from './types'

// All requests go to the Next.js proxy route (/api/*), which forwards
// server-side to the Rust API. The browser never touches port 3001 directly.
async function get<T>(path: string, params?: Record<string, string>): Promise<T | null> {
  try {
    const url = new URL(path, typeof window !== 'undefined' ? window.location.origin : 'http://localhost:3000')
    if (params) Object.entries(params).forEach(([k, v]) => url.searchParams.set(k, v))
    const res = await fetch(url.toString(), { cache: 'no-store' })
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
  health:    () => get<HealthStatus>('/api/health'),
  pools:     (limit = 30) => get<{ pools: Pool[]; total: number }>('/api/pools', { limit: String(limit) }),
  logs:      (lines = 80) => get<{ lines: string[] }>('/api/logs', { lines: String(lines) }),
  trades:    (limit = 20) => get<{ trades: Trade[] }>('/api/trades', { limit: String(limit) }),
  decisions: (limit = 40) => get<{ decisions: PoolDecision[] }>('/api/decisions', { limit: String(limit) }),
  txTelemetry: (limit = 40) => get<{ telemetry: TxTelemetry[] }>('/api/tx-telemetry', { limit: String(limit) }),
}
