'use client'

import { useAlchemDoctor } from '@/lib/alchem/queries'

// End-to-end readiness for one network.
//
// Three failures are silent in practice: you are on the keyless fallback and getting
// rate limited, you are pointed at the wrong chain, or a feed technically responds but
// has not published in hours. Each check turns one of those into a visible line.

export function DoctorPanel({ network }: { network: string }) {
  const { data, ok, loading } = useAlchemDoctor(network)

  return (
    <div className="alchem-panel h-full flex flex-col">
      <div className="alchem-panel-header justify-between">
        <span>Doctor</span>
        {data && (
          <span
            className={`text-[0.6rem] px-1.5 py-0.5 border normal-case tracking-normal ${
              data.ok
                ? 'text-alchem-green border-alchem-green/40 bg-alchem-green/10'
                : 'text-alchem-amber border-alchem-amber/40 bg-alchem-amber/10'
            }`}
          >
            {data.ok ? 'READY' : 'ATTENTION'}
          </span>
        )}
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto p-3 text-xs">
        {loading && <p className="text-alchem-amber">Running checks…</p>}

        {!loading && !data && (
          <p className="text-alchem-red">Could not reach /api/alchem/doctor.</p>
        )}

        {data && (
          <>
            <div className="mb-3 pb-2 border-b border-alchem-border/50">
              <p className="text-alchem-muted text-[0.65rem]">
                <span className="text-alchem-dim">network </span>
                {data.networkLabel}
              </p>
              <p className="text-alchem-muted text-[0.65rem] break-all">
                <span className="text-alchem-dim">endpoint </span>
                {data.endpoint}{' '}
                <span className="text-alchem-dim">({data.endpointSource})</span>
              </p>
            </div>

            <div className={`flex flex-col gap-2 ${ok ? '' : 'opacity-60'}`}>
              {data.checks.map(check => (
                <div key={check.name} className="flex gap-2">
                  <span
                    className={`shrink-0 text-[0.65rem] ${
                      check.ok ? 'text-alchem-green' : 'text-alchem-red'
                    }`}
                  >
                    [{check.ok ? 'ok  ' : 'fail'}]
                  </span>
                  <div className="min-w-0">
                    <p className="text-alchem-blue text-[0.7rem]">{check.name}</p>
                    <p className="text-alchem-text text-[0.65rem] break-words">{check.detail}</p>
                    {check.hint && (
                      <p className="text-alchem-amber text-[0.6rem] mt-0.5">{check.hint}</p>
                    )}
                  </div>
                </div>
              ))}
            </div>

            {!data.authenticated && (
              <p className="text-alchem-dim text-[0.6rem] mt-3 pt-2 border-t border-alchem-border/50">
                Set <span className="text-alchem-blue-dim">ALCHEMY_API_KEY</span> in the
                server environment for real rate limits. The key stays server-side; it is
                never sent to this page.
              </p>
            )}
          </>
        )}
      </div>
    </div>
  )
}
