'use client'

import { useCallback, useEffect, useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import { Transaction } from '@solana/web3.js'

import { formatAmount } from '@/lib/escrow/program'
import { useMintLookup } from '@/lib/escrow/useMint'

// The way back out.
//
// Until this existed, /escrow could create a vault and lock funds into it and offered no
// path to take them back — the builder's own copy said "only you can withdraw" while no
// withdraw builder existed anywhere in web/. Recovering a position meant hand-crafting a
// transaction from three values (token mint, backing mint, nonce) that are recorded
// nowhere the depositor can see; the nonce in particular is chosen at deposit time.
//
// Three rules, all inherited from the rest of /escrow and all load-bearing here:
//
//  - **Nothing is invented.** No simulation branch. A read that fails renders as a
//    failure naming its reason, never as an empty list — "you have no positions" and
//    "we could not ask" are different claims, and the first one is terrifying if it is
//    wrong.
//  - **No USD, no percentages.** Raw amounts at the mint's own decimals. This panel is
//    about a quantity of money somebody is owed.
//  - **Maturity is decided by the CHAIN's clock**, which the listing returns alongside
//    the rows. The browser's clock can be minutes off, and either error is bad: showing
//    a matured position as locked hides money, showing a locked one as matured offers a
//    button that costs a signature and fails with StillLocked.
//
// No polling. Positions change when the holder acts, and this refetches after each
// action — a background timer would be load without information, and the site's polling
// rules exist to stop components growing private ones.

interface PositionRow {
  address: string
  vault: string
  tokenMint: string
  backingMint: string
  tokenAmount: string
  backingAmount: string
  createdUnix: string
  unlockUnix: string
  nonce: string
  vaultReadable: boolean
}

interface ListResponse {
  ok: boolean
  reason?: string
  detail?: string
  positions?: PositionRow[]
  measuredAt?: { unix: number }
  rpc?: { host: string; authenticated: boolean }
}

const short = (a: string) => (a.length > 12 ? `${a.slice(0, 4)}…${a.slice(-4)}` : a)

function remaining(unlockUnix: string, nowUnix: number): string {
  const left = Number(BigInt(unlockUnix) - BigInt(Math.floor(nowUnix)))
  if (left <= 0) return 'unlocked'
  const d = Math.floor(left / 86400)
  const h = Math.floor((left % 86400) / 3600)
  const m = Math.floor((left % 3600) / 60)
  if (d > 0) return `${d}d ${h}h`
  if (h > 0) return `${h}h ${m}m`
  return `${m}m`
}

export function PositionsPanel() {
  const { publicKey, signTransaction, connected } = useWallet()
  const [data, setData] = useState<ListResponse | null>(null)
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  const [note, setNote] = useState<{ ok: boolean; text: string; signature?: string } | null>(null)

  const owner = publicKey?.toBase58() ?? null

  const load = useCallback(async () => {
    if (!owner) return
    setLoading(true)
    try {
      const res = await fetch(`/api/escrow/positions?owner=${owner}`, { cache: 'no-store' })
      setData((await res.json()) as ListResponse)
    } catch (e) {
      setData({ ok: false, reason: 'unreachable', detail: e instanceof Error ? e.message : String(e) })
    } finally {
      setLoading(false)
    }
  }, [owner])

  // Keyed on the wallet, not on a timer. Reconnecting as a different wallet must not
  // leave the previous one's positions on screen.
  useEffect(() => {
    setData(null)
    setNote(null)
    if (owner) void load()
  }, [owner, load])

  const act = useCallback(
    async (row: PositionRow, action: 'withdraw' | 'extend', newUnlockUnix?: bigint) => {
      if (!owner || !signTransaction) return
      setBusy(row.address)
      setNote(null)
      try {
        const build = await fetch('/api/escrow/withdraw', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({
            owner,
            tokenMint: row.tokenMint,
            backingMint: row.backingMint,
            nonce: row.nonce,
            action,
            newUnlockUnix: newUnlockUnix?.toString(),
          }),
        })
        const quote = (await build.json()) as { ok: boolean; transaction?: string; detail?: string }
        if (!quote.ok || !quote.transaction) {
          setNote({ ok: false, text: quote.detail ?? 'could not build the transaction' })
          return
        }
        const signed = await signTransaction(Transaction.from(Buffer.from(quote.transaction, 'base64')))
        const send = await fetch('/api/escrow/send', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ transaction: signed.serialize().toString('base64') }),
        })
        const sent = (await send.json()) as { ok: boolean; signature?: string; detail?: string }
        setNote(
          sent.ok
            ? {
                ok: true,
                text: action === 'withdraw' ? 'Withdrawn — both legs returned' : 'Lock extended',
                signature: sent.signature,
              }
            : { ok: false, text: sent.detail ?? 'send failed' },
        )
        if (sent.ok) await load()
      } catch (e) {
        setNote({ ok: false, text: e instanceof Error ? e.message : String(e) })
      } finally {
        setBusy(null)
      }
    },
    [owner, signTransaction, load],
  )

  if (!connected) {
    return (
      <section className="border border-escrow-border bg-escrow-surface">
        <Header />
        <p className="px-4 py-6 text-xs text-escrow-dim">
          Connect the wallet that made the deposits. Positions are keyed to the depositor, and no
          other wallet can withdraw them.
        </p>
      </section>
    )
  }

  const rows = data?.positions ?? []
  // The chain's clock as of the listing, not the browser's. See the note at the top.
  const now = data?.measuredAt?.unix ?? Math.floor(Date.now() / 1000)

  return (
    <section className="border border-escrow-border bg-escrow-surface">
      <Header onRefresh={load} loading={loading} />

      {/* A failed read is never an empty list. Each reason renders distinctly, because
          "not deployed", "the RPC refused" and "you have no positions" send a reader to
          three different places and only one of them is about their own money. */}
      {data && !data.ok && (
        <div className="px-4 py-4 text-xs">
          <span className="text-escrow-alarm font-bold">
            {data.reason === 'not_configured' ? 'Vault program not deployed' : 'Could not read your positions'}
          </span>
          <div className="text-escrow-dim mt-1">{data.detail}</div>
          <div className="text-escrow-dim mt-1">
            This is not a statement that you have none — it is a statement that we could not ask.
          </div>
        </div>
      )}

      {data?.ok && rows.length === 0 && (
        <p className="px-4 py-6 text-xs text-escrow-dim">
          No open positions for {short(owner ?? '')}. A vault you funded from another wallet will
          not appear here.
        </p>
      )}

      {rows.length > 0 && (
        <div className="divide-y divide-escrow-border">
          {rows.map(row => (
            <Row
              key={row.address}
              row={row}
              now={now}
              busy={busy === row.address}
              onWithdraw={() => act(row, 'withdraw')}
              onExtend={days =>
                act(row, 'extend', BigInt(row.unlockUnix) + BigInt(days) * 86400n)
              }
            />
          ))}
        </div>
      )}

      {note && (
        <div className={`px-4 py-3 text-xs border-t border-escrow-border ${note.ok ? 'text-escrow-teal' : 'text-escrow-alarm'}`}>
          {note.text}
          {note.signature && <span className="text-escrow-dim"> — {short(note.signature)}</span>}
        </div>
      )}
    </section>
  )
}

function Header({ onRefresh, loading }: { onRefresh?: () => void; loading?: boolean } = {}) {
  return (
    <div className="flex items-center justify-between px-4 py-3 border-b border-escrow-border">
      <div>
        <h2 className="text-sm text-escrow-teal uppercase tracking-wider">Your positions</h2>
        <p className="text-[10px] text-escrow-dim mt-0.5">
          Locked reserve you can take back. Nobody else can, including whoever deployed this.
        </p>
      </div>
      {onRefresh && (
        <button
          onClick={onRefresh}
          disabled={loading}
          className="px-3 py-1.5 text-xs border border-escrow-border text-escrow-muted hover:text-escrow-text disabled:opacity-40"
        >
          {loading ? 'reading…' : 'refresh'}
        </button>
      )}
    </div>
  )
}

function Row({
  row,
  now,
  busy,
  onWithdraw,
  onExtend,
}: {
  row: PositionRow
  now: number
  busy: boolean
  onWithdraw: () => void
  onExtend: (days: number) => void
}) {
  // Decimals come from the mint account, never from a token list: a wrong `decimals` is a
  // wrong quantity of money on screen, not a wrong label.
  // Debounce 0: these addresses come from the chain, not from a keystroke.
  const tokenMint = useMintLookup(row.vaultReadable ? row.tokenMint : null, 0)
  const backingMint = useMintLookup(row.vaultReadable ? row.backingMint : null, 0)
  const decimalsOf = (m: typeof tokenMint) => (m.result?.ok ? m.result.facts.decimals : null)
  const unlocked = BigInt(Math.floor(now)) >= BigInt(row.unlockUnix)

  // An amount whose decimals have not been read yet is NOT rendered at a guessed scale.
  // Printing base units as though they were whole tokens overstates by 10^decimals.
  const amount = (raw: string, decimals: number | null) =>
    decimals === null ? `${raw} base units` : formatAmount(raw, decimals)

  if (!row.vaultReadable) {
    return (
      <div className="px-4 py-3 text-xs">
        <div className="text-escrow-alarm">Vault unreadable — {short(row.vault)}</div>
        <div className="text-escrow-dim mt-1">
          This position is still yours and still holds {row.backingAmount} backing base units. Its
          vault account could not be read, so the mints and decimals are unknown and no withdraw can
          be built. It is listed rather than hidden, because dropping it would report your money as
          absent.
        </div>
      </div>
    )
  }

  return (
    <div className="px-4 py-3">
      <div className="flex items-baseline justify-between gap-3 flex-wrap">
        <div className="text-xs text-escrow-text">
          <span className="text-escrow-teal">{amount(row.backingAmount, decimalsOf(backingMint))}</span>{' '}
          <span className="text-escrow-dim">{short(row.backingMint)} reserve</span>
          {row.tokenAmount !== '0' && (
            <>
              <span className="text-escrow-dim"> + </span>
              <span className="text-escrow-teal">{amount(row.tokenAmount, decimalsOf(tokenMint))}</span>{' '}
              <span className="text-escrow-dim">{short(row.tokenMint)}</span>
            </>
          )}
        </div>
        <div className={`text-[10px] uppercase tracking-wider ${unlocked ? 'text-escrow-teal-hi' : 'text-escrow-muted'}`}>
          {unlocked ? 'unlocked' : `locked · ${remaining(row.unlockUnix, now)}`}
        </div>
      </div>

      <div className="text-[10px] text-escrow-dim mt-1">
        nonce {row.nonce} · unlocks {new Date(Number(row.unlockUnix) * 1000).toISOString().replace('T', ' ').slice(0, 16)}Z
      </div>

      <div className="flex gap-1 mt-2 flex-wrap">
        <button
          onClick={onWithdraw}
          disabled={!unlocked || busy}
          title={unlocked ? 'Return both legs and close the position' : 'There is no early exit, by design'}
          className="px-3 py-1.5 text-xs border border-escrow-border text-escrow-muted hover:text-escrow-text disabled:opacity-30 disabled:hover:text-escrow-muted"
        >
          {busy ? 'working…' : 'withdraw'}
        </button>
        {/* Extending is always available — a lock may be strengthened at any time, and
            only ever lengthened. That asymmetry is the product. */}
        {[30, 90, 365].map(d => (
          <button
            key={d}
            onClick={() => onExtend(d)}
            disabled={busy}
            title={`Push the unlock ${d} days further out. This cannot be undone.`}
            className="px-3 py-1.5 text-xs border border-escrow-border text-escrow-dim hover:text-escrow-text disabled:opacity-30"
          >
            +{d}d
          </button>
        ))}
      </div>
    </div>
  )
}
