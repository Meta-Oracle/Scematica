'use client'

import { useEffect, useRef, useState } from 'react'

import type { MarketRow, SourceStatus } from './types'
import type { MarketStats } from './aggregate'

// The ONLY timer on the Escrow Market page.
//
// `web/`'s polling rule is one timer per endpoint, with panels subscribing rather than
// calling setInterval themselves — a private timer in a panel silently undoes the
// refcounting that stops hidden panels fetching. This page has many panels and one
// endpoint, so the timer lives here and every panel takes rows as props.
//
// A failed poll RETAINS the previous snapshot and records the error, rather than
// blanking the board. An empty market and an unreachable API look identical once you
// clear the rows, and only one of them is true.

export interface CustodyMeta {
  available: boolean
  error: string | null
  programId: string | null
  vaultCount: number
  measuredAtSlot: number | null
  rpc: { host: string | null; authenticated: boolean }
}

export interface MarketPayload {
  ok: boolean
  rows: MarketRow[]
  sources: SourceStatus[]
  stats: MarketStats
  custody: CustodyMeta
  fetchedAt: number
}

export interface MarketState {
  data: MarketPayload | null
  loading: boolean
  /** Set when the LAST poll failed. The `data` beside it may still be good. */
  error: string | null
  lastGoodAt: number | null
  refresh: () => void
}

const POLL_MS = 30_000

export function useMarket(): MarketState {
  const [data, setData] = useState<MarketPayload | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [lastGoodAt, setLastGoodAt] = useState<number | null>(null)
  const [nonce, setNonce] = useState(0)
  const alive = useRef(true)

  useEffect(() => {
    alive.current = true
    let timer: ReturnType<typeof setTimeout> | null = null

    const tick = async () => {
      try {
        const res = await fetch('/api/market', { cache: 'no-store' })
        if (!res.ok) throw new Error(`HTTP ${res.status}`)
        const json = (await res.json()) as MarketPayload
        if (!alive.current) return
        setData(json)
        setLastGoodAt(Date.now())
        setError(null)
      } catch (e) {
        if (!alive.current) return
        // Keep `data` — a stale board labelled stale beats an empty board labelled
        // nothing. The banner reads off `error`.
        setError(e instanceof Error ? e.message : String(e))
      } finally {
        if (alive.current) {
          setLoading(false)
          timer = setTimeout(tick, POLL_MS)
        }
      }
    }

    void tick()
    return () => {
      alive.current = false
      if (timer) clearTimeout(timer)
    }
  }, [nonce])

  return { data, loading, error, lastGoodAt, refresh: () => setNonce(n => n + 1) }
}

/** Re-render on a wall clock so "3s ago" ages without each row owning a timer. */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs)
    return () => clearInterval(id)
  }, [intervalMs])
  return now
}
