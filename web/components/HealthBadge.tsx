'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { HealthStatus } from '@/lib/types'

export function HealthBadge() {
  const [health, setHealth] = useState<HealthStatus | null>(null)

  useEffect(() => {
    let alive = true
    async function poll() {
      const h = await api.health()
      if (alive) setHealth(h)
    }
    poll()
    const iv = setInterval(poll, 5_000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  const apiOk     = health?.api === 'ok'
  const sniperOk  = health?.sniper_running === true
  const connected = health !== null

  return (
    <div className="hidden md:flex items-center gap-3 text-xs">
      <div className="flex items-center gap-1.5">
        <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${
          !connected ? 'bg-scema-dim' : apiOk ? 'bg-scema-green animate-pulse' : 'bg-scema-red-hi'
        }`} />
        <span className={!connected ? 'text-scema-dim' : apiOk ? 'text-scema-green' : 'text-scema-red-hi'}>
          API
        </span>
      </div>
      <div className="flex items-center gap-1.5">
        <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${
          sniperOk ? 'bg-scema-green animate-pulse' : connected ? 'bg-scema-dim' : 'bg-scema-dim'
        }`} />
        <span className={sniperOk ? 'text-scema-green' : 'text-scema-dim'}>
          SNIPER {sniperOk ? 'LIVE' : 'OFFLINE'}
        </span>
      </div>
    </div>
  )
}
