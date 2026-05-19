'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { Trade } from '@/lib/types'

function shortMint(mint: string) {
  return `${mint.slice(0, 4)}…${mint.slice(-4)}`
}

function fmtTime(ts: string) {
  try {
    return new Date(ts).toLocaleTimeString('en-GB', { hour12: false })
  } catch {
    return ts.slice(11, 19) || '—'
  }
}

// BUY: amount is SOL spent. SELL: amount is tokens — derive SOL received from pnl/pnl_pct.
function fmtSol(t: Trade): string {
  if (t.kind === 'BUY') return (t.amount ?? 0).toFixed(4)
  const pct = t.pnl_pct
  if (!pct) return '—'
  const sol = (t.pnl * (100 + pct)) / pct
  return isFinite(sol) && sol > 0 ? sol.toFixed(4) : '—'
}

export function TradesHistory() {
  const [trades, setTrades] = useState<Trade[] | null>(null)

  useEffect(() => {
    let alive = true
    async function poll() {
      const r = await api.trades(25)
      if (!alive) return
      setTrades(r?.trades ?? [])
    }
    poll()
    const iv = setInterval(poll, 5000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  return (
    <div className="panel flex flex-col h-full">
      <div className="panel-header justify-between">
        <span>Recent Trades</span>
        <span className="text-scema-dim">{trades === null ? '…' : trades.length}</span>
      </div>

      {/* min-h-0 is required: without it flex children default to min-height:auto
          and overflow-y-auto never constrains, causing the table to burst out */}
      <div className="flex-1 min-h-0 overflow-y-auto">
        {trades === null && (
          <div className="p-4 text-scema-dim text-xs text-center">Loading…</div>
        )}
        {trades !== null && trades.length === 0 && (
          <div className="p-4 text-scema-dim text-xs text-center">No trades yet</div>
        )}
        {trades !== null && trades.length > 0 && (
          <table className="w-full text-xs border-collapse">
            <thead className="sticky top-0 z-10 bg-scema-panel">
              <tr className="text-scema-dim border-b border-scema-border">
                <th className="text-left   px-2 py-1.5 font-normal">TIME</th>
                <th className="text-left   px-2 py-1.5 font-normal">TOKEN</th>
                <th className="text-center px-2 py-1.5 font-normal">TYPE</th>
                <th className="text-right  px-2 py-1.5 font-normal">SOL</th>
                <th className="text-right  px-2 py-1.5 font-normal">PnL%</th>
                <th className="text-center px-2 py-1.5 font-normal">TX</th>
              </tr>
            </thead>
            <tbody>
              {trades.map((t, i) => {
                const pnlPct    = t.pnl_pct ?? 0
                const isSell    = t.kind === 'SELL'
                const confirmed = t.status === '✓'
                return (
                  <tr key={i} className="border-b border-scema-border/20 hover:bg-white/5">

                    <td className="px-2 py-1 font-mono text-scema-dim tabular-nums whitespace-nowrap">
                      {fmtTime(t.timestamp)}
                    </td>

                    <td className="px-2 py-1 font-mono">
                      <a
                        href={`https://solscan.io/token/${t.mint}`}
                        target="_blank"
                        rel="noopener noreferrer"
                        title={t.mint}
                        className="text-scema-muted hover:text-scema-red-hi transition-colors"
                      >
                        {t.symbol?.trim() || shortMint(t.mint)}
                      </a>
                    </td>

                    <td className="px-2 py-1 text-center">
                      <span className={`font-bold ${isSell ? 'text-scema-red-hi' : 'text-scema-green'}`}>
                        {t.kind}
                      </span>
                    </td>

                    <td className="px-2 py-1 text-right tabular-nums text-scema-muted font-mono">
                      {fmtSol(t)}
                    </td>

                    <td className={`px-2 py-1 text-right tabular-nums font-mono ${
                      pnlPct > 0 ? 'text-scema-green' : pnlPct < 0 ? 'text-scema-red-hi' : 'text-scema-dim'
                    }`}>
                      {isSell ? `${pnlPct >= 0 ? '+' : ''}${pnlPct.toFixed(2)}%` : '—'}
                    </td>

                    <td className="px-2 py-1 text-center font-mono">
                      {t.signature && t.signature !== 'pool_drained' ? (
                        <a
                          href={`https://solscan.io/tx/${t.signature}`}
                          target="_blank"
                          rel="noopener noreferrer"
                          title={t.signature}
                          className={confirmed ? 'text-scema-green hover:underline' : 'text-scema-dim hover:underline'}
                        >
                          {t.status}
                        </a>
                      ) : (
                        <span className="text-scema-dim">{t.status}</span>
                      )}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  )
}
