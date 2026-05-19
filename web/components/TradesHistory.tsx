'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { Trade } from '@/lib/types'

function shortMint(mint: string) {
  return `${mint.slice(0, 4)}…${mint.slice(-4)}`
}

function tradeTime(ts: string) {
  // ISO-8601 → HH:MM:SS local time
  try {
    const d = new Date(ts)
    return d.toLocaleTimeString('en-GB', { hour12: false })
  } catch {
    return ts.slice(11, 19)
  }
}

// SELL amount is token count; derive SOL received from pnl/pnl_pct.
// BUY amount is directly SOL spent.
function solForTrade(t: Trade): number {
  if (t.kind === 'BUY') return t.amount ?? 0
  const pnlPct = t.pnl_pct ?? 0
  if (pnlPct === 0) return t.amount ?? 0
  return (t.pnl ?? 0) * (100 + pnlPct) / pnlPct
}

export function TradesHistory() {
  const [trades, setTrades] = useState<Trade[]>([])

  useEffect(() => {
    let alive = true
    async function poll() {
      const r = await api.trades(25)
      if (alive && r?.trades) setTrades(r.trades)
    }
    poll()
    const iv = setInterval(poll, 5000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  return (
    <div className="panel flex flex-col h-full">
      <div className="panel-header justify-between">
        <span>Recent Trades</span>
        <span className="text-scema-dim">{trades.length}</span>
      </div>
      <div className="flex-1 overflow-y-auto">
        {trades.length === 0 ? (
          <div className="p-4 text-scema-dim text-xs text-center">No trades yet</div>
        ) : (
          <table className="w-full text-xs">
            <thead>
              <tr className="text-scema-dim border-b border-scema-border sticky top-0 bg-scema-black">
                <th className="text-left px-2 py-1.5 font-normal">TIME</th>
                <th className="text-left px-2 py-1.5 font-normal">TOKEN</th>
                <th className="text-center px-2 py-1.5 font-normal">TYPE</th>
                <th className="text-right px-2 py-1.5 font-normal">SOL</th>
                <th className="text-right px-2 py-1.5 font-normal">PnL%</th>
                <th className="text-center px-2 py-1.5 font-normal">TX</th>
              </tr>
            </thead>
            <tbody>
              {trades.map((t, i) => {
                const pnlPct  = t.pnl_pct ?? 0
                const sol     = solForTrade(t)
                const label   = t.symbol && t.symbol.trim() ? t.symbol.toUpperCase() : shortMint(t.mint)
                const confirmed = t.status === '✓'
                return (
                  <tr key={i} className="border-b border-scema-border/30 hover:bg-scema-red-bg/10">

                    {/* Time */}
                    <td className="px-2 py-1 tabular-nums text-scema-dim font-mono whitespace-nowrap">
                      {tradeTime(t.timestamp)}
                    </td>

                    {/* Token — symbol or shortened mint */}
                    <td className="px-2 py-1 font-mono">
                      <a
                        href={`https://solscan.io/token/${t.mint}`}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-scema-muted hover:text-scema-red-hi transition-colors"
                        title={t.mint}
                      >
                        {label}
                      </a>
                    </td>

                    {/* BUY / SELL */}
                    <td className="px-2 py-1 text-center">
                      <span className={`font-bold ${
                        t.kind === 'BUY' ? 'text-scema-green' : 'text-scema-red-hi'
                      }`}>
                        {t.kind}
                      </span>
                    </td>

                    {/* SOL amount */}
                    <td className="px-2 py-1 text-right tabular-nums text-scema-muted font-mono">
                      {sol.toFixed(4)}
                    </td>

                    {/* PnL% — only meaningful on SELL */}
                    <td className={`px-2 py-1 text-right tabular-nums font-mono ${
                      pnlPct > 0 ? 'text-scema-green' : pnlPct < 0 ? 'text-scema-red-hi' : 'text-scema-dim'
                    }`}>
                      {t.kind === 'SELL'
                        ? `${pnlPct >= 0 ? '+' : ''}${pnlPct.toFixed(2)}%`
                        : '—'}
                    </td>

                    {/* TX link */}
                    <td className="px-2 py-1 text-center">
                      {t.signature && t.signature !== 'pool_drained' ? (
                        <a
                          href={`https://solscan.io/tx/${t.signature}`}
                          target="_blank"
                          rel="noopener noreferrer"
                          className={`${confirmed ? 'text-scema-green' : 'text-scema-dim'} hover:underline`}
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
