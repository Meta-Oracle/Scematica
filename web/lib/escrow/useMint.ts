'use client'

import { useEffect, useState } from 'react'

import { looksLikeAddress, type MintResolution } from './mintinfo'

// One mint lookup per address, shared by every component that asks for it.
//
// This follows `web/`'s one-timer-per-endpoint rule in spirit: the picker previews a
// pasted address while the builder simultaneously needs the same mint's decimals, and
// without a shared cache that is two identical chain reads per keystroke-settle. The
// cache is module-level and process-lifetime — a mint's decimals and token program are
// immutable, and supply/authorities are re-read on a page load, which is often enough for
// a figure that is shown as context rather than computed with.
//
// In-flight requests are deduped by promise, not just results, so the picker and the
// builder asking at the same moment produce one fetch rather than two.

const cache = new Map<string, MintResolution>()
const inflight = new Map<string, Promise<MintResolution>>()

export function cachedMint(address: string): MintResolution | undefined {
  return cache.get(address)
}

export async function resolveMint(address: string): Promise<MintResolution> {
  const hit = cache.get(address)
  if (hit) return hit
  const running = inflight.get(address)
  if (running) return running

  const p = (async (): Promise<MintResolution> => {
    try {
      const res = await fetch(`/api/escrow/mint?address=${encodeURIComponent(address)}`, {
        cache: 'no-store',
      })
      const json = (await res.json()) as MintResolution
      // Failures are cached too, but only the deterministic ones. An rpc_failed is about
      // the network at that instant, not about the mint, so caching it would make a
      // transient blip look like a permanent property of the address.
      if (json.ok || json.reason !== 'rpc_failed') cache.set(address, json)
      return json
    } catch (e) {
      return {
        ok: false,
        reason: 'rpc_failed',
        detail: e instanceof Error ? e.message : String(e),
      }
    } finally {
      inflight.delete(address)
    }
  })()

  inflight.set(address, p)
  return p
}

export interface MintLookup {
  /** `true` while a read is outstanding — never render a figure during this. */
  loading: boolean
  result: MintResolution | null
}

/**
 * Resolve `address` against the chain, debounced.
 *
 * Passing `null` (or a string that is not address-shaped) resolves to idle rather than
 * an error: the picker calls this on every keystroke, and "you have typed four
 * characters" is not a failure worth rendering.
 */
export function useMintLookup(address: string | null, debounceMs = 350): MintLookup {
  const usable = address && looksLikeAddress(address) ? address.trim() : null
  const [state, setState] = useState<MintLookup>(() => {
    const hit = usable ? cache.get(usable) : undefined
    return { loading: Boolean(usable) && !hit, result: hit ?? null }
  })

  useEffect(() => {
    if (!usable) {
      setState({ loading: false, result: null })
      return
    }
    const hit = cache.get(usable)
    if (hit) {
      setState({ loading: false, result: hit })
      return
    }

    let alive = true
    setState({ loading: true, result: null })
    const id = setTimeout(() => {
      void resolveMint(usable).then(r => {
        if (alive) setState({ loading: false, result: r })
      })
    }, debounceMs)

    return () => {
      alive = false
      clearTimeout(id)
    }
  }, [usable, debounceMs])

  return state
}
