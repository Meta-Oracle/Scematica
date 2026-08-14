'use client'

import { useCallback, useEffect, useMemo, useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { Transaction } from '@solana/web3.js'

import type { MarketRow } from '@/lib/market/types'

// Create a vault: pick a token, pick what backs it, lock it.
//
// Amount handling is the part most likely to lose someone money, so it never touches a
// float. Human input is parsed digit-by-digit into base units as a BigInt — `parseFloat`
// on "0.1" at 9 decimals gives 100000000.00000001, and a token amount that is wrong in
// the last place is a transfer of the wrong quantity.
//
// The cost preview comes from /api/escrow/build, which reads the chain: it knows whether
// the vault already exists, whether your token accounts exist, and what rent each costs.
// Nothing here is guessed, and when a figure is an estimate (Token-2022 extensions) it
// says so rather than presenting an exact-looking number.

const DAY = 24 * 60 * 60
const LOCKS = [
  { label: '7 days', secs: 7 * DAY, note: 'the program minimum' },
  { label: '30 days', secs: 30 * DAY, note: '' },
  { label: '90 days', secs: 90 * DAY, note: '' },
  { label: '1 year', secs: 365 * DAY, note: '' },
  { label: '4 years', secs: 4 * 365 * DAY, note: '' },
]

/** Reserve assets worth offering first. Every one is a wrapped claim — see DEPLOY.md. */
const BACKING_PRESETS = [
  { symbol: 'cbBTC', mint: 'cbbtcf3aa214zXHbiAZQwf4122FBYbraNdFqgw4iMij', note: 'Coinbase-issued BTC' },
  { symbol: 'wBTC', mint: '3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh', note: 'Wormhole-wrapped BTC' },
  { symbol: 'wETH', mint: '7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs', note: 'Wormhole-wrapped ETH' },
  { symbol: 'USDC', mint: 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', note: 'Circle USD' },
  { symbol: 'SOL', mint: 'So11111111111111111111111111111111111111112', note: 'wrapped SOL' },
]

interface Selected {
  mint: string
  symbol: string
  decimals: number
}

interface CostLine {
  label: string
  lamports: number
}

interface BuildResponse {
  ok: boolean
  reason?: string
  detail?: string
  transaction?: string
  vault?: string
  needsVault?: boolean
  decimals?: { token: number; backing: number }
  balances?: { token: string; backing: string; lamports: number }
  costs?: { lineItems: CostLine[]; totalLamports: number; estimated: boolean }
  shortfalls?: string[]
}

/** Human decimal string → base units, without a float anywhere in the path. */
function toBaseUnits(input: string, decimals: number): bigint | null {
  const s = input.trim()
  if (!s) return 0n
  if (!/^\d*\.?\d*$/.test(s)) return null
  const [whole = '', frac = ''] = s.split('.')
  if (frac.length > decimals) return null
  const padded = frac.padEnd(decimals, '0')
  try {
    return BigInt((whole || '0') + (decimals > 0 ? padded : ''))
  } catch {
    return null
  }
}

function fmtSol(lamports: number): string {
  return (lamports / 1e9).toFixed(6).replace(/0+$/, '').replace(/\.$/, '')
}

export function VaultBuilder({ rows }: { rows: MarketRow[] }) {
  const { publicKey, signTransaction, connected } = useWallet()

  const [token, setToken] = useState<Selected | null>(null)
  const [backing, setBacking] = useState<Selected | null>(null)
  const [tokenAmt, setTokenAmt] = useState('')
  const [backingAmt, setBackingAmt] = useState('')
  const [lockSecs, setLockSecs] = useState(LOCKS[0].secs)

  const [quote, setQuote] = useState<BuildResponse | null>(null)
  const [quoting, setQuoting] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null)

  const tokenBase = token ? toBaseUnits(tokenAmt, token.decimals) : null
  const backingBase = backing ? toBaseUnits(backingAmt, backing.decimals) : null
  const amountsValid = tokenBase !== null && backingBase !== null && backingBase > 0n

  const ready = Boolean(token && backing && amountsValid && publicKey)

  // Quote whenever the inputs settle. Debounced — every keystroke would otherwise be a
  // chain read.
  useEffect(() => {
    if (!ready || !token || !backing || !publicKey) {
      setQuote(null)
      return
    }
    let alive = true
    const id = setTimeout(async () => {
      setQuoting(true)
      try {
        const res = await fetch('/api/escrow/build', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({
            owner: publicKey.toBase58(),
            tokenMint: token.mint,
            backingMint: backing.mint,
            tokenAmount: (tokenBase ?? 0n).toString(),
            backingAmount: (backingBase ?? 0n).toString(),
            lockSecs,
            nonce: Date.now().toString(),
          }),
        })
        const json = (await res.json()) as BuildResponse
        if (alive) setQuote(json)
      } catch (e) {
        if (alive) setQuote({ ok: false, reason: 'network', detail: String(e) })
      } finally {
        if (alive) setQuoting(false)
      }
    }, 500)
    return () => {
      alive = false
      clearTimeout(id)
    }
  }, [ready, token, backing, tokenBase, backingBase, lockSecs, publicKey])

  const submit = useCallback(async () => {
    if (!quote?.ok || !quote.transaction || !signTransaction) return
    setSubmitting(true)
    setResult(null)
    try {
      const tx = Transaction.from(Buffer.from(quote.transaction, 'base64'))
      const signed = await signTransaction(tx)
      const res = await fetch('/api/escrow/send', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          transaction: signed.serialize().toString('base64'),
        }),
      })
      const json = (await res.json()) as { ok: boolean; signature?: string; detail?: string }
      setResult(
        json.ok
          ? { ok: true, text: `Confirmed — ${json.signature}` }
          : { ok: false, text: json.detail ?? 'send failed' },
      )
    } catch (e) {
      setResult({ ok: false, text: e instanceof Error ? e.message : String(e) })
    } finally {
      setSubmitting(false)
    }
  }, [quote, signTransaction])

  return (
    <section className="border border-escrow-border bg-escrow-surface">
      <div className="px-4 py-3 border-b border-escrow-border flex items-center justify-between gap-4 flex-wrap">
        <div>
          <h2 className="text-sm text-escrow-teal uppercase tracking-wider">Create a vault</h2>
          <p className="text-[11px] text-escrow-dim mt-0.5">
            Lock a reserve behind a token. Non-custodial — only you can withdraw, and only
            after the lock elapses.
          </p>
        </div>
        <WalletMultiButton
          style={{
            backgroundColor: 'transparent',
            border: '1px solid var(--escrow-border-hi, #1c5c58)',
            borderRadius: 0,
            fontFamily: 'inherit',
            fontSize: '12px',
            height: '34px',
            letterSpacing: '0.05em',
          }}
        />
      </div>

      <div className="grid gap-4 lg:grid-cols-2 p-4">
        <TokenPicker
          label="Token to back"
          hint="the token whose holders you want to reassure"
          rows={rows}
          selected={token}
          onSelect={setToken}
          exclude={backing?.mint}
        />
        <TokenPicker
          label="Backing it with"
          hint="the reserve asset locked behind it"
          rows={rows}
          selected={backing}
          onSelect={setBacking}
          exclude={token?.mint}
          presets={BACKING_PRESETS}
        />
      </div>

      <div className="grid gap-4 sm:grid-cols-2 px-4 pb-4">
        <AmountField
          label={`Token amount${token ? ` (${token.symbol})` : ''}`}
          value={tokenAmt}
          onChange={setTokenAmt}
          disabled={!token}
          invalid={tokenAmt !== '' && tokenBase === null}
          note="optional — a pure-reserve deposit is allowed"
        />
        <AmountField
          label={`Backing amount${backing ? ` (${backing.symbol})` : ''}`}
          value={backingAmt}
          onChange={setBackingAmt}
          disabled={!backing}
          invalid={backingAmt !== '' && backingBase === null}
          note="required — must be greater than zero"
        />
      </div>

      <div className="px-4 pb-4">
        <div className="text-[10px] text-escrow-dim uppercase tracking-wider mb-2">Lock duration</div>
        <div className="flex gap-1 flex-wrap">
          {LOCKS.map(l => (
            <button
              key={l.secs}
              onClick={() => setLockSecs(l.secs)}
              title={l.note}
              className={`px-3 py-1.5 text-xs border ${
                lockSecs === l.secs
                  ? 'border-escrow-border-hi text-escrow-teal bg-escrow-hi'
                  : 'border-escrow-border text-escrow-muted hover:text-escrow-text'
              }`}
            >
              {l.label}
            </button>
          ))}
        </div>
        <p className="text-[10px] text-escrow-dim mt-2">
          Withdrawal is impossible until the lock elapses — including for you. The 7-day
          floor exists so &ldquo;backed&rdquo; cannot mean a position opened and closed
          inside a block for a screenshot.
        </p>
      </div>

      <CostPanel quote={quote} quoting={quoting} connected={connected} ready={ready} />

      <div className="px-4 pb-4">
        <button
          onClick={submit}
          disabled={!quote?.ok || submitting || (quote.shortfalls?.length ?? 0) > 0}
          className="px-5 py-2 border border-escrow-border-hi text-escrow-teal hover:bg-escrow-hi disabled:opacity-40 disabled:hover:bg-transparent text-sm tracking-wider"
        >
          {submitting ? 'AWAITING SIGNATURE…' : 'CREATE VAULT & LOCK'}
        </button>

        {result && (
          <div
            className={`mt-3 border px-3 py-2 text-xs break-all ${
              result.ok
                ? 'border-escrow-border-hi text-escrow-teal'
                : 'border-escrow-alarm/50 text-escrow-alarm'
            }`}
          >
            {result.text}
          </div>
        )}
      </div>
    </section>
  )
}

// ── pickers ──────────────────────────────────────────────────────────────────

function TokenPicker({
  label,
  hint,
  rows,
  selected,
  onSelect,
  exclude,
  presets,
}: {
  label: string
  hint: string
  rows: MarketRow[]
  selected: Selected | null
  onSelect: (s: Selected | null) => void
  exclude?: string
  presets?: { symbol: string; mint: string; note: string }[]
}) {
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState(false)

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase()
    return rows
      .filter(r => r.token.mint !== exclude)
      .filter(
        r =>
          !q ||
          r.token.symbol.toLowerCase().includes(q) ||
          r.token.name.toLowerCase().includes(q) ||
          r.token.mint.toLowerCase().startsWith(q),
      )
      .slice(0, 40)
  }, [rows, query, exclude])

  return (
    <div>
      <div className="text-[10px] text-escrow-dim uppercase tracking-wider">{label}</div>
      <div className="text-[10px] text-escrow-dim mb-1">{hint}</div>

      {selected ? (
        <div className="flex items-center justify-between gap-2 border border-escrow-border-hi bg-escrow-black px-3 py-2">
          <div className="min-w-0">
            <div className="text-escrow-teal-hi text-sm">{selected.symbol}</div>
            <div className="text-[10px] text-escrow-dim truncate">{selected.mint}</div>
          </div>
          <button
            onClick={() => {
              onSelect(null)
              setOpen(true)
            }}
            className="text-[10px] text-escrow-muted hover:text-escrow-text shrink-0"
          >
            CHANGE
          </button>
        </div>
      ) : (
        <>
          <input
            value={query}
            onChange={e => {
              setQuery(e.target.value)
              setOpen(true)
            }}
            onFocus={() => setOpen(true)}
            placeholder="search symbol, name, or paste a mint"
            className="w-full bg-escrow-black border border-escrow-border focus:border-escrow-border-hi outline-none px-3 py-2 text-sm text-escrow-text placeholder:text-escrow-dim"
          />

          {presets && !query && (
            <div className="flex gap-1 flex-wrap mt-2">
              {presets.map(p => (
                <button
                  key={p.mint}
                  title={p.note}
                  onClick={() => {
                    const row = rows.find(r => r.token.mint === p.mint)
                    onSelect({
                      mint: p.mint,
                      symbol: p.symbol,
                      // Fall back to 6 only when the board has never seen the mint; the
                      // build route re-reads decimals from the chain regardless, so a
                      // wrong guess here cannot reach a transaction.
                      decimals: row?.token.decimals ?? 6,
                    })
                    setOpen(false)
                  }}
                  className="px-2 py-1 text-[11px] border border-escrow-border text-escrow-muted hover:text-escrow-teal"
                >
                  {p.symbol}
                </button>
              ))}
            </div>
          )}

          {open && matches.length > 0 && (
            <ul className="mt-1 max-h-56 overflow-y-auto border border-escrow-border divide-y divide-escrow-border/50">
              {matches.map(r => (
                <li key={r.token.mint}>
                  <button
                    onClick={() => {
                      onSelect({
                        mint: r.token.mint,
                        symbol: r.token.symbol,
                        decimals: r.token.decimals,
                      })
                      setOpen(false)
                      setQuery('')
                    }}
                    className="w-full text-left px-3 py-2 hover:bg-escrow-hi/40"
                  >
                    <div className="flex items-baseline justify-between gap-2">
                      <span className="text-escrow-teal-hi text-sm">{r.token.symbol}</span>
                      <span className="text-[10px] text-escrow-dim">{r.token.dex}</span>
                    </div>
                    <div className="text-[10px] text-escrow-dim truncate">{r.token.mint}</div>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  )
}

function AmountField({
  label,
  value,
  onChange,
  disabled,
  invalid,
  note,
}: {
  label: string
  value: string
  onChange: (v: string) => void
  disabled: boolean
  invalid: boolean
  note: string
}) {
  return (
    <label className="block">
      <span className="text-[10px] text-escrow-dim uppercase tracking-wider">{label}</span>
      <input
        value={value}
        onChange={e => onChange(e.target.value)}
        disabled={disabled}
        inputMode="decimal"
        placeholder={disabled ? 'select a token first' : '0.0'}
        className={`w-full mt-1 bg-escrow-black border outline-none px-3 py-2 text-sm text-escrow-text placeholder:text-escrow-dim disabled:opacity-40 ${
          invalid ? 'border-escrow-alarm' : 'border-escrow-border focus:border-escrow-border-hi'
        }`}
      />
      <span className={`text-[10px] ${invalid ? 'text-escrow-alarm' : 'text-escrow-dim'}`}>
        {invalid ? 'too many decimal places for this token' : note}
      </span>
    </label>
  )
}

// ── costs ────────────────────────────────────────────────────────────────────

function CostPanel({
  quote,
  quoting,
  connected,
  ready,
}: {
  quote: BuildResponse | null
  quoting: boolean
  connected: boolean
  ready: boolean
}) {
  if (!connected) {
    return (
      <div className="mx-4 mb-4 border border-escrow-border px-3 py-2 text-xs text-escrow-muted">
        Connect a wallet to see the exact cost and create a vault.
      </div>
    )
  }
  if (!ready) {
    return (
      <div className="mx-4 mb-4 border border-escrow-border px-3 py-2 text-xs text-escrow-dim">
        Pick both tokens and a backing amount above.
      </div>
    )
  }
  if (quoting && !quote) {
    return (
      <div className="mx-4 mb-4 border border-escrow-border px-3 py-2 text-xs text-escrow-dim">
        reading the chain…
      </div>
    )
  }
  if (!quote) return null

  if (!quote.ok) {
    const notDeployed = quote.reason === 'not_configured'
    return (
      <div
        className={`mx-4 mb-4 border px-3 py-2 text-xs ${
          notDeployed ? 'border-escrow-border' : 'border-escrow-alarm/50'
        }`}
      >
        <div className={notDeployed ? 'text-escrow-teal' : 'text-escrow-alarm'}>
          {notDeployed ? 'Vault program not deployed' : 'Cannot build this transaction'}
        </div>
        <p className="text-escrow-muted mt-1">{quote.detail}</p>
      </div>
    )
  }

  const c = quote.costs
  return (
    <div className="mx-4 mb-4 border border-escrow-border">
      <div className="px-3 py-2 border-b border-escrow-border flex items-baseline justify-between">
        <span className="text-[10px] text-escrow-dim uppercase tracking-wider">
          SOL cost {quote.needsVault ? '(new vault)' : '(vault exists)'}
        </span>
        <span className="text-escrow-teal-hi text-sm">
          {c ? `${fmtSol(c.totalLamports)} SOL` : '—'}
        </span>
      </div>
      <ul className="text-[11px] divide-y divide-escrow-border/40">
        {c?.lineItems.map(l => (
          <li key={l.label} className="px-3 py-1.5 flex justify-between gap-3">
            <span className="text-escrow-muted">{l.label}</span>
            <span className="text-escrow-text shrink-0">{fmtSol(l.lamports)}</span>
          </li>
        ))}
      </ul>

      {c?.estimated && (
        <p className="px-3 py-2 text-[10px] text-escrow-dim border-t border-escrow-border">
          Token-2022 mint: token accounts carrying extensions are larger than the 165-byte
          base, so this can under-read. Your wallet&rsquo;s preview is authoritative.
        </p>
      )}

      {(quote.shortfalls?.length ?? 0) > 0 && (
        <div className="border-t border-escrow-alarm/50 px-3 py-2">
          <div className="text-escrow-alarm text-[11px] uppercase tracking-wider">
            Insufficient balance
          </div>
          <ul className="text-[10px] text-escrow-muted mt-1 space-y-0.5">
            {quote.shortfalls?.map(s => <li key={s}>{s}</li>)}
          </ul>
        </div>
      )}

      {quote.vault && (
        <p className="px-3 py-2 text-[10px] text-escrow-dim border-t border-escrow-border break-all">
          vault PDA {quote.vault}
        </p>
      )}
    </div>
  )
}
