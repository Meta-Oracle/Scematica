'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { Pool } from '@/lib/types'

function scoreColor(score: number) {
  if (score >= 98) return 'text-scema-red-hi'
  if (score >= 88) return 'text-scema-amber'
  if (score >= 70) return 'text-scema-muted'
  return 'text-scema-dim'
}

function shortMint(mint: string) {
  return `${mint.slice(0, 6)}…${mint.slice(-4)}`
}

function fmtAge(ts: number) {
  const diff = Math.floor(Date.now() / 1000) - ts
  if (diff < 60)  return `${diff}s`
  if (diff < 3600) return `${Math.floor(diff/60)}m`
  return `${Math.floor(diff/3600)}h`
}

export function PoolRadar() {
  const [pools, setPools] = useState<Pool[]>([])
  const [total, setTotal] = useState(0)

  useEffect(() => {
    let alive = true
    async function poll() {
      const r = await api.pools(30)
      if (!alive || !r) return
      setPools(r.pools)
      setTotal(r.total)
    }
    poll()
    const iv = setInterval(poll, 3000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  return (
    <div className="panel flex flex-col h-full">
      <div className="panel-header justify-between">
        <span>Pool Radar</span>
        <span className="text-scema-dim">{total} total</span>
      </div>
      <div className="flex-1 overflow-y-auto">
        {pools.length === 0 ? (
          <div className="p-4 text-scema-dim text-xs text-center">Scanning…</div>
        ) : (
          <table className="w-full text-xs">
            <thead>
              <tr className="text-scema-dim border-b border-scema-border">
                <th className="text-left px-3 py-1.5 font-normal tracking-wider">MINT</th>
                <th className="text-right px-2 py-1.5 font-normal">SCORE</th>
                <th className="text-right px-2 py-1.5 font-normal">SOL</th>
                <th className="text-right px-2 py-1.5 font-normal">AGE</th>
                <th className="text-center px-2 py-1.5 font-normal">PASS</th>
              </tr>
            </thead>
            <tbody>
              {pools.map((p, i) => (
                <tr
                  key={`${p.mint}-${i}`}
                  className={`border-b border-scema-border/40 hover:bg-scema-red-bg/20 transition-colors ${
                    p.passed_filters ? 'bg-scema-red-bg/10' : ''
                  }`}
                >
                  <td className="px-3 py-1 font-mono">
                    <a
                      href={`https://solscan.io/token/${p.mint}`}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-scema-muted hover:text-scema-red-hi transition-colors"
                    >
                      {shortMint(p.mint)}
                    </a>
                  </td>
                  <td className={`px-2 py-1 text-right font-bold ${scoreColor(p.score)}`}>
                    {p.score}
                  </td>
                  <td className="px-2 py-1 text-right text-scema-text tabular-nums">
                    {p.size_sol.toFixed(1)}
                  </td>
                  <td className="px-2 py-1 text-right text-scema-muted tabular-nums">
                    {fmtAge(p.timestamp)}
                  </td>
                  <td className="px-2 py-1 text-center">
                    {p.passed_filters
                      ? <span className="text-scema-green text-xs">✓</span>
                      : <span className="text-scema-dim text-xs">✗</span>}
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
