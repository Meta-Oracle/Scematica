'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { Trade } from '@/lib/types'

interface Position {
  mint: string
  entryAmount: number  // SOL spent
  entryTime: string
}

function shortMint(mint: string) {
  return `${mint.slice(0, 5)}…${mint.slice(-4)}`
}

function fmtAge(ts: string) {
  const ms = Date.now() - new Date(ts).getTime()
  const s = Math.floor(ms / 1000)
  if (s < 60)   return `${s}s`
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`
}

export function OpenPositions() {
  const [positions, setPositions] = useState<Position[] | null>(null)

  useEffect(() => {
    let alive = true
    async function poll() {
      const r = await api.trades(200)
      if (!alive || !r?.trades) return

      // trades are newest-first; track which mints have been sold
      const sold = new Set<string>()
      const open = new Map<string, Position>()

      for (const t of r.trades as Trade[]) {
        if (t.kind === 'SELL' && t.status === '✓') {
          sold.add(t.mint)
        }
        if (t.kind === 'BUY' && t.status === '✓') {
          // If we haven't seen a SELL for this mint yet and it's not already noted as open
          if (!sold.has(t.mint) && !open.has(t.mint)) {
            open.set(t.mint, {
              mint: t.mint,
              entryAmount: t.amount ?? 0,
              entryTime: t.timestamp,
            })
          }
        }
      }

      setPositions(Array.from(open.values()))
    }
    poll()
    const iv = setInterval(poll, 5_000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  const count = positions?.length ?? 0

  return (
    <div className="panel flex flex-col h-full">
      <div className="panel-header justify-between">
        <span>Open Positions</span>
        <span className={`text-xs ${count > 0 ? 'text-scema-amber' : 'text-scema-dim'}`}>
          {positions === null ? '…' : count}
        </span>
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto">
        {positions === null && (
          <div className="p-4 text-scema-dim text-xs text-center">Loading…</div>
        )}
        {positions !== null && positions.length === 0 && (
          <div className="p-4 text-scema-dim text-xs text-center">No open positions</div>
        )}
        {positions !== null && positions.length > 0 && (
          <table className="w-full text-xs border-collapse">
            <thead className="sticky top-0 z-10 bg-scema-panel">
              <tr className="text-scema-dim border-b border-scema-border">
                <th className="text-left   px-2 py-1.5 font-normal">TOKEN</th>
                <th className="text-right  px-2 py-1.5 font-normal">ENTRY</th>
                <th className="text-right  px-2 py-1.5 font-normal">AGE</th>
              </tr>
            </thead>
            <tbody>
              {positions.map((p, i) => (
                <tr key={i} className="border-b border-scema-border/20 hover:bg-white/5">
                  <td className="px-2 py-1.5 font-mono">
                    <a
                      href={`https://solscan.io/token/${p.mint}`}
                      target="_blank"
                      rel="noopener noreferrer"
                      title={p.mint}
                      className="text-scema-amber hover:text-scema-red-hi transition-colors"
                    >
                      {shortMint(p.mint)}
                    </a>
                  </td>
                  <td className="px-2 py-1.5 text-right tabular-nums text-scema-muted font-mono">
                    {p.entryAmount.toFixed(4)} SOL
                  </td>
                  <td className="px-2 py-1.5 text-right tabular-nums text-scema-dim font-mono">
                    {fmtAge(p.entryTime)}
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
