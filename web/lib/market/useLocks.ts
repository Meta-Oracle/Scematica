'use client'

import { useEffect, useRef, useState } from 'react'

import type { LockLookup } from './commitment'

// Tier-3 enrichment for the rows currently on screen.
//
// Deliberately NOT part of useMarket. These are indexed memcmp queries against programs
// holding 800k+ accounts — fast individually (~65-250ms) but not something to put in
// front of a 200-row board on a 30s poll. So: the board renders immediately with lock
// status "not checked", and this fills in the top rows behind it.
//
// Results are cached by mint for the lifetime of the page. A lock contract appearing or
// disappearing is a once-in-a-token's-life event, not a tick, so re-polling it would be
// load without information.

// Must not exceed MAX_MINTS in app/api/market/locks/route.ts, which truncates silently
// past its own cap. Kept in step so the client never asks for answers it cannot get.
const MAX_PER_REQUEST = 8

interface LocksResponse {
  ok: boolean
  locks?: Record<string, LockLookup>
}

export function useLocks(mints: string[]): Record<string, LockLookup> {
  const [locks, setLocks] = useState<Record<string, LockLookup>>({})
  // `requested` guards against re-asking for a mint already in flight or answered —
  // without it, every board refresh would re-issue the same queries.
  const requested = useRef<Set<string>>(new Set())

  const wanted = mints.slice(0, MAX_PER_REQUEST).filter(m => !requested.current.has(m))
  const key = wanted.join(',')

  useEffect(() => {
    if (!key) return
    const batch = key.split(',')
    for (const m of batch) requested.current.add(m)

    let alive = true
    ;(async () => {
      try {
        const res = await fetch(`/api/market/locks?mints=${encodeURIComponent(key)}`, {
          cache: 'no-store',
        })
        if (!res.ok) throw new Error(`HTTP ${res.status}`)
        const json = (await res.json()) as LocksResponse
        if (!alive || !json.ok || !json.locks) return
        setLocks(prev => ({ ...prev, ...json.locks }))
      } catch {
        // Drop the guard so a transient failure can be retried on a later render.
        // Leaving these mints as "not checked" is correct: the grader treats an absent
        // entry as unknown, never as "no locks".
        for (const m of batch) requested.current.delete(m)
      }
    })()

    return () => {
      alive = false
    }
  }, [key])

  return locks
}
