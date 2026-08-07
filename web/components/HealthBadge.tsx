'use client'

import { useHealth } from '@/lib/queries'

export function HealthBadge() {
  // Shared 'health' key — OfflineBanner and useDataSource read the same single poll.
  const { data, ok, loading } = useHealth()
  const health = loading ? 'loading' : ok ? data : null

  if (health === 'loading') {
    return (
      <div className="hidden md:flex items-center gap-1.5 text-xs text-scema-dim">
        <span className="w-1.5 h-1.5 rounded-full bg-scema-dim animate-pulse" />
        API …
      </div>
    )
  }

  // null = fetch returned null = 502/503/network error = API server down
  if (health === null) {
    return (
      <div className="hidden md:flex items-center gap-2 px-2 py-0.5 border border-scema-red/50
                      bg-scema-red/5 text-scema-red-hi text-xs">
        <span className="w-1.5 h-1.5 rounded-full bg-scema-red-hi animate-pulse shrink-0" />
        API OFFLINE
      </div>
    )
  }

  // Simulation must never render as a live sniper in the status chrome.
  if (health.simulated) {
    return (
      <div className="hidden md:flex items-center gap-2 px-2 py-0.5 border border-scema-amber/50
                      bg-scema-amber/5 text-scema-amber text-xs">
        <span className="w-1.5 h-1.5 rounded-full bg-scema-amber animate-pulse shrink-0" />
        SIMULATION
      </div>
    )
  }

  const apiOk    = health.api === 'ok'
  const sniperOk = health.sniper_running === true

  return (
    <div className="hidden md:flex items-center gap-3 text-xs">
      <div className="flex items-center gap-1.5">
        <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${
          apiOk ? 'bg-scema-green animate-pulse' : 'bg-scema-red-hi'
        }`} />
        <span className={apiOk ? 'text-scema-green' : 'text-scema-red-hi'}>
          API {apiOk ? 'OK' : 'ERR'}
        </span>
      </div>
      <div className="flex items-center gap-1.5">
        <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${
          sniperOk ? 'bg-scema-green animate-pulse' : 'bg-scema-amber'
        }`} />
        <span className={sniperOk ? 'text-scema-green' : 'text-scema-amber'}>
          SNIPER {sniperOk ? 'LIVE' : 'OFFLINE'}
        </span>
      </div>
    </div>
  )
}
