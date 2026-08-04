'use client'

import { useHealth } from './queries'

export type DataSource = 'loading' | 'live' | 'simulation' | 'offline'

/**
 * Where the dashboard's numbers are coming from right now.
 *
 * - `live`       a real paired/local sniper is answering
 * - `simulation` the built-in self-contained engine (no bot anywhere)
 * - `offline`    nothing answered at all
 *
 * Every panel renders the same either way, so this must be surfaced prominently —
 * simulated PnL must never be mistaken for real money.
 */
export function useDataSource(): DataSource {
  // Reads the shared 'health' key — no timer of its own.
  const { data, ok, loading } = useHealth()
  if (loading) return 'loading'
  if (!ok || data === null) return 'offline'
  return data.simulated ? 'simulation' : 'live'
}
