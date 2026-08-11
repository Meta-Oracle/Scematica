'use client'

// Opportunistic generated portraits for the avatar.
//
// This hook returns a *source override* per expression, never a new expression and never
// a timing signal. That separation is the whole design: `expressions.ts` stays pure and
// keeps deciding which frame is showing and how she is posed, and this only changes what
// pixels that frame points at. Wiring generation into the state machine instead would
// couple an animation that must never stutter to a network call that takes seconds.
//
// Every failure mode ends in "keep using sprites", silently:
//
//   - generation disabled, no backend, ComfyUI down, IPAdapter nodes missing → the probe
//     says not-ok and nothing is ever requested. One probe per mount, no retry storm.
//   - an individual generation fails → that expression keeps its sprite, the others carry
//     on. A partial set is fine because the sprite and the portrait are the same
//     character in the same framing.
//
// Requests are issued **one at a time**. There is a single GPU behind this; three
// parallel POSTs do not finish three times sooner, they just queue inside ComfyUI and
// make every one of them look like a timeout.

import { useEffect, useState } from 'react'

import { EXPRESSIONS, type Expression } from './expressions'

export type PortraitSources = Partial<Record<Expression, string>>

interface Probe {
  ok?: boolean
  enabled?: boolean
  active?: string | null
}

export function usePortraits(enabled = true): PortraitSources {
  const [sources, setSources] = useState<PortraitSources>({})

  useEffect(() => {
    if (!enabled) return

    let cancelled = false
    // Tracked separately from state so cleanup can revoke every URL it created, including
    // ones produced after the last render. Leaking object URLs holds the decoded bitmap
    // in memory for the life of the document.
    const created: string[] = []

    void (async () => {
      let probe: Probe
      try {
        const res = await fetch('/api/scylar/portrait', { cache: 'no-store' })
        probe = (await res.json()) as Probe
      } catch {
        return
      }
      if (cancelled || !probe.ok) return

      for (const mood of EXPRESSIONS) {
        if (cancelled) return
        try {
          const res = await fetch('/api/scylar/portrait', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ mood }),
          })
          // A JSON body here is a refusal, not an image — the route never dresses a
          // sprite up as a generation, so there is nothing to salvage from it.
          if (!res.ok || !res.headers.get('Content-Type')?.startsWith('image/')) continue

          const url = URL.createObjectURL(await res.blob())
          created.push(url)
          if (cancelled) {
            URL.revokeObjectURL(url)
            return
          }
          setSources((prev) => ({ ...prev, [mood]: url }))
        } catch {
          // Keep going: one failed mood must not cost the other two.
        }
      }
    })()

    return () => {
      cancelled = true
      for (const url of created) URL.revokeObjectURL(url)
    }
  }, [enabled])

  return sources
}
