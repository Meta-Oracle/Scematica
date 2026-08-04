'use client'

import { useMemo } from 'react'
import { useTrades } from '@/lib/queries'
import type { Trade } from '@/lib/types'

interface Point { cumPnl: number; ts: string }

function fmtSol(n: number) {
  return `${n >= 0 ? '+' : ''}${n.toFixed(6)}`
}

export function PnlChart() {
  const { data } = useTrades()

  const points = useMemo<Point[]>(() => {
    // Confirmed sells oldest-first
    const sells: Trade[] = (data?.trades ?? [])
      .filter((t: Trade) => t.kind === 'SELL' && t.status === '✓')
      .reverse()

    if (sells.length === 0) return []

    let cum = 0
    const pts: Point[] = [{ cumPnl: 0, ts: sells[0].timestamp }]
    for (const t of sells) {
      cum += t.pnl
      pts.push({ cumPnl: cum, ts: t.timestamp })
    }
    return pts
  }, [data])

  const current = points.length > 0 ? points[points.length - 1].cumPnl : null
  const positive = (current ?? 0) >= 0

  if (points.length < 2) {
    return (
      <div className="panel flex flex-col h-full">
        <div className="panel-header">
          <span>PnL Curve</span>
        </div>
        <div className="flex-1 min-h-0 flex items-center justify-center">
          <span className="text-scema-dim text-xs">No sell history yet</span>
        </div>
      </div>
    )
  }

  const W = 500; const H = 80; const PAD = 6
  const pnls   = points.map(p => p.cumPnl)
  const minPnl = Math.min(...pnls)
  const maxPnl = Math.max(...pnls)
  const range  = maxPnl - minPnl || 0.0001
  const n      = points.length

  const toX = (i: number) => PAD + (i / (n - 1)) * (W - 2 * PAD)
  const toY = (v: number) => PAD + ((maxPnl - v) / range) * (H - 2 * PAD)

  const linePts = points.map((p, i) => `${toX(i)},${toY(p.cumPnl)}`).join(' ')
  const fillPts = [
    `${toX(0)},${toY(0)}`,
    ...points.map((p, i) => `${toX(i)},${toY(p.cumPnl)}`),
    `${toX(n - 1)},${toY(0)}`,
  ].join(' ')

  const zeroY = toY(0)
  const lineColor = positive ? '#00cc44' : '#ff2020'
  const fillColor = positive ? 'rgba(0,204,68,0.07)' : 'rgba(255,32,32,0.07)'

  return (
    <div className="panel flex flex-col h-full">
      <div className="panel-header justify-between">
        <span>PnL Curve</span>
        <div className="flex items-center gap-3 text-xs font-mono tabular-nums">
          <span className="text-scema-dim">{points.length - 1} sells</span>
          <span className={positive ? 'text-scema-green' : 'text-scema-red-hi'}>
            {fmtSol(current!)} SOL
          </span>
        </div>
      </div>
      <div className="flex-1 min-h-0 px-2 pb-2 pt-1">
        <svg
          viewBox={`0 0 ${W} ${H}`}
          className="w-full h-full"
          preserveAspectRatio="none"
        >
          {/* Zero baseline */}
          {zeroY >= PAD && zeroY <= H - PAD && (
            <line
              x1={PAD} y1={zeroY} x2={W - PAD} y2={zeroY}
              stroke="#330000" strokeWidth="0.8" strokeDasharray="3 3"
            />
          )}
          {/* Fill */}
          <polygon points={fillPts} fill={fillColor} />
          {/* Line */}
          <polyline
            points={linePts}
            fill="none"
            stroke={lineColor}
            strokeWidth="1.5"
            strokeLinejoin="round"
            strokeLinecap="round"
          />
          {/* Current value dot */}
          <circle
            cx={toX(n - 1)} cy={toY(current!)}
            r="3" fill={lineColor}
          />
        </svg>
      </div>
      {/* Min / Max labels */}
      <div className="flex justify-between px-2 pb-1.5 text-xs text-scema-dim font-mono tabular-nums">
        <span>{fmtSol(minPnl)}</span>
        <span>{fmtSol(maxPnl)}</span>
      </div>
    </div>
  )
}
