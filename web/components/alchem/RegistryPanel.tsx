'use client'

import { fmtPrice, shortAddress } from '@/lib/alchem/format'
import { useAlchemVerify } from '@/lib/alchem/queries'

// Does each registered address still report the pair it is filed under?
//
// This is the check that caught the address widely passed around as Base "BTC/USD"
// reporting `WBTC / USD` on-chain — a wrapper that can depeg from spot BTC. It is also
// the only thing keeping the TypeScript registry honest against the Python one it was
// ported from: both are just tables, and this asks the chain instead of either.

export function RegistryPanel({ network }: { network: string }) {
  const { data, loading } = useAlchemVerify(network)

  return (
    <div className="alchem-panel h-full flex flex-col">
      <div className="alchem-panel-header justify-between">
        <span>Registry — verified on-chain</span>
        {data && (
          <span
            className={`text-[0.6rem] px-1.5 py-0.5 border normal-case tracking-normal ${
              data.ok
                ? 'text-alchem-green border-alchem-green/40 bg-alchem-green/10'
                : 'text-alchem-amber border-alchem-amber/40 bg-alchem-amber/10'
            }`}
          >
            {data.ok ? 'ALL MATCH' : 'MISMATCH'}
          </span>
        )}
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto p-3">
        {loading && <p className="text-alchem-amber text-xs">Calling description()…</p>}

        {!loading && !data && (
          <p className="text-alchem-red text-xs">Could not reach /api/alchem/verify.</p>
        )}

        {data?.error && <p className="text-alchem-red text-xs mb-2">{data.error}</p>}

        {data && data.entries.length > 0 && (
          <div className="overflow-x-auto">
            <table className="w-full text-[0.65rem] border-collapse">
              <thead>
                <tr className="text-alchem-dim uppercase tracking-widest text-[0.55rem]">
                  <th className="text-left font-normal pb-1.5 pr-2">Filed as</th>
                  <th className="text-left font-normal pb-1.5 pr-2">Reports</th>
                  <th className="text-right font-normal pb-1.5 pr-2">Dec</th>
                  <th className="text-right font-normal pb-1.5 pr-2">Price</th>
                  <th className="text-left font-normal pb-1.5">Address</th>
                </tr>
              </thead>
              <tbody>
                {data.entries.map(entry => (
                  <tr key={entry.address} className="border-t border-alchem-border/40">
                    <td className="py-1.5 pr-2 text-alchem-blue whitespace-nowrap">{entry.pair}</td>
                    <td className="py-1.5 pr-2 whitespace-nowrap">
                      {entry.error ? (
                        <span className="text-alchem-red">{entry.error}</span>
                      ) : (
                        <span className={entry.ok ? 'text-alchem-text' : 'text-alchem-amber'}>
                          {entry.description}
                          {!entry.ok && ' ⚠'}
                        </span>
                      )}
                    </td>
                    <td className="py-1.5 pr-2 text-right tabular-nums">
                      <span
                        className={
                          entry.decimals !== undefined && entry.decimals !== entry.declaredDecimals
                            ? 'text-alchem-amber'
                            : 'text-alchem-muted'
                        }
                      >
                        {entry.decimals ?? '—'}
                      </span>
                    </td>
                    <td className="py-1.5 pr-2 text-right text-alchem-text tabular-nums">
                      {entry.price !== undefined ? fmtPrice(entry.price) : '—'}
                    </td>
                    <td className="py-1.5 text-alchem-dim" title={entry.address}>
                      {shortAddress(entry.address)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {data && !data.ok && !data.error && (
          <p className="text-alchem-amber text-[0.6rem] mt-3 pt-2 border-t border-alchem-border/50">
            A row flagged here means the on-chain <code>description()</code> does not match
            the label this registry files it under. Trust the contract, not the table.
          </p>
        )}
      </div>
    </div>
  )
}
