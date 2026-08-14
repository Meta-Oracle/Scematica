'use client'

import { useMemo, useState } from 'react'

import { groupByDex, rankByAge } from '@/lib/market/aggregate'
import {
  TIER_MEANING,
  TIER_NAME,
  gradeCommitment,
  summariseCommitments,
  type CommitmentTier,
  type LockLookup,
} from '@/lib/market/commitment'
import { useLocks } from '@/lib/market/useLocks'
import { useMarket, useNow, type MarketPayload } from '@/lib/market/useMarket'
import { VaultBuilder } from './VaultBuilder'
import { DEX_LABEL, formatUnits, type Dex, type MarketRow } from '@/lib/market/types'

// The Escrow Market.
//
// Every other token board on Solana ranks by volume, which is the number easiest to
// manufacture. This one ranks by whether anything is actually behind the token, and puts
// the live launch stream next to that question so the contrast is unavoidable: hundreds
// of mints an hour, essentially none of them backed by anything.
//
// Two halves that must never merge:
//
//   MARKET  — third-party observation. Price, liquidity, volume, age. Sourced from
//             Raydium and Jupiter, labelled with which API answered, and rendered as an
//             em dash when a source declined to say. Never a zero.
//   CUSTODY — read from the chain at a named slot. Raw amounts, no price, no percentage,
//             three verdicts. `NO VAULT` is the honest majority answer.
//
// The page states the market's backed share prominently. That figure will be close to
// zero, and that is the argument, not an embarrassment.

const DEX_ORDER: Dex[] = ['raydium', 'pumpfun', 'meteora', 'orca', 'jupiter', 'unknown']

type Tab = 'all' | Dex

export function MarketTerminal() {
  const { data, loading, error, lastGoodAt, refresh } = useMarket()
  const [tab, setTab] = useState<Tab>('all')

  // Tier-3 enrichment for the rows most likely to be read. The board never waits on it.
  const visible = useMemo(() => {
    const rows = data?.rows ?? []
    const scoped = tab === 'all' ? rows : rows.filter(r => r.token.dex === tab)
    return scoped.slice(0, 25).map(r => r.token.mint)
  }, [data, tab])
  const locks = useLocks(visible)

  return (
    <div className="escrow-root min-h-screen bg-escrow-black text-escrow-text font-mono">
      <Header data={data} error={error} lastGoodAt={lastGoodAt} onRefresh={refresh} />

      <div className="max-w-[1600px] mx-auto px-4 pb-16 grid gap-4 lg:grid-cols-[340px_minmax(0,1fr)]">
        <div className="space-y-4">
          <LaunchStream rows={data?.rows ?? []} loading={loading} />
        </div>

        <div className="space-y-4">
          <CustodyBanner data={data} />
          <VaultBuilder rows={data?.rows ?? []} />
          <CommitmentLadder rows={data?.rows ?? []} locks={locks} />
          <DexTabs rows={data?.rows ?? []} tab={tab} onTab={setTab} />
          <MarketTable rows={data?.rows ?? []} tab={tab} loading={loading} locks={locks} />
        </div>
      </div>
    </div>
  )
}

// ── header ───────────────────────────────────────────────────────────────────

function Header({
  data,
  error,
  lastGoodAt,
  onRefresh,
}: {
  data: MarketPayload | null
  error: string | null
  lastGoodAt: number | null
  onRefresh: () => void
}) {
  const s = data?.stats
  return (
    <header className="border-b border-escrow-border mb-4">
      <div className="max-w-[1600px] mx-auto px-4 py-5">
        <div className="flex items-start justify-between gap-6 flex-wrap">
          <div>
            <h1 className="text-2xl text-escrow-teal tracking-wide">SCEMA ESCROW MARKET</h1>
            <p className="text-escrow-muted text-sm mt-1 max-w-3xl">
              Live Solana launches, ranked by what is actually behind them. Every other
              board ranks by volume — the number easiest to manufacture.
            </p>
          </div>
          <button
            onClick={onRefresh}
            className="px-3 py-1.5 border border-escrow-border-hi text-escrow-teal hover:bg-escrow-hi text-xs tracking-wider"
          >
            REFRESH
          </button>
        </div>

        <div className="mt-4 flex gap-6 flex-wrap">
          <Stat label="Tokens listed" value={s ? String(s.totalTokens) : '—'} />
          <Stat label="Backed by a vault" value={s ? String(s.backedTokens) : '—'} accent />
          <Stat
            label="Share backed"
            value={s ? `${(s.backedShare * 100).toFixed(2)}%` : '—'}
            accent
          />
          <Stat
            label="Shortfalls"
            value={s ? String(s.shortfalls) : '—'}
            alarm={!!s && s.shortfalls > 0}
          />
        </div>

        {error && (
          <div className="mt-3 border border-escrow-alarm/50 bg-escrow-surface px-3 py-2 text-xs text-escrow-alarm">
            Last refresh failed: {error}
            {lastGoodAt && (
              <span className="text-escrow-muted">
                {' '}
                — showing the snapshot from {new Date(lastGoodAt).toLocaleTimeString()}.
                These figures are stale, not current.
              </span>
            )}
          </div>
        )}
      </div>
    </header>
  )
}

function Stat({
  label,
  value,
  accent,
  alarm,
}: {
  label: string
  value: string
  accent?: boolean
  alarm?: boolean
}) {
  const tone = alarm ? 'text-escrow-alarm' : accent ? 'text-escrow-teal-hi' : 'text-escrow-text'
  return (
    <div>
      <div className="text-[10px] text-escrow-dim uppercase tracking-wider">{label}</div>
      <div className={`text-xl ${tone}`}>{value}</div>
    </div>
  )
}

// ── live launches ────────────────────────────────────────────────────────────

function ageOf(createdAtUnix: number | null, now: number): string {
  if (createdAtUnix === null) return '—'
  const secs = Math.max(0, Math.floor(now / 1000) - createdAtUnix)
  if (secs < 60) return `${secs}s`
  if (secs < 3600) return `${Math.floor(secs / 60)}m`
  if (secs < 86_400) return `${Math.floor(secs / 3600)}h`
  return `${Math.floor(secs / 86_400)}d`
}

function LaunchStream({ rows, loading }: { rows: MarketRow[]; loading: boolean }) {
  const now = useNow()
  const launches = useMemo(() => rankByAge(rows).slice(0, 40), [rows])

  return (
    <section className="border border-escrow-border bg-escrow-surface">
      <div className="px-3 py-2 border-b border-escrow-border flex items-center justify-between">
        <span className="text-xs text-escrow-teal uppercase tracking-wider">Live launches</span>
        <span className="text-[10px] text-escrow-dim">newest first</span>
      </div>

      {loading && launches.length === 0 && (
        <div className="p-4 text-xs text-escrow-dim">reading the market…</div>
      )}
      {!loading && launches.length === 0 && (
        <div className="p-4 text-xs text-escrow-dim">
          No launches reported. This is an empty feed, not an empty market — check the
          source panel below.
        </div>
      )}

      <ul className="divide-y divide-escrow-border/50 max-h-[560px] overflow-y-auto">
        {launches.map(r => (
          <li key={r.token.mint} className="px-3 py-2 hover:bg-escrow-hi/40">
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-escrow-teal-hi text-sm truncate">{r.token.symbol}</span>
              <span className="text-[10px] text-escrow-dim shrink-0">
                {ageOf(r.token.createdAtUnix, now)}
              </span>
            </div>
            <div className="flex items-baseline justify-between gap-2 mt-0.5">
              <span className="text-[10px] text-escrow-muted truncate">
                {DEX_LABEL[r.token.dex]}
                {r.token.holderCount !== null && ` · ${r.token.holderCount} holders`}
              </span>
              <BackingPill row={r} compact />
            </div>
          </li>
        ))}
      </ul>
    </section>
  )
}

// The standalone Sources panel was removed with the builder redesign. Provenance is not
// lost: every row's Venue cell renders `token.source`, so which API answered for a given
// figure is still attributable per row rather than in one aggregate box. The no-simulation
// guarantee is enforced in lib/market/sources.ts, not by a panel describing it.

// ── custody state ────────────────────────────────────────────────────────────

function CustodyBanner({ data }: { data: MarketPayload | null }) {
  const c = data?.custody
  if (!c) return null

  if (!c.programId) {
    return (
      <div className="border border-escrow-border bg-escrow-surface px-4 py-3 text-xs">
        <span className="text-escrow-teal uppercase tracking-wider">Vault program not deployed</span>
        <p className="text-escrow-muted mt-1">
          No vault can exist yet, so every row below reads NO VAULT. That is the correct
          answer today — not a failure to read.
        </p>
      </div>
    )
  }

  if (!c.available) {
    return (
      <div className="border border-escrow-alarm/50 bg-escrow-surface px-4 py-3 text-xs">
        <span className="text-escrow-alarm uppercase tracking-wider">Could not read the chain</span>
        <p className="text-escrow-muted mt-1">
          {c.error} — backing is UNKNOWN below, not absent. Those are different claims and
          this page will not conflate them.
        </p>
      </div>
    )
  }

  return (
    <div className="border border-escrow-border bg-escrow-surface px-4 py-2 text-[11px] text-escrow-muted flex gap-4 flex-wrap">
      <span>
        <span className="text-escrow-dim">vaults </span>
        <span className="text-escrow-teal-hi">{c.vaultCount}</span>
      </span>
      <span>
        <span className="text-escrow-dim">slot </span>
        {c.measuredAtSlot ?? '—'}
      </span>
      <span>
        <span className="text-escrow-dim">rpc </span>
        {c.rpc.host ?? '—'}
        {!c.rpc.authenticated && ' (public)'}
      </span>
      <span className="text-escrow-dim">program {c.programId.slice(0, 8)}…</span>
    </div>
  )
}

// ── commitment ladder ────────────────────────────────────────────────────────

const TIER_TONE: Record<CommitmentTier, string> = {
  0: 'text-escrow-dim',
  1: 'text-escrow-muted',
  2: 'text-escrow-text',
  3: 'text-escrow-teal',
  4: 'text-escrow-teal-hi',
}

function CommitmentLadder({
  rows,
  locks,
}: {
  rows: MarketRow[]
  locks: Record<string, LockLookup>
}) {
  // Computed client-side FROM THE SAME rows and locks the table renders, rather than
  // from the server's `commitments`. The server cannot see lock data — tier 3 is a
  // separate on-demand lookup — so a server-computed strip would permanently read
  // LOCKED=0 while rows beneath it showed LOCKED. One grader, one set of inputs.
  const c = useMemo(() => (rows.length ? summariseCommitments(rows, locks) : null), [rows, locks])
  const checked = Object.keys(locks).length
  const tiers: CommitmentTier[] = [0, 1, 2, 3, 4]

  return (
    <section className="border border-escrow-border bg-escrow-surface">
      <div className="px-3 py-2 border-b border-escrow-border flex items-baseline justify-between gap-3 flex-wrap">
        <span className="text-xs text-escrow-teal uppercase tracking-wider">
          Commitment ladder
        </span>
        <span className="text-[10px] text-escrow-dim">
          ordered by cost to fake · {c ? `${(c.anyCommitmentShare * 100).toFixed(1)}% at tier 1+` : '—'}
        </span>
      </div>
      <div className="grid grid-cols-2 sm:grid-cols-5 divide-x divide-escrow-border/50">
        {tiers.map(t => (
          <div key={t} className="px-3 py-2" title={TIER_MEANING[t]}>
            <div className="text-[10px] text-escrow-dim uppercase tracking-wider">
              {t} · {TIER_NAME[t]}
            </div>
            <div className={`text-lg ${TIER_TONE[t]}`}>{c ? c.counts[t] : '—'}</div>
          </div>
        ))}
      </div>
      <p className="px-3 py-2 text-[10px] text-escrow-dim border-t border-escrow-border leading-snug">
        Each rung is something the issuer gave up irreversibly and a stranger can check
        without trusting this page. Tiers 1–2 need no program at all and are evaluated for
        every row. <span className="text-escrow-muted">Tier 3 is a live lookup against
        Jupiter Lock and Streamflow and has been checked for {checked} of {rows.length}{' '}
        rows</span> — the rest are unevaluated, which is not the same as unlocked. Tier 4
        is a Scema vault. A rung is not a score: it names exactly what was verified.
      </p>
    </section>
  )
}

function CommitmentCell({ row, locks }: { row: MarketRow; locks?: LockLookup }) {
  const c = gradeCommitment(row, locks)
  const note = c.evidence[0] ?? c.unknown[0] ?? TIER_MEANING[0]
  return (
    <div title={note}>
      <div className={`${TIER_TONE[c.tier]} tracking-wider`}>{TIER_NAME[c.tier]}</div>
      <div className="text-[10px] text-escrow-dim truncate max-w-[220px]">{note}</div>
    </div>
  )
}

// ── dex tabs ─────────────────────────────────────────────────────────────────

function DexTabs({
  rows,
  tab,
  onTab,
}: {
  rows: MarketRow[]
  tab: Tab
  onTab: (t: Tab) => void
}) {
  const groups = useMemo(() => groupByDex(rows), [rows])
  const present = DEX_ORDER.filter(d => (groups.get(d)?.length ?? 0) > 0)

  return (
    <div className="flex gap-1 flex-wrap">
      <TabButton active={tab === 'all'} onClick={() => onTab('all')}>
        ALL <span className="text-escrow-dim">{rows.length}</span>
      </TabButton>
      {present.map(d => (
        <TabButton key={d} active={tab === d} onClick={() => onTab(d)}>
          {DEX_LABEL[d].toUpperCase()}{' '}
          <span className="text-escrow-dim">{groups.get(d)?.length ?? 0}</span>
        </TabButton>
      ))}
    </div>
  )
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      onClick={onClick}
      className={`px-3 py-1.5 text-xs tracking-wider border ${
        active
          ? 'border-escrow-border-hi text-escrow-teal bg-escrow-hi'
          : 'border-escrow-border text-escrow-muted hover:text-escrow-text'
      }`}
    >
      {children}
    </button>
  )
}

// ── the board ────────────────────────────────────────────────────────────────

function usd(v: number | null): string {
  if (v === null) return '—'
  if (v >= 1_000_000_000) return `$${(v / 1_000_000_000).toFixed(2)}B`
  if (v >= 1_000_000) return `$${(v / 1_000_000).toFixed(2)}M`
  if (v >= 1_000) return `$${(v / 1_000).toFixed(1)}K`
  if (v >= 1) return `$${v.toFixed(2)}`
  if (v > 0) return `$${v.toPrecision(3)}`
  return '$0'
}

function BackingPill({ row, compact }: { row: MarketRow; compact?: boolean }) {
  const b = row.backing
  if (!b) {
    return (
      <span className={`${compact ? 'text-[10px]' : 'text-xs'} text-escrow-dim tracking-wider`}>
        NO VAULT
      </span>
    )
  }
  const tone =
    b.verdict === 'SHORTFALL'
      ? 'text-escrow-alarm font-bold'
      : b.verdict === 'donated'
        ? 'text-escrow-teal-hi'
        : 'text-escrow-teal'
  return (
    <span className={`${compact ? 'text-[10px]' : 'text-xs'} ${tone} tracking-wider`}>
      {b.verdict.toUpperCase()}
    </span>
  )
}

function MarketTable({
  rows,
  tab,
  loading,
  locks,
}: {
  rows: MarketRow[]
  tab: Tab
  loading: boolean
  locks: Record<string, LockLookup>
}) {
  const now = useNow(5_000)
  const shown = useMemo(
    () => (tab === 'all' ? rows : rows.filter(r => r.token.dex === tab)),
    [rows, tab],
  )

  return (
    <section className="border border-escrow-border bg-escrow-surface overflow-x-auto">
      <table className="w-full text-xs min-w-[900px]">
        <thead>
          <tr className="text-escrow-dim border-b border-escrow-border">
            <Th className="text-left">Token</Th>
            <Th className="text-left">Venue</Th>
            <Th>Price</Th>
            <Th>Liquidity</Th>
            <Th>24h vol</Th>
            <Th>24h %</Th>
            <Th>Age</Th>
            <Th className="text-left border-l border-escrow-border">Commitment</Th>
            <Th>Backing</Th>
            <Th className="text-left">Reserve held</Th>
          </tr>
        </thead>
        <tbody>
          {shown.map(r => (
            <tr
              key={r.token.mint}
              className={`border-b border-escrow-border/40 hover:bg-escrow-hi/30 ${
                r.backing?.verdict === 'SHORTFALL' ? 'bg-escrow-alarm/10' : ''
              }`}
            >
              <td className="px-3 py-2">
                <div className="text-escrow-teal-hi">{r.token.symbol}</div>
                <div className="text-[10px] text-escrow-dim truncate max-w-[220px]">
                  {r.token.name || r.token.mint}
                </div>
              </td>
              <td className="px-3 py-2">
                <div className="text-escrow-text">{DEX_LABEL[r.token.dex]}</div>
                <div className="text-[10px] text-escrow-dim truncate max-w-[200px]">
                  {r.token.source}
                </div>
              </td>
              <Td>{usd(r.token.priceUsd)}</Td>
              <Td>{usd(r.token.liquidityUsd)}</Td>
              <Td>{usd(r.token.volume24hUsd)}</Td>
              <Td
                className={
                  r.token.priceChange24hPct === null
                    ? ''
                    : r.token.priceChange24hPct >= 0
                      ? 'text-escrow-teal'
                      : 'text-escrow-alarm'
                }
              >
                {r.token.priceChange24hPct === null
                  ? '—'
                  : `${r.token.priceChange24hPct >= 0 ? '+' : ''}${r.token.priceChange24hPct.toFixed(1)}%`}
              </Td>
              <Td>{ageOf(r.token.createdAtUnix, now)}</Td>
              <td className="px-3 py-2 text-xs border-l border-escrow-border">
                <CommitmentCell row={r} locks={locks[r.token.mint]} />
              </td>
              <td className="px-3 py-2 text-center">
                <BackingPill row={r} />
              </td>
              <td className="px-3 py-2">
                {r.backing ? (
                  <div>
                    <div className="text-escrow-teal-hi">
                      {formatUnits(r.backing.actualBacking, r.backing.backingDecimals)}
                    </div>
                    <div className="text-[10px] text-escrow-dim">
                      recorded {formatUnits(r.backing.recordedBacking, r.backing.backingDecimals)}
                      {' · slot '}
                      {r.backing.measuredAtSlot}
                    </div>
                  </div>
                ) : (
                  <span className="text-escrow-dim">—</span>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      {loading && shown.length === 0 && (
        <div className="p-6 text-xs text-escrow-dim">reading the market…</div>
      )}
      {!loading && shown.length === 0 && (
        <div className="p-6 text-xs text-escrow-dim">No tokens on this venue right now.</div>
      )}

      <p className="px-3 py-2 text-[10px] text-escrow-dim border-t border-escrow-border leading-snug">
        Price, liquidity and volume are third-party observations, attributed per row. The
        backing columns are read from the chain at the stated slot and carry no price —
        raw amounts only, so there is no feed to manipulate. An em dash means the source
        did not report the figure; it never means zero.
      </p>
    </section>
  )
}

function Th({ children, className = '' }: { children: React.ReactNode; className?: string }) {
  return (
    <th className={`px-3 py-2 font-normal uppercase tracking-wider text-[10px] ${className || 'text-right'}`}>
      {children}
    </th>
  )
}

function Td({ children, className = '' }: { children: React.ReactNode; className?: string }) {
  return <td className={`px-3 py-2 text-right ${className}`}>{children}</td>
}
