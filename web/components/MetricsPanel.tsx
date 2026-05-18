'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { Metrics } from '@/lib/types'

function MetricCard({
  label,
  value,
  sub,
  kind = 'neutral',
}: {
  label: string
  value: string
  sub?: string
  kind?: 'positive' | 'negative' | 'neutral' | 'red'
}) {
  const colorClass = {
    positive: 'text-scema-green',
    negative: 'text-scema-red-hi',
    neutral:  'text-scema-text',
    red:      'glow-red',
  }[kind]

  return (
    <div className="panel flex flex-col gap-1 p-3 min-w-0">
      <span className="text-scema-muted text-xs tracking-widest uppercase truncate">{label}</span>
      <span className={`text-xl font-bold font-mono tabular-nums ${colorClass}`}>{value}</span>
      {sub && <span className="text-scema-dim text-xs">{sub}</span>}
    </div>
  )
}

function fmtUptime(secs: number) {
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

export function MetricsPanel() {
  const [data, setData] = useState<Metrics | null>(null)
  const [err, setErr] = useState(false)

  useEffect(() => {
    let alive = true
    async function poll() {
      const m = await api.metrics()
      if (!alive) return
      if (m) { setData(m); setErr(false) }
      else setErr(true)
    }
    poll()
    const iv = setInterval(poll, 3000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  if (err || !data) {
    return (
      <div className="panel p-4 col-span-full text-scema-muted text-xs text-center">
        <span className="animate-cursor-blink">AWAITING SNIPER CONNECTION</span>
        <p className="mt-1 text-scema-dim">Start the API server: <code className="text-scema-red-dim">cargo run --release --bin api</code></p>
      </div>
    )
  }

  const winRate = data.session_wins + data.session_losses > 0
    ? ((data.session_wins / (data.session_wins + data.session_losses)) * 100).toFixed(0)
    : '—'

  const pnlKind = data.pnl_sol > 0 ? 'positive' : data.pnl_sol < 0 ? 'negative' : 'neutral'

  return (
    <>
      <MetricCard
        label="PnL (Session)"
        value={`${data.pnl_sol >= 0 ? '+' : ''}${data.pnl_sol.toFixed(4)} SOL`}
        sub={`Daily: ${data.daily_pnl_sol >= 0 ? '+' : ''}${data.daily_pnl_sol.toFixed(4)} SOL`}
        kind={pnlKind}
      />
      <MetricCard
        label="Trades"
        value={`${data.trades_confirmed}/${data.trades_attempted}`}
        sub={`${winRate}% win rate`}
        kind="neutral"
      />
      <MetricCard
        label="W / L"
        value={`${data.session_wins} / ${data.session_losses}`}
        sub={`${data.open_positions} open`}
        kind={data.session_wins > data.session_losses ? 'positive' : data.session_losses > 0 ? 'negative' : 'neutral'}
      />
      <MetricCard
        label="Wallet"
        value={`${data.wallet_balance_sol.toFixed(4)} SOL`}
        sub="Balance"
        kind={data.wallet_balance_sol < 0.05 ? 'negative' : 'neutral'}
      />
      <MetricCard
        label="Uptime"
        value={fmtUptime(data.uptime_secs)}
        kind="red"
      />
    </>
  )
}
