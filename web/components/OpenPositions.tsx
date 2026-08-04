'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { LivePosition } from '@/lib/types'

const LAMPORTS = 1_000_000_000
const STALE_SECS = 10 // no price-check update in this long ⇒ flag as stale

function shortMint(mint: string) {
  return `${mint.slice(0, 5)}…${mint.slice(-4)}`
}

function fmtAge(unixSecs: number) {
  const s = Math.max(0, Math.floor(Date.now() / 1000 - unixSecs))
  if (s < 60)   return `${s}s`
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`
}

function fmtSol(lamports: number) {
  return (lamports / LAMPORTS).toFixed(4)
}

// Live open positions from scematica-positions.json — the sell-monitor flushes real
// unrealized PnL, the DQ*-escalated TP target, and the trailing SL every second, so
// this reads the bot's actual in-flight risk rather than being reconstructed after
// the fact from trade history.
export function OpenPositions() {
  const [positions, setPositions] = useState<LivePosition[] | null>(null)
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    let alive = true
    async function poll() {
      const r = await api.positions()
      if (!alive) return
      setPositions(r ?? [])
    }
    poll()
    const iv = setInterval(poll, 3_000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  // Re-render every second so AGE / staleness stay live between polls.
  useEffect(() => {
    const iv = setInterval(() => setNow(Date.now()), 1_000)
    return () => clearInterval(iv)
  }, [])

  const count = positions?.length ?? 0
  const unrealizedLamports = (positions ?? []).reduce(
    (sum, p) => sum + (p.current_value_lamports - p.entry_lamports), 0,
  )
  const unrealizedPositive = unrealizedLamports >= 0

  return (
    <div className="panel flex flex-col h-full">
      <div className="panel-header justify-between">
        <span>Open Positions</span>
        <div className="flex items-center gap-3">
          {count > 0 && (
            <span className={`font-mono tabular-nums normal-case tracking-normal ${unrealizedPositive ? 'text-scema-green' : 'text-scema-red-hi'}`}>
              {unrealizedPositive ? '+' : ''}{fmtSol(unrealizedLamports)} SOL unrealized
            </span>
          )}
          <span className={count > 0 ? 'text-scema-amber' : 'text-scema-dim'}>
            {positions === null ? '…' : count}
          </span>
        </div>
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
                <th className="text-right  px-2 py-1.5 font-normal">PNL</th>
                <th className="text-right  px-2 py-1.5 font-normal">PEAK</th>
                <th className="text-right  px-2 py-1.5 font-normal">TP</th>
                <th className="text-right  px-2 py-1.5 font-normal">SL</th>
                <th className="text-right  px-2 py-1.5 font-normal">AGE</th>
              </tr>
            </thead>
            <tbody>
              {positions.map((p) => {
                const pnlPct = p.entry_lamports > 0
                  ? ((p.current_value_lamports - p.entry_lamports) / p.entry_lamports) * 100
                  : 0
                const peakPct = p.entry_lamports > 0
                  ? ((p.peak_value_lamports - p.entry_lamports) / p.entry_lamports) * 100
                  : 0
                const pnlSol = (p.current_value_lamports - p.entry_lamports) / LAMPORTS
                const stale = now / 1000 - p.last_check_unix_secs > STALE_SECS
                const declining = p.decline_streak >= 2
                const pnlColor = pnlPct >= 0 ? 'text-scema-green' : 'text-scema-red-hi'

                return (
                  <tr key={p.mint} className="border-b border-scema-border/20 hover:bg-white/5">
                    <td className="px-2 py-1.5 font-mono">
                      <div className="flex items-center gap-1.5">
                        <span
                          className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                            stale ? 'bg-scema-dim' : declining ? 'bg-scema-amber animate-pulse' : 'bg-scema-green animate-pulse'
                          }`}
                          title={stale ? 'stale — no recent price check' : declining ? `declining (${p.decline_streak} ticks)` : 'live'}
                        />
                        <a
                          href={`https://solscan.io/token/${p.mint}`}
                          target="_blank"
                          rel="noopener noreferrer"
                          title={p.mint}
                          className="text-scema-amber hover:text-scema-red-hi transition-colors"
                        >
                          {shortMint(p.mint)}
                        </a>
                        {p.escalations > 0 && (
                          <span
                            className="text-scema-red-hi text-[0.6rem]"
                            title={`${p.escalations} momentum escalation${p.escalations > 1 ? 's' : ''}`}
                          >
                            ×{p.escalations}
                          </span>
                        )}
                      </div>
                    </td>
                    <td className={`px-2 py-1.5 text-right tabular-nums font-mono font-bold ${pnlColor}`}>
                      {pnlPct >= 0 ? '+' : ''}{pnlPct.toFixed(1)}%
                      <span className="block text-[0.6rem] font-normal opacity-70">
                        {pnlSol >= 0 ? '+' : ''}{pnlSol.toFixed(4)}
                      </span>
                    </td>
                    <td className="px-2 py-1.5 text-right tabular-nums text-scema-muted font-mono">
                      +{Math.max(0, peakPct).toFixed(0)}%
                    </td>
                    <td className="px-2 py-1.5 text-right tabular-nums text-scema-green font-mono">
                      {p.dynamic_tp_pct.toFixed(0)}%
                    </td>
                    <td className="px-2 py-1.5 text-right tabular-nums text-scema-red-hi font-mono">
                      {p.current_sl_pct >= 0 ? '+' : ''}{p.current_sl_pct.toFixed(0)}%
                    </td>
                    <td className="px-2 py-1.5 text-right tabular-nums text-scema-dim font-mono whitespace-nowrap">
                      {fmtAge(p.entry_unix_secs)}
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
