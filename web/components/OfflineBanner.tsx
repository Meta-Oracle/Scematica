'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'

export function OfflineBanner() {
  const [offline, setOffline]   = useState(false)
  const [dismissed, setDismissed] = useState(false)

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

  if (!offline || dismissed) return null

  return (
    <div className="border-b border-scema-red/40 bg-scema-red/8 px-4 py-2">
      <div className="max-w-[1600px] mx-auto flex items-center justify-between gap-4 flex-wrap">
        <div className="flex items-center gap-3 flex-wrap text-xs">
          <div className="flex items-center gap-1.5 shrink-0">
            <span className="w-1.5 h-1.5 rounded-full bg-scema-red-hi animate-pulse" />
            <span className="text-scema-red-hi font-bold tracking-widest uppercase">API Offline</span>
          </div>
          <span className="text-scema-muted">
            Panels have no data — start the Rust API server:
          </span>
          <code className="font-mono text-scema-amber bg-black/30 px-2 py-0.5 whitespace-nowrap">
            .\target\release\api.exe
          </code>
          <span className="text-scema-dim">
            then restart the sniper if not running.
          </span>
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
