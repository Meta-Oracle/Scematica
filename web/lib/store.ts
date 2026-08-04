'use client'

import { useCallback, useSyncExternalStore } from 'react'

// Shared polling store.
//
// Every panel used to own a `setInterval` + `useState` pair, so the dashboard opened
// ~18 independent timers and issued ~245 requests/min — with `/api/health` fetched by
// three components, `/api/positions` by two and `/api/trades` by four. Against a local
// bot that was merely wasteful; against rate-limited public APIs (see lib/feed/) it is
// disqualifying.
//
// Here each endpoint is a *key* with exactly one timer, fanned out to every subscriber.
// Two properties follow that the old per-component pattern could not give:
//
//   1. Duplicate consumers cost nothing — 3 health subscribers = 1 request.
//   2. Polling is refcounted, so a hidden panel stops fetching. In Beginner mode the
//      AdvancedOnly panels unmount and their upstreams go quiet automatically.
//
// A failed poll keeps the last good `data` and flips `ok` to false, so stale-tolerant
// panels keep rendering while OfflineBanner/useDataSource can still see the outage.

export interface Snapshot<T> {
  /** Last successful value, retained across a failed poll. */
  data: T | null
  /** Did the most recent poll succeed? */
  ok: boolean
  /** True until the first poll of this key settles. */
  loading: boolean
}

type Fetcher<T> = () => Promise<T | null>

interface Entry {
  snapshot: Snapshot<unknown>
  fetcher: Fetcher<unknown>
  intervalMs: number
  timer: ReturnType<typeof setInterval> | null
  subs: Set<() => void>
  inFlight: boolean
}

const LOADING: Snapshot<never> = { data: null, ok: false, loading: true }
const entries = new Map<string, Entry>()

function emit(e: Entry) {
  e.subs.forEach(fn => fn())
}

async function refresh(key: string): Promise<void> {
  const e = entries.get(key)
  // Skip if a poll is still in flight — a slow upstream must not queue up a backlog
  // of overlapping requests behind it.
  if (!e || e.inFlight) return
  e.inFlight = true
  try {
    const next = await e.fetcher()
    const cur = entries.get(key)
    if (!cur) return
    cur.snapshot =
      next === null
        ? { data: cur.snapshot.data, ok: false, loading: false }
        : { data: next, ok: true, loading: false }
    emit(cur)
  } catch {
    const cur = entries.get(key)
    if (cur) {
      cur.snapshot = { data: cur.snapshot.data, ok: false, loading: false }
      emit(cur)
    }
  } finally {
    const cur = entries.get(key)
    if (cur) cur.inFlight = false
  }
}

function ensure<T>(key: string, fetcher: Fetcher<T>, intervalMs: number): Entry {
  let e = entries.get(key)
  if (!e) {
    e = {
      snapshot: LOADING,
      fetcher: fetcher as Fetcher<unknown>,
      intervalMs,
      timer: null,
      subs: new Set(),
      inFlight: false,
    }
    entries.set(key, e)
  } else {
    // Keep the newest fetcher (closures capture fresh props) and honour the fastest
    // cadence any live subscriber asked for — restarting a running timer if the new
    // subscriber wants it faster than the one already ticking.
    e.fetcher = fetcher as Fetcher<unknown>
    if (intervalMs < e.intervalMs) {
      e.intervalMs = intervalMs
      if (e.timer) {
        clearInterval(e.timer)
        e.timer = setInterval(() => void refresh(key), e.intervalMs)
      }
    }
  }
  return e
}

function subscribeKey<T>(
  key: string,
  fetcher: Fetcher<T>,
  intervalMs: number,
  cb: () => void,
): () => void {
  const e = ensure(key, fetcher, intervalMs)
  const first = e.subs.size === 0
  e.subs.add(cb)

  if (first) {
    void refresh(key)
    e.timer = setInterval(() => void refresh(key), e.intervalMs)
  }

  return () => {
    const cur = entries.get(key)
    if (!cur) return
    cur.subs.delete(cb)
    if (cur.subs.size === 0 && cur.timer) {
      // Last subscriber left: stop the network work but keep the data, so a remount
      // (tab switch, Beginner ⇄ Pro toggle) paints instantly from cache.
      clearInterval(cur.timer)
      cur.timer = null
    }
  }
}

/**
 * Subscribe to a polled endpoint. Every caller passing the same `key` shares one
 * timer and one in-flight request.
 */
export function usePoll<T>(key: string, fetcher: Fetcher<T>, intervalMs: number): Snapshot<T> {
  const subscribe = useCallback(
    (cb: () => void) => subscribeKey(key, fetcher, intervalMs, cb),
    // `fetcher` is intentionally excluded: inline arrow props change identity every
    // render, which would tear down and rebuild the subscription on each paint.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key, intervalMs],
  )
  const getSnapshot = useCallback(
    () => (entries.get(key)?.snapshot ?? LOADING) as Snapshot<T>,
    [key],
  )
  const getServerSnapshot = useCallback(() => LOADING as Snapshot<T>, [])

  return useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot)
}

/** Force an immediate re-poll of a key (e.g. right after a control POST). */
export function invalidate(key: string): void {
  void refresh(key)
}

// No `reset()` here on purpose: the only event that invalidates every key at once is a
// pairing change, and `Pairing.pair()` reloads the page, which clears the module anyway.
