'use client'

import { fmtAge, fmtPrice, heartbeatProgress, shortAddress, STATUS_BADGE, STATUS_TEXT } from '@/lib/alchem/format'
import { useAlchemFeeds } from '@/lib/alchem/queries'

// The live half of alchem-link: every registered Chainlink feed on one network, with the
// staleness verdict that the raw `latestRoundData()` call does not give you.
//
// A feed that answers is not a feed that published. The aggregator returns happily
// whether the last update was ten seconds or ten hours ago, and acting on a stale answer
// is how oracle integrations lose money — so age against the declared heartbeat is the
// primary column here, not a footnote.

function HeartbeatBar({ ageSecs, heartbeatSecs, stale }: {
  ageSecs: number
  heartbeatSecs: number
  stale: boolean
}) {
  const progress = heartbeatProgress(ageSecs, heartbeatSecs)
  return (
    <div
      className="h-1 w-full bg-alchem-hi overflow-hidden"
      title={`${ageSecs}s of a ${heartbeatSecs}s heartbeat`}
    >
      <div
        className={`h-full transition-[width] duration-500 ${stale ? 'bg-alchem-amber' : 'bg-alchem-blue'}`}
        style={{ width: `${Math.max(2, progress * 100)}%` }}
      />
    </div>
  )
}

export function FeedBoard({ network }: { network: string }) {
  const { data, ok, loading } = useAlchemFeeds(network)

  return (
    <div className="alchem-panel h-full flex flex-col">
      <div className="alchem-panel-header justify-between">
        <span>Live Feeds — {network}</span>
        {data && (
          <span className="text-alchem-dim normal-case tracking-normal text-[0.6rem] truncate max-w-[45%]">
            {data.endpoint} ({data.endpointSource})
          </span>
        )}
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto p-3">
        {loading && (
          <p className="text-alchem-amber text-xs">Reading aggregators…</p>
        )}

        {/* A failed poll keeps the last good payload in the store, so an outage dims the
            board rather than blanking it — but it must never look current. */}
        {!loading && !ok && !data && (
          <p className="text-alchem-red text-xs">
            Could not reach /api/alchem/feeds. The dev server may be down.
          </p>
        )}

        {data?.error && (
          <div className="mb-3 border border-alchem-red/40 bg-alchem-red/5 px-3 py-2">
            <p className="text-alchem-red text-xs">{data.error}</p>
            <p className="text-alchem-dim text-[0.65rem] mt-1">
              Endpoint unreachable — nothing below is current.
            </p>
          </div>
        )}

        {data && data.readings.length > 0 && (
          <div className={`flex flex-col gap-2 ${ok ? '' : 'opacity-60'}`}>
            {data.readings.map(r => (
              <div
                key={r.address}
                className="border border-alchem-border/60 bg-alchem-black/40 hover:border-alchem-border-hi transition-colors"
              >
                <div className="flex items-baseline justify-between gap-3 px-3 pt-2">
                  <span className="text-alchem-blue font-bold text-sm shrink-0">{r.pair}</span>
                  <span className="text-alchem-text text-sm tabular-nums truncate">
                    {fmtPrice(r.price)}
                  </span>
                  <span
                    className={`text-[0.6rem] px-1.5 py-0.5 border shrink-0 ${STATUS_BADGE[r.status]}`}
                  >
                    {r.status}
                  </span>
                </div>

                <div className="flex items-center justify-between gap-3 px-3 pb-2 pt-1">
                  <span className={`text-[0.65rem] ${STATUS_TEXT[r.status]}`}>
                    {fmtAge(r.ageSecs)} ago
                  </span>
                  <span className="text-alchem-dim text-[0.6rem]">
                    heartbeat {r.heartbeatSecs}s
                  </span>
                  <a
                    href={`${data.explorer}/address/${r.address}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-alchem-dim hover:text-alchem-blue text-[0.6rem] transition-colors"
                    title={r.address}
                  >
                    {shortAddress(r.address)} ↗
                  </a>
                </div>

                <HeartbeatBar
                  ageSecs={r.ageSecs}
                  heartbeatSecs={r.heartbeatSecs}
                  stale={r.stale}
                />

                {r.note && (
                  <p className="text-alchem-amber text-[0.6rem] px-3 py-1.5 border-t border-alchem-border/40">
                    ⚠ {r.note}
                  </p>
                )}
              </div>
            ))}
          </div>
        )}

        {/* Failures are rendered, never dropped. A missing row would read as
            "this network has fewer feeds", which is a different and wrong claim. */}
        {data && data.failures.length > 0 && (
          <div className="mt-3 border-t border-alchem-border/50 pt-2">
            <p className="text-alchem-dim text-[0.6rem] uppercase tracking-widest mb-1.5">
              Unreadable
            </p>
            {data.failures.map(f => (
              <div key={f.address} className="text-[0.65rem] mb-1">
                <span className="text-alchem-red">{f.pair}</span>
                <span className="text-alchem-dim"> — {f.error}</span>
              </div>
            ))}
          </div>
        )}

        {data && data.readings.length === 0 && data.failures.length === 0 && !data.error && (
          <p className="text-alchem-muted text-xs">No feeds registered for this network.</p>
        )}
      </div>

      {data && (
        <div className="border-t border-alchem-border px-3 py-1.5 flex items-center justify-between text-[0.6rem]">
          <span className="text-alchem-muted">
            {data.readings.length} feeds ·{' '}
            {data.readings.filter(r => !r.stale).length} fresh ·{' '}
            {data.readings.filter(r => r.stale).length} past heartbeat
          </span>
          <span className={data.authenticated ? 'text-alchem-blue-dim' : 'text-alchem-amber'}>
            {data.authenticated ? 'KEYED' : 'KEYLESS'}
          </span>
        </div>
      )}
    </div>
  )
}
