'use client'

import { useMemo } from 'react'
import { useDecisions } from '@/lib/queries'
import { useDiscovery } from '@/lib/useDiscovery'

function shortMint(mint: string) {
  return mint ? `${mint.slice(0, 4)}...${mint.slice(-4)}` : '----'
}

function fmtTime(ts: string) {
  try {
    return new Date(ts).toLocaleTimeString('en-GB', { hour12: false })
  } catch {
    return ts?.slice(11, 19) || '--:--:--'
  }
}

function decisionClass(decision: string) {
  if (decision === 'accepted') return 'text-scema-green'
  if (decision === 'rejected') return 'text-scema-red-hi'
  return 'text-scema-dim'
}

export function DecisionLedger() {
  const discovery = useDiscovery()
  const live = useDecisions()

  const fromLive = discovery.source === 'live'
  const decisions = fromLive
    ? (live.loading && !live.data ? null : (live.data?.decisions ?? []))
    : (discovery.loading ? null : discovery.decisions)

  const stats = useMemo(() => {
    const rows = decisions ?? []
    const accepted = rows.filter(d => d.decision === 'accepted').length
    const rejected = rows.filter(d => d.decision === 'rejected').length
    const ignored = rows.filter(d => d.decision === 'ignored').length
    const decisive = accepted + rejected
    const acceptRate = decisive > 0 ? (accepted / decisive) * 100 : 0
    return { accepted, rejected, ignored, acceptRate }
  }, [decisions])

  return (
    <div className="panel flex flex-col h-full">
      <div className="panel-header justify-between">
        <span>Pool Decision Ledger</span>
        <span className="flex items-center gap-2">
          {!fromLive && (
            <span
              title="Verdicts from the ported pipeline over the public mint feed. INFLOW and DQ* are blank because the feed cannot supply them."
              className="text-scema-amber border border-scema-amber/40 px-1.5 leading-tight"
            >
              FEED
            </span>
          )}
          <span className="text-scema-muted">{decisions === null ? '...' : decisions.length}</span>
        </span>
      </div>

      <div className="grid grid-cols-4 divide-x divide-scema-border border-b border-scema-border text-xs">
        <div className="flex flex-col px-3 py-2">
          <span className="text-scema-dim">ACCEPT</span>
          <span className="text-scema-green font-bold tabular-nums">{stats.accepted}</span>
        </div>
        <div className="flex flex-col px-3 py-2">
          <span className="text-scema-dim">REJECT</span>
          <span className="text-scema-red-hi font-bold tabular-nums">{stats.rejected}</span>
        </div>
        <div className="flex flex-col px-3 py-2">
          <span className="text-scema-dim">IGNORE</span>
          <span className="text-scema-muted font-bold tabular-nums">{stats.ignored}</span>
        </div>
        <div className="flex flex-col px-3 py-2">
          <span className="text-scema-dim">RATE</span>
          <span className="text-scema-text font-bold tabular-nums">{stats.acceptRate.toFixed(1)}%</span>
        </div>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto">
        {decisions === null && (
          <div className="p-4 text-scema-dim text-xs text-center">Loading...</div>
        )}
        {decisions !== null && decisions.length === 0 && (
          <div className="p-4 text-scema-dim text-xs text-center">No pool decisions yet</div>
        )}
        {decisions !== null && decisions.length > 0 && (
          <table className="w-full text-xs border-collapse">
            <thead className="sticky top-0 z-10 bg-scema-panel">
              <tr className="text-scema-dim border-b border-scema-border">
                <th className="text-left px-2 py-1.5 font-normal">TIME</th>
                <th className="text-left px-2 py-1.5 font-normal">TOKEN</th>
                <th className="text-left px-2 py-1.5 font-normal">GATE</th>
                <th className="text-right px-2 py-1.5 font-normal">SCORE</th>
                <th className="text-right px-2 py-1.5 font-normal">INFLOW</th>
                <th className="text-left px-2 py-1.5 font-normal">DQ*</th>
                <th className="text-left px-2 py-1.5 font-normal">REASON</th>
              </tr>
            </thead>
            <tbody>
              {decisions.map((d, i) => (
                <tr key={`${d.timestamp}-${d.mint}-${i}`} className="border-b border-scema-border/20 hover:bg-white/5">
                  <td className="px-2 py-1 font-mono text-scema-dim tabular-nums whitespace-nowrap">
                    {fmtTime(d.timestamp)}
                  </td>
                  <td className="px-2 py-1 font-mono">
                    <a
                      href={`https://solscan.io/token/${d.mint}`}
                      target="_blank"
                      rel="noopener noreferrer"
                      title={d.mint}
                      className={`${decisionClass(d.decision)} hover:underline`}
                    >
                      {shortMint(d.mint)}
                    </a>
                  </td>
                  <td className="px-2 py-1 font-mono text-scema-muted whitespace-nowrap">
                    {d.stage}
                  </td>
                  <td className="px-2 py-1 text-right font-mono tabular-nums text-scema-text">
                    {(d.pool_score ?? 0).toFixed(1)}
                  </td>
                  <td className="px-2 py-1 text-right font-mono tabular-nums text-scema-muted">
                    {(d.inflow_rate_sol_per_sec ?? 0).toFixed(3)}
                  </td>
                  <td className="px-2 py-1 font-mono whitespace-nowrap">
                    {d.dq_action ? (
                      <span className={
                        d.dq_action.startsWith('Buy') ? 'text-scema-green'
                        : d.dq_action.startsWith('Sell') ? 'text-scema-red-hi'
                        : 'text-scema-amber'
                      }>
                        {d.dq_action}
                        <span className="text-scema-dim ml-1">{((d.dq_confidence ?? 0) * 100).toFixed(0)}%</span>
                      </span>
                    ) : (
                      <span className="text-scema-dim">—</span>
                    )}
                  </td>
                  <td className="px-2 py-1 text-scema-dim truncate max-w-[220px]" title={d.reason}>
                    {d.reason || d.decision}
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
