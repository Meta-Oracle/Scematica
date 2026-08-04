'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import { getPairing } from '@/lib/net'
import { Pairing } from './Pairing'

// Shown whenever no bot is reachable — either no instance was ever paired (a cold
// visit to a standalone deploy of this dashboard, e.g. hosted publicly with no
// Rust API of its own) or a previously-paired instance has gone offline. Either way
// the fix is the same "Pair with it" flow, so both cases share this one banner
// instead of only pointing people at a same-machine `api.exe`.
export function OfflineBanner() {
  const [offline, setOffline]   = useState(false)
  const [dismissed, setDismissed] = useState(false)
  const [pairing, setPairing]   = useState(false)

  useEffect(() => {
    let alive = true
    async function check() {
      const h = await api.health()
      if (!alive) return
      const down = h === null
      setOffline(down)
      if (!down) setDismissed(false) // auto-restore when API comes back
    }
    check()
    const iv = setInterval(check, 5_000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  if (pairing) {
    return (
      <div className="fixed inset-0 z-[100] bg-scema-black overflow-y-auto">
        <button
          onClick={() => setPairing(false)}
          aria-label="Cancel pairing"
          className="absolute top-4 right-4 z-10 text-scema-dim hover:text-scema-muted text-lg"
        >
          ✕
        </button>
        <Pairing onPaired={() => setPairing(false)} />
      </div>
    )
  }

  if (!offline || dismissed) return null

  const alreadyPaired = getPairing() !== null

  return (
    <div className="border-b border-scema-red/40 bg-scema-red/8 px-4 py-2">
      <div className="max-w-[1600px] mx-auto flex items-center justify-between gap-4 flex-wrap">
        <div className="flex items-center gap-3 flex-wrap text-xs">
          <div className="flex items-center gap-1.5 shrink-0">
            <span className="w-1.5 h-1.5 rounded-full bg-scema-red-hi animate-pulse" />
            <span className="text-scema-red-hi font-bold tracking-widest uppercase">
              {alreadyPaired ? 'Paired Instance Offline' : 'No Bot Connected'}
            </span>
          </div>
          <span className="text-scema-muted">
            Panels have no data.
          </span>
          <button
            onClick={() => setPairing(true)}
            className="text-xs px-2 py-1 border border-scema-red/50 text-scema-red-hi
                       hover:bg-scema-red/10 font-bold uppercase tracking-widest transition-colors"
          >
            {alreadyPaired ? 'Re-pair ↗' : 'Pair with your instance ↗'}
          </button>
          <span className="text-scema-dim">
            or, if it's on this machine, run:
          </span>
          <code className="font-mono text-scema-amber bg-black/30 px-2 py-0.5 whitespace-nowrap">
            .\target\release\api.exe
          </code>
        </div>
        <button
          onClick={() => setDismissed(true)}
          aria-label="Dismiss"
          className="text-scema-dim hover:text-scema-muted transition-colors shrink-0 text-base leading-none"
        >
          ✕
        </button>
      </div>
    </div>
  )
}
