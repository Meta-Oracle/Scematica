'use client'

import { useCallback, useEffect, useState } from 'react'
import Link from 'next/link'

import {
  MAINNET,
  formatUnits,
  isAddress,
  shortAddress,
  type Network,
} from '@/lib/botchain/networks'

// The BOT Chain console. Reads a chain or shows why it could not — there is no
// simulation path behind any of this, matching the alchem-link routes.
//
// The pool-creation figure is the point of the page rather than a decoration. It is
// currently ~2 events in 8 days, and that number is the whole argument for the sniper
// staying on Solana. It is rendered plainly, including when it is zero.

interface VenueStatus {
  name: string
  router: string
  factory: string
  events: number
  blocksScanned: number
  rangesRefused: number
  perDay: number | null
}

interface Status {
  ok: boolean
  error?: string
  network: { name: string; chainId: number; symbol: string; explorer: string; chainIdWarning: string | null }
  source: { endpoint: string; kind: string; elapsedMs: number; canBroadcast: boolean }
  head: number
  gasGwei: number
  blockSeconds: number
  venues: VenueStatus[]
  flow: { windowBlocks: number; blocksScanned: number; events: number; perDay: number | null }
}

interface TokenRow {
  symbol: string
  name: string
  address: string
  decimals: number
  ok: boolean
  balance: string | null
  error?: string
}

interface AddressResult {
  ok: boolean
  error?: string
  address: string
  nativeWei: string
  symbol: string
  decimals: number
  nonce: number
  tokens: TokenRow[]
  source: { endpoint: string; kind: string }
}

export function BotchainConsole() {
  const network: Network = MAINNET
  const [status, setStatus] = useState<Status | null>(null)
  const [statusError, setStatusError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const [query, setQuery] = useState('')
  const [lookup, setLookup] = useState<AddressResult | null>(null)
  const [lookupError, setLookupError] = useState<string | null>(null)
  const [looking, setLooking] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const res = await fetch('/api/botchain/status?network=mainnet', { cache: 'no-store' })
      const data = await res.json()
      if (!res.ok || !data.ok) throw new Error(data.error || `Request failed (${res.status})`)
      setStatus(data)
      setStatusError(null)
    } catch (err) {
      setStatusError(err instanceof Error ? err.message : String(err))
      setStatus(null)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const search = useCallback(async () => {
    const address = query.trim()
    if (!isAddress(address)) {
      setLookupError('Enter a 0x-prefixed 20-byte address.')
      setLookup(null)
      return
    }
    setLooking(true)
    setLookupError(null)
    try {
      const res = await fetch(`/api/botchain/address?address=${address}&network=mainnet`, {
        cache: 'no-store',
      })
      const data = await res.json()
      if (!res.ok || !data.ok) throw new Error(data.error || `Request failed (${res.status})`)
      setLookup(data)
    } catch (err) {
      setLookupError(err instanceof Error ? err.message : String(err))
      setLookup(null)
    } finally {
      setLooking(false)
    }
  }, [query])

  return (
    <div className="botchain-root flex min-h-screen flex-col">
      <header className="border-b border-botchain-border px-4 py-3">
        <div className="mx-auto flex max-w-[1400px] items-center gap-3">
          <div className="flex flex-col leading-tight">
            <span className="glow-amber text-sm font-bold tracking-[0.3em]">BOT CHAIN</span>
            <span className="text-xs tracking-widest text-botchain-dim">
              CHAIN {network.chainId} · EVM · PARLIA POSA
            </span>
          </div>
          <div className="ml-auto flex items-center gap-3 text-xs">
            <a
              href={network.explorer}
              target="_blank"
              rel="noopener noreferrer"
              className="hidden border border-botchain-border px-2 py-0.5 tracking-widest
                         text-botchain-amber transition-all hover:border-botchain-amber
                         hover:text-botchain-amber-hi sm:block"
            >
              BOTSCAN ↗
            </a>
            <Link
              href="/"
              className="border border-botchain-border px-2 py-0.5 tracking-widest
                         text-botchain-amber transition-all hover:border-botchain-amber
                         hover:text-botchain-amber-hi"
            >
              ⬢ SCEMATICA
            </Link>
          </div>
        </div>
      </header>

      <main className="mx-auto flex w-full max-w-[1400px] flex-1 flex-col gap-4 px-3 py-4">
        {statusError && (
          <div className="border border-botchain-red/40 bg-botchain-red/5 p-3 text-xs text-botchain-red">
            <p className="mb-2">Could not read BOT Chain: {statusError}</p>
            <button onClick={() => void load()} className="border border-botchain-red/50 px-2 py-0.5 tracking-widest">
              RETRY
            </button>
          </div>
        )}

        {/* ── network ────────────────────────────────────────────────────── */}
        <section className="botchain-panel">
          <div className="botchain-panel-header">
            NETWORK
            {status && (
              <span className="ml-auto flex items-center gap-2 normal-case tracking-normal">
                {/* Which endpoint answered is not a detail: the explorer proxy cannot
                    broadcast, so a read served by it is not equivalent to a node read. */}
                <span
                  className={`text-[0.6rem] tracking-widest ${
                    status.source.canBroadcast ? 'text-botchain-green' : 'text-botchain-amber'
                  }`}
                  title={status.source.endpoint}
                >
                  {status.source.canBroadcast ? 'NODE' : 'EXPLORER PROXY (READ-ONLY)'}
                </span>
                <span className="text-[0.6rem] tracking-widest text-botchain-dim">
                  {status.source.elapsedMs}ms
                </span>
              </span>
            )}
          </div>

          <div className="grid grid-cols-2 gap-px bg-botchain-border/40 sm:grid-cols-4">
            <Metric label="CHAIN ID" value={loading ? '…' : String(status?.network.chainId ?? '—')} note="verified" />
            <Metric label="HEAD BLOCK" value={loading ? '…' : status ? status.head.toLocaleString() : '—'} />
            <Metric label="GAS" value={loading ? '…' : status ? `${status.gasGwei.toFixed(2)} gwei` : '—'} />
            <Metric label="BLOCK TIME" value={status ? `${status.blockSeconds}s` : '—'} note="measured" />
          </div>
        </section>

        {/* ── venues / flow ──────────────────────────────────────────────── */}
        <section className="botchain-panel">
          <div className="botchain-panel-header">
            DEX VENUES
            {status && (
              <span className="ml-auto text-[0.6rem] normal-case tracking-widest text-botchain-dim">
                last {status.flow.windowBlocks.toLocaleString()} blocks
              </span>
            )}
          </div>

          <div className="space-y-2 p-3 text-xs">
            {loading && <p className="text-botchain-dim">reading factories…</p>}

            {status?.venues.map((v) => (
              <div
                key={v.factory}
                className="flex flex-wrap items-baseline gap-x-4 gap-y-1 border-b border-botchain-border/40 pb-2 last:border-0"
              >
                <span className="text-botchain-text">{v.name}</span>
                <a
                  href={`${status.network.explorer}/address/${v.factory}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-botchain-dim underline decoration-botchain-border underline-offset-2 hover:text-botchain-amber"
                >
                  factory {shortAddress(v.factory)}
                </a>
                <span className="ml-auto text-botchain-muted">
                  {v.events} pool{v.events === 1 ? '' : 's'} created
                  {v.perDay !== null && (
                    <span className="text-botchain-dim"> · {v.perDay.toFixed(2)}/day</span>
                  )}
                </span>
                {v.rangesRefused > 0 && (
                  <span className="text-botchain-red" title="Some log ranges were refused; the count is a floor.">
                    {v.rangesRefused} ranges refused
                  </span>
                )}
              </div>
            ))}

            {status && status.flow.events === 0 && (
              // The honest headline. Softening this would defeat the purpose of the page.
              <p className="pt-1 leading-relaxed text-botchain-muted">
                No pool creation in this window on any venue. Measured August 2026: two
                events across ~1,000,000 blocks (~8 days). A new-pool sniper has nothing to
                act on here, which is why the trading bot stays on Solana — re-run{' '}
                <code className="text-botchain-amber">botchain-probe</code> before revisiting
                that.
              </p>
            )}
          </div>
        </section>

        {/* ── address lookup ─────────────────────────────────────────────── */}
        <section className="botchain-panel">
          <div className="botchain-panel-header">ADDRESS LOOKUP</div>

          <div className="flex gap-2 border-b border-botchain-border p-3">
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && void search()}
              placeholder="0x…"
              spellCheck={false}
              className="min-w-0 flex-1 bg-botchain-hi px-3 py-2 text-sm text-botchain-text
                         outline-none ring-1 ring-botchain-border placeholder:text-botchain-dim
                         focus:ring-botchain-amber-dim"
            />
            <button
              onClick={() => void search()}
              disabled={looking || !query.trim()}
              className="border border-botchain-border px-4 text-xs tracking-widest
                         text-botchain-amber transition-all hover:border-botchain-amber
                         hover:text-botchain-amber-hi disabled:opacity-30"
            >
              {looking ? '…' : 'READ'}
            </button>
          </div>

          <div className="p-3 text-xs">
            {lookupError && <p className="text-botchain-red">{lookupError}</p>}

            {!lookup && !lookupError && (
              <p className="text-botchain-dim">
                Native balance, nonce and token balances. Read-only — nothing here signs.
              </p>
            )}

            {lookup && (
              <div className="space-y-2">
                <div className="flex flex-wrap items-baseline gap-x-4">
                  <span className="text-botchain-muted">{shortAddress(lookup.address)}</span>
                  <span className="text-botchain-text">
                    {formatUnits(BigInt(lookup.nativeWei), lookup.decimals)} {lookup.symbol}
                  </span>
                  <span className="text-botchain-dim">nonce {lookup.nonce}</span>
                </div>

                {lookup.tokens.map((t) => (
                  <div key={t.address} className="flex flex-wrap items-baseline gap-x-4">
                    <span className="w-16 text-botchain-amber">{t.symbol}</span>
                    {t.ok && t.balance !== null ? (
                      <span className="text-botchain-text">
                        {formatUnits(BigInt(t.balance), t.decimals)}
                      </span>
                    ) : (
                      // Rendered as a failure row rather than dropped — a token that
                      // could not be read is different from one with a zero balance.
                      <span className="text-botchain-red" title={t.error}>
                        unreadable
                      </span>
                    )}
                    <span className="text-botchain-dim">{t.name}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>

        <p className="pb-4 text-[0.65rem] leading-relaxed text-botchain-dim">
          Reads go through an ordered endpoint list — official node first, explorer proxy as
          fallback — and every response reports which one answered. Chain ID is verified
          against the endpoint on each call rather than trusted from a registry.
        </p>
      </main>
    </div>
  )
}

function Metric({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <div className="bg-botchain-surface p-3">
      <div className="text-[0.6rem] tracking-widest text-botchain-dim">{label}</div>
      <div className="mt-1 text-sm text-botchain-text">{value}</div>
      {note && <div className="text-[0.55rem] tracking-widest text-botchain-dim">{note}</div>}
    </div>
  )
}
