import type { FilterStats, HealthStatus, Metrics, NNStats, Pool, Trade } from './types'

const BASE = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001'

async function get<T>(path: string, params?: Record<string, string>): Promise<T | null> {
  try {
    const url = new URL(`${BASE}${path}`)
    if (params) Object.entries(params).forEach(([k, v]) => url.searchParams.set(k, v))
    const res = await fetch(url.toString(), { next: { revalidate: 0 } })
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
  logs:      (lines = 60) => get<{ lines: string[] }>('/api/logs', { lines: String(lines) }),
  trades:    (limit = 20) => get<{ trades: Trade[] }>('/api/trades', { limit: String(limit) }),
}
