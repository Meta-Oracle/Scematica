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

  const pnl      = data.pnl_sol ?? 0
  const dailyPnl = data.daily_pnl_sol ?? 0
  const walletBal = data.wallet_balance_sol ?? 0
  const wins     = data.session_wins ?? 0
  const losses   = data.session_losses ?? 0
  const attempted = data.trades_attempted ?? 0
  const confirmed = data.trades_confirmed ?? 0
  const open      = data.open_positions ?? 0

  const winRate = wins + losses > 0
    ? ((wins / (wins + losses)) * 100).toFixed(0)
    : '—'

  const pnlKind = pnl > 0 ? 'positive' : pnl < 0 ? 'negative' : 'neutral'

  return (
    <>
      <MetricCard
        label="PnL (Session)"
        value={`${pnl >= 0 ? '+' : ''}${pnl.toFixed(4)} SOL`}
        sub={`Daily: ${dailyPnl >= 0 ? '+' : ''}${dailyPnl.toFixed(4)} SOL`}
        kind={pnlKind}
      />
      <MetricCard
        label="Trades"
        value={`${confirmed}/${attempted}`}
        sub={`${winRate}% win rate`}
        kind="neutral"
      />
      <MetricCard
        label="W / L"
        value={`${wins} / ${losses}`}
        sub={`${open} open`}
        kind={wins > losses ? 'positive' : losses > 0 ? 'negative' : 'neutral'}
      />
      <MetricCard
        label="Wallet"
        value={`${walletBal.toFixed(4)} SOL`}
        sub="Balance"
        kind={walletBal < 0.05 ? 'negative' : 'neutral'}
      />
      <MetricCard
        label="Uptime"
        value={fmtUptime(data.uptime_secs)}
        kind="red"
      />
    </>
  )
}
