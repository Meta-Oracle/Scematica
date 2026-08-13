'use client'

import { useCallback, useState } from 'react'

import { formatAmount, type SolvencyVerdict } from '@/lib/escrow/program'

// The Escrow Market proof-of-reserve console.
//
// This page has one job: let anyone check whether the reserve behind a token is
// actually there, without trusting whoever operates the site. That shapes every
// decision below.
//
//  - **Nothing is ever invented.** `/api/escrow/vault` has no simulation branch, and
//    neither does this. A failed read renders as a failure, an unconfigured program
//    renders as "not deployed". Neither renders as a zero, because "no reserve" and
//    "could not read the reserve" are completely different claims and only one of them
//    is an accusation.
//  - **No USD anywhere.** The program stores no price and consults no oracle; showing a
//    dollar figure here would reintroduce exactly the manipulable, arguable number the
//    whole design removes. Raw amounts and decimals go out; valuation is the reader's.
//  - **Provenance travels with the figure.** Slot, timestamp and RPC host are shown
//    beside the number, because a reserve figure nobody can re-derive is the thing this
//    product exists to replace.
//  - **No polling.** A one-shot read on submit. Reserve balances change on the timescale
//    of deposits, so a background timer would be load without information — and the
//    site's polling rules exist precisely to stop components growing private timers.

interface VaultResponse {
  ok: boolean
  reason?: string
  detail?: string
  programId?: string
  vault?: string
  state?: {
    tokenMint: string
    backingMint: string
    tokenVault: string
    backingVault: string
    totalTokenLocked: string
    totalBackingLocked: string
    positionsOpen: string
    positionsLifetime: string
  }
  balances?: {
    token: string
    backing: string
    tokenDecimals: number
    backingDecimals: number
  }
  solvency?: { token: SolvencyVerdict; backing: SolvencyVerdict }
  measuredAt?: { slot: number; fetchedAt: string }
  rpc?: { host: string; authenticated: boolean }
}

const VERDICT_STYLE: Record<SolvencyVerdict, string> = {
  backed: 'text-escrow-teal',
  donated: 'text-escrow-teal-hi',
  SHORTFALL: 'text-escrow-alarm font-bold',
}

const VERDICT_NOTE: Record<SolvencyVerdict, string> = {
  backed: 'on-chain balance equals the recorded total',
  donated: 'balance exceeds the recorded total — a donation, permanently stuck',
  SHORTFALL: 'balance is BELOW the recorded total — accounting and tokens disagree',
}

/**
 * `embedded` drops the full-page chrome so this can sit under the market board without a
 * second banner and a second min-h-screen. The verification logic is identical either
 * way — this is layout, not behaviour, and the guarantees above hold in both modes.
 */
export function EscrowConsole({ embedded = false }: { embedded?: boolean } = {}) {
  const [token, setToken] = useState('')
  const [backing, setBacking] = useState('')
  const [data, setData] = useState<VaultResponse | null>(null)
  const [loading, setLoading] = useState(false)

  const check = useCallback(async () => {
    if (!token.trim() || !backing.trim()) return
    setLoading(true)
    setData(null)
    try {
      const res = await fetch(
        `/api/escrow/vault?token=${encodeURIComponent(token.trim())}&backing=${encodeURIComponent(backing.trim())}`,
      )
      setData((await res.json()) as VaultResponse)
    } catch (error) {
      // A network failure is still a failure to read, not an absence of reserve.
      setData({
        ok: false,
        reason: 'network',
        detail: error instanceof Error ? error.message : String(error),
      })
    } finally {
      setLoading(false)
    }
  }, [token, backing])

  return (
    <div
      className={
        embedded
          ? 'escrow-root text-escrow-text font-mono'
          : 'escrow-root min-h-screen bg-escrow-black text-escrow-text font-mono p-6'
      }
    >
      {!embedded && (
        <header className="max-w-4xl mx-auto mb-8 border-b border-escrow-border pb-4">
          <h1 className="text-2xl text-escrow-teal tracking-wide">SCEMA ESCROW MARKET</h1>
          <p className="text-escrow-muted text-sm mt-2 max-w-2xl">
            Time-locked, non-custodial backing for any Solana token. Read a vault below to
            see how much reserve stands behind it, measured on-chain at a named slot.
          </p>
          <p className="text-escrow-dim text-xs mt-2 max-w-2xl">
            No prices are shown and no oracle is consulted — raw amounts only. Any
            valuation is yours to compute, so there is no feed here to manipulate.
          </p>
        </header>
      )}

      <section className={embedded ? '' : 'max-w-4xl mx-auto'}>
        <div className="grid gap-3 sm:grid-cols-2">
          <label className="block">
            <span className="text-xs text-escrow-muted uppercase tracking-wider">Token mint</span>
            <input
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="the token being backed"
              className="w-full mt-1 bg-escrow-surface border border-escrow-border focus:border-escrow-border-hi outline-none px-3 py-2 text-sm text-escrow-text placeholder:text-escrow-dim"
            />
          </label>
          <label className="block">
            <span className="text-xs text-escrow-muted uppercase tracking-wider">
              Backing mint
            </span>
            <input
              value={backing}
              onChange={(e) => setBacking(e.target.value)}
              placeholder="wBTC / wETH / reserve asset"
              className="w-full mt-1 bg-escrow-surface border border-escrow-border focus:border-escrow-border-hi outline-none px-3 py-2 text-sm text-escrow-text placeholder:text-escrow-dim"
            />
          </label>
        </div>

        <button
          onClick={check}
          disabled={loading || !token.trim() || !backing.trim()}
          className="mt-4 px-5 py-2 border border-escrow-border-hi text-escrow-teal hover:bg-escrow-hi disabled:opacity-40 disabled:hover:bg-transparent text-sm tracking-wider"
        >
          {loading ? 'READING CHAIN…' : 'VERIFY RESERVE'}
        </button>

        {data && !data.ok && (
          <div className="mt-6 border border-escrow-alarm/50 bg-escrow-surface p-4">
            <div className="text-escrow-alarm text-sm uppercase tracking-wider">
              {data.reason === 'not_configured' ? 'Not deployed' : 'Could not read'}
            </div>
            <p className="text-escrow-muted text-sm mt-2">{data.detail}</p>
            <p className="text-escrow-dim text-xs mt-3">
              This is a failure to read, not a reserve of zero. The two are different
              claims and this page will not conflate them.
            </p>
          </div>
        )}

        {data?.ok && data.state && data.balances && data.solvency && data.measuredAt && (
          <div className="mt-6 space-y-4">
            {(data.solvency.backing === 'SHORTFALL' || data.solvency.token === 'SHORTFALL') && (
              <div className="border border-escrow-alarm bg-escrow-alarm/10 p-4 text-escrow-alarm text-sm">
                RESERVE SHORTFALL — the vault holds less than its own records claim.
                Do not treat this vault as backed.
              </div>
            )}

            <Row
              label="Reserve locked"
              recorded={data.state.totalBackingLocked}
              balance={data.balances.backing}
              decimals={data.balances.backingDecimals}
              mint={data.state.backingMint}
              verdict={data.solvency.backing}
            />
            <Row
              label="Token locked"
              recorded={data.state.totalTokenLocked}
              balance={data.balances.token}
              decimals={data.balances.tokenDecimals}
              mint={data.state.tokenMint}
              verdict={data.solvency.token}
            />

            <div className="border border-escrow-border bg-escrow-surface p-4 text-xs text-escrow-muted space-y-1">
              <Line k="Vault PDA" v={data.vault ?? '—'} />
              <Line k="Program" v={data.programId ?? '—'} />
              <Line k="Open positions" v={data.state.positionsOpen} />
              <Line k="Lifetime positions" v={data.state.positionsLifetime} />
              <Line k="Measured at slot" v={String(data.measuredAt.slot)} />
              <Line k="Fetched" v={data.measuredAt.fetchedAt} />
              <Line
                k="RPC"
                v={`${data.rpc?.host ?? 'unknown'}${data.rpc?.authenticated ? '' : ' (public fallback)'}`}
              />
            </div>

            <p className="text-escrow-dim text-xs">
              Verify this yourself: the vault PDA and both token accounts are derivable
              from the two mints and the program ID, and their balances are public. This
              page is a convenience, not an authority.
            </p>
          </div>
        )}
      </section>
    </div>
  )
}

function Row({
  label,
  recorded,
  balance,
  decimals,
  mint,
  verdict,
}: {
  label: string
  recorded: string
  balance: string
  decimals: number
  mint: string
  verdict: SolvencyVerdict
}) {
  return (
    <div className="border border-escrow-border bg-escrow-surface p-4">
      <div className="flex items-baseline justify-between gap-4 flex-wrap">
        <span className="text-xs text-escrow-muted uppercase tracking-wider">{label}</span>
        <span className={`text-xs uppercase tracking-wider ${VERDICT_STYLE[verdict]}`}>
          {verdict}
        </span>
      </div>
      <div className="text-2xl text-escrow-teal-hi mt-2 break-all">
        {formatAmount(balance, decimals)}
      </div>
      <div className="text-xs text-escrow-dim mt-2 space-y-0.5">
        <div className="break-all">mint {mint}</div>
        <div>
          recorded {formatAmount(recorded, decimals)} · {VERDICT_NOTE[verdict]}
        </div>
      </div>
    </div>
  )
}

function Line({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex gap-3 flex-wrap">
      <span className="text-escrow-dim min-w-[9rem]">{k}</span>
      <span className="text-escrow-text break-all">{v}</span>
    </div>
  )
}
