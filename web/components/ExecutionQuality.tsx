'use client'

import { useEffect, useMemo, useState } from 'react'
import { api } from '@/lib/api'
import type { TxTelemetry } from '@/lib/types'

function fmtTime(ts: string) {
  try {
    return new Date(ts).toLocaleTimeString('en-GB', { hour12: false })
  } catch {
    return ts?.slice(11, 19) || '--:--:--'
  }
}

function avg(values: number[]) {
  if (values.length === 0) return 0
  return values.reduce((sum, value) => sum + value, 0) / values.length
}

export function ExecutionQuality() {
  const [rows, setRows] = useState<TxTelemetry[] | null>(null)

  useEffect(() => {
    let alive = true
    async function poll() {
      const r = await api.txTelemetry(60)
      if (!alive) return
      setRows(r?.telemetry ?? [])
    }
    poll()
    const iv = setInterval(poll, 5000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  const stats = useMemo(() => {
    const data = rows ?? []
    const landed = data.filter(t => t.confirmed).length
    const landedRate = data.length > 0 ? (landed / data.length) * 100 : 0
    const avgMs = avg(data.map(t => t.elapsed_ms ?? 0))
    const avgAttempts = avg(data.map(t => t.attempts ?? 0))
    const rateLimits = data.reduce((sum, t) => sum + (t.rate_limit_count ?? 0), 0)
    const timeouts = data.reduce((sum, t) => sum + (t.timeout_count ?? 0), 0)
    return { landed, landedRate, avgMs, avgAttempts, rateLimits, timeouts }
  }, [rows])

  return (
    <div className="panel flex flex-col h-full">
      <div className="panel-header justify-between">
        <span>Execution Quality</span>
        <span className="text-scema-muted">{rows === null ? '...' : rows.length}</span>
      </div>

      <div className="grid grid-cols-3 divide-x divide-scema-border border-b border-scema-border text-xs">
        <div className="flex flex-col px-3 py-2">
          <span className="text-scema-dim">LANDED</span>
          <span className="text-scema-green font-bold tabular-nums">{stats.landedRate.toFixed(1)}%</span>
        </div>
        <div className="flex flex-col px-3 py-2">
          <span className="text-scema-dim">AVG MS</span>
          <span className="text-scema-text font-bold tabular-nums">{stats.avgMs.toFixed(0)}</span>
        </div>
        <div className="flex flex-col px-3 py-2">
          <span className="text-scema-dim">RETRY</span>
          <span className="text-scema-amber font-bold tabular-nums">{stats.avgAttempts.toFixed(1)}x</span>
        </div>
      </div>

      <div className="px-3 py-1.5 border-b border-scema-border flex justify-between text-xs">
        <span className="text-scema-dim">rate limits <span className="text-scema-red-hi tabular-nums">{stats.rateLimits}</span></span>
        <span className="text-scema-dim">timeouts <span className="text-scema-red-hi tabular-nums">{stats.timeouts}</span></span>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto">
        {rows === null && (
          <div className="p-4 text-scema-dim text-xs text-center">Loading...</div>
        )}
        {rows !== null && rows.length === 0 && (
          <div className="p-4 text-scema-dim text-xs text-center">No execution telemetry yet</div>
        )}
        {rows !== null && rows.length > 0 && (
          <table className="w-full text-xs border-collapse">
            <thead className="sticky top-0 z-10 bg-scema-panel">
              <tr className="text-scema-dim border-b border-scema-border">
                <th className="text-left px-2 py-1.5 font-normal">TIME</th>
                <th className="text-left px-2 py-1.5 font-normal">EXEC</th>
                <th className="text-left px-2 py-1.5 font-normal">KIND</th>
                <th className="text-right px-2 py-1.5 font-normal">MS</th>
                <th className="text-right px-2 py-1.5 font-normal">TRY</th>
                <th className="text-left px-2 py-1.5 font-normal">RESULT</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((t, i) => (
                <tr key={`${t.timestamp}-${i}`} className="border-b border-scema-border/20 hover:bg-white/5">
                  <td className="px-2 py-1 font-mono text-scema-dim tabular-nums whitespace-nowrap">
                    {fmtTime(t.timestamp)}
                  </td>
                  <td className="px-2 py-1 font-mono text-scema-muted">{t.executor}</td>
                  <td className="px-2 py-1 font-mono text-scema-muted">{t.tx_kind}</td>
                  <td className="px-2 py-1 text-right font-mono tabular-nums text-scema-text">
                    {(t.elapsed_ms ?? 0).toFixed(0)}
                  </td>
                  <td className="px-2 py-1 text-right font-mono tabular-nums text-scema-muted">
                    {t.attempts ?? 0}
                  </td>
                  <td
                    className={`px-2 py-1 truncate max-w-[160px] ${t.confirmed ? 'text-scema-green' : 'text-scema-red-hi'}`}
                    title={t.error || t.signature}
                  >
                    {t.confirmed ? 'landed' : (t.error || 'failed')}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  )
}
