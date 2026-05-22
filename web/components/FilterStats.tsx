'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { FilterStats as FS } from '@/lib/types'

export function FilterStatsPanel() {
  const [data, setData] = useState<FS | null>(null)

  useEffect(() => {
    let alive = true
    async function poll() {
      const r = await api.filters()
      if (alive && r) setData(r)
    }
    poll()
    const iv = setInterval(poll, 5000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  if (!data) return (
    <div className="panel h-full">
      <div className="panel-header">Filter Stats</div>
      <div className="p-4 text-scema-dim text-xs text-center">Waiting…</div>
    </div>
  )

  const passRate = data.pools_seen > 0
    ? ((data.pools_passed / data.pools_seen) * 100).toFixed(1)
    : '0.0'

  const rejections = Object.entries(data.rejections)
    .sort(([, a], [, b]) => b - a)

  return (
    <div className="panel flex flex-col h-full">
      <div className="panel-header justify-between">
        <span>Filter Pipeline</span>
        <span className="text-scema-muted">{passRate}% pass</span>
      </div>

      {/* Summary bar */}
      <div className="px-3 py-2 border-b border-scema-border flex gap-4">
        <div className="flex flex-col">
          <span className="text-scema-dim text-xs">SEEN</span>
          <span className="text-scema-text font-bold tabular-nums">{data.pools_seen}</span>
        </div>
        <div className="flex flex-col">
          <span className="text-scema-dim text-xs">PASSED</span>
          <span className="text-scema-green font-bold tabular-nums">{data.pools_passed}</span>
        </div>
        <div className="flex flex-col">
          <span className="text-scema-dim text-xs">REJECTED</span>
          <span className="text-scema-red-hi font-bold tabular-nums">
            {data.pools_seen - data.pools_passed}
          </span>
        </div>
      </div>

      {/* Pass rate bar */}
      <div className="px-3 py-2 border-b border-scema-border">
        <div className="w-full h-1 bg-scema-dim rounded-none overflow-hidden">
          <div
            className="h-full bg-gradient-to-r from-scema-red to-scema-red-hi transition-all duration-1000"
            style={{ width: `${Math.min(Number(passRate), 100)}%` }}
          />
        </div>
      </div>

      {/* Rejection breakdown */}
      <div className="flex-1 min-h-0 overflow-y-auto divide-y divide-scema-border/40">
        {rejections.map(([name, count]) => (
          <div key={name} className="flex items-center justify-between px-3 py-1.5 hover:bg-scema-red-bg/10 transition-colors">
            <span className="text-scema-muted text-xs font-mono truncate max-w-[60%]">{name}</span>
            <div className="flex items-center gap-2">
              <div className="w-12 h-0.5 bg-scema-dim overflow-hidden">
                <div
                  className="h-full bg-scema-red-dim"
                  style={{ width: `${data.pools_seen > 0 ? Math.min((count / data.pools_seen) * 100, 100) : 0}%` }}
                />
              </div>
              <span className="text-scema-red-hi text-xs tabular-nums w-6 text-right">{count}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
