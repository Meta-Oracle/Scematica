'use client'

import { useEffect, useState } from 'react'
import { api } from './api'

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
  const [source, setSource] = useState<DataSource>('loading')

  useEffect(() => {
    let alive = true
    async function check() {
      const h = await api.health()
      if (!alive) return
      if (h === null) setSource('offline')
      else setSource(h.simulated ? 'simulation' : 'live')
    }
    check()
    const iv = setInterval(check, 10_000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  return source
}
