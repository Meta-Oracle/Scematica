'use client'

import { useMetrics, usePositions, useTrades } from '@/lib/queries'

const LAMPORTS = 1_000_000_000

function MetricCard({ label, value, sub, kind = 'neutral' }: {
  label: string; value: string; sub?: string
  kind?: 'positive' | 'negative' | 'neutral' | 'red'
}) {
  const color = { positive: 'text-scema-green', negative: 'text-scema-red-hi', neutral: 'text-scema-text', red: 'glow-red' }[kind]
  return (
    <div className="panel flex flex-col gap-1 p-3 min-w-0">
      <span className="text-scema-muted text-xs tracking-widest uppercase truncate">{label}</span>
      <span className={`text-xl font-bold font-mono tabular-nums truncate ${color}`}>{value}</span>
      {sub && <span className="text-scema-dim text-xs truncate">{sub}</span>}
    </div>
  )
}

function fmtUptime(secs: number) {
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = Math.floor(secs % 60)
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

export function MetricsPanel() {
  const { data, ok, loading } = useMetrics()
  const { data: tradesData } = useTrades()
  const { data: positionsData } = usePositions()

  const trades = tradesData?.trades ?? []
  const positions = positionsData ?? []
  // Don't show the "no sniper" prompt during the very first poll — only once one
  // has actually come back empty.
  const err = !ok && !loading

  if (err && !data) {
    return (
      <div className="panel p-4 col-span-full text-scema-muted text-xs text-center">
        <span className="animate-cursor-blink">AWAITING SNIPER CONNECTION</span>
        <p className="mt-1 text-scema-dim">
          Start the API server: <code className="text-scema-red-dim">cargo run --release --bin api</code>
        </p>
      </div>
    )
  }

  // Derive wins/losses from trade history
  const sells = trades.filter(t => t.kind === 'SELL' && t.status === '✓')
  const wins   = sells.filter(t => (t.pnl_pct ?? 0) > 0).length
  const losses = sells.filter(t => (t.pnl_pct ?? 0) < 0).length
  const winRate = wins + losses > 0 ? ((wins / (wins + losses)) * 100).toFixed(0) : '—'

  const pnlSol    = data ? data.total_pnl_lamports / LAMPORTS : 0
  const attempted = data?.trades_attempted ?? 0
  const confirmed = data?.trades_confirmed ?? 0
  const failed    = data?.trades_failed ?? 0
  const pools     = data?.pools_tracked ?? 0
  const arb       = data?.arb_executed ?? 0
  const uptime    = data?.uptime_secs ?? 0

  const pnlKind = pnlSol > 0 ? 'positive' : pnlSol < 0 ? 'negative' : 'neutral'
  const wlKind  = wins > losses ? 'positive' : losses > wins ? 'negative' : 'neutral'

  const unrealizedSol = positions.reduce(
    (sum, p) => sum + (p.current_value_lamports - p.entry_lamports), 0,
  ) / LAMPORTS
  const unrealizedKind = unrealizedSol > 0 ? 'positive' : unrealizedSol < 0 ? 'negative' : 'neutral'

  return (
    <>
      <MetricCard
        label="Session PnL"
        value={`${pnlSol >= 0 ? '+' : ''}${pnlSol.toFixed(4)} SOL`}
        sub={`${confirmed} confirmed trades`}
        kind={pnlKind}
      />
      <MetricCard
        label="Unrealized PnL"
        value={`${unrealizedSol >= 0 ? '+' : ''}${unrealizedSol.toFixed(4)} SOL`}
        sub={positions.length > 0 ? `${positions.length} open position${positions.length === 1 ? '' : 's'}` : 'no open positions'}
        kind={positions.length > 0 ? unrealizedKind : 'neutral'}
      />
      <MetricCard
        label="Trades"
        value={`${confirmed} / ${attempted}`}
        sub={failed > 0 ? `${failed} failed` : 'confirmed / attempted'}
        kind="neutral"
      />
      <MetricCard
        label="W / L"
        value={`${wins} / ${losses}`}
        sub={`${winRate}% win rate`}
        kind={wlKind}
      />
      <MetricCard
        label="Pools · Arb"
        value={`${pools}`}
        sub={`${arb} arb executed`}
        kind="neutral"
      />
      <MetricCard
        label="Uptime"
        value={fmtUptime(uptime)}
        kind="red"
      />
    </>
  )
}
