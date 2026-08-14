'use client'

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { Transaction } from '@solana/web3.js'

import {
  PROGRAM_LABEL,
  displaySymbol,
  looksLikeAddress,
  pairingProblem,
  toBaseUnits,
  type MintFacts,
  type MintResolution,
} from '@/lib/escrow/mintinfo'
import { useMintLookup } from '@/lib/escrow/useMint'
import { formatUnits, type MarketRow } from '@/lib/market/types'

// Create a vault: pick a token, pick what backs it, lock it.
//
// TWO THINGS THIS COMPONENT REFUSES TO GUESS.
//
// 1. **Decimals.** Human input becomes base units by shifting the decimal point
//    `decimals` places, so a wrong `decimals` is a wrong quantity of money, not a wrong
//    label. This used to read decimals off the market row (a third-party token list) and
//    fall back to a hardcoded 6 for anything the board had not listed — which for wBTC,
//    at 8 decimals, locks one hundredth of the intended reserve. Every mint now resolves
//    through /api/escrow/mint, the amount fields stay inert until that read lands, and
//    the number in the transaction comes from the mint account itself. See
//    lib/escrow/mintinfo.ts.
//
// 2. **What a token is.** Anything you can paste, you can vault. The picker searches the
//    live board, but an address that is not on it is not a dead end — it is resolved
//    against the chain and shown with what the chain says about it. A token minted ten
//    seconds ago has no listing anywhere, and those are precisely the ones this page is
//    about.
//
// The cost preview comes from /api/escrow/build, which also reads the chain: it knows
// whether the vault exists, whether your token accounts exist, and what rent each costs.
// When a figure is an estimate (Token-2022 extensions) it says so rather than presenting
// an exact-looking number.

const DAY = 24 * 60 * 60
const MIN_LOCK_DAYS = 7
const MAX_LOCK_DAYS = 10 * 365

const LOCKS = [
  { label: '7 days', days: 7, note: 'the program minimum' },
  { label: '30 days', days: 30, note: '' },
  { label: '90 days', days: 90, note: '' },
  { label: '1 year', days: 365, note: '' },
  { label: '4 years', days: 4 * 365, note: '' },
]

/** Reserve assets worth offering first. Every one is a wrapped claim — see DEPLOY.md.
 *  A starting point, not a menu: any mint can be pasted into either side. */
const BACKING_PRESETS = [
  { symbol: 'cbBTC', mint: 'cbbtcf3aa214zXHbiAZQwf4122FBYbraNdFqgw4iMij', note: 'Coinbase-issued BTC' },
  { symbol: 'wBTC', mint: '3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh', note: 'Wormhole-wrapped BTC' },
  { symbol: 'wETH', mint: '7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs', note: 'Wormhole-wrapped ETH' },
  { symbol: 'USDC', mint: 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', note: 'Circle USD' },
  { symbol: 'SOL', mint: 'So11111111111111111111111111111111111111112', note: 'wrapped SOL' },
]

/**
 * A chosen mint, before the chain has been asked about it.
 *
 * Deliberately carries no `decimals`. The symbol is a display hint from wherever the
 * choice came from; everything the builder computes with comes from the resolution.
 */
export interface Choice {
  mint: string
  symbol: string | null
  from: 'board' | 'preset' | 'pasted'
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

function fmtSol(lamports: number): string {
  return (lamports / 1e9).toFixed(6).replace(/0+$/, '').replace(/\.$/, '')
}

const SOLSCAN = (mint: string) => `https://solscan.io/token/${mint}`

export interface VaultBuilderProps {
  rows: MarketRow[]
  /** Controlled from the page so a click on any market row lands here. */
  token: Choice | null
  backing: Choice | null
  onToken: (c: Choice | null) => void
  onBacking: (c: Choice | null) => void
  /** Fired after a confirmed deposit so the board can re-read custody. */
  onCreated?: () => void
}

export function VaultBuilder({ rows, token, backing, onToken, onBacking, onCreated }: VaultBuilderProps) {
  const { publicKey, signTransaction, connected } = useWallet()

  const [tokenAmt, setTokenAmt] = useState('')
  const [backingAmt, setBackingAmt] = useState('')
  const [lockDays, setLockDays] = useState(MIN_LOCK_DAYS)
  const [customDays, setCustomDays] = useState('')

  const [quote, setQuote] = useState<BuildResponse | null>(null)
  const [quoting, setQuoting] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [result, setResult] = useState<{ ok: boolean; text: string; signature?: string } | null>(null)

  // Chain truth for both legs. Everything below computes from these, never from the row.
  const tokenLookup = useMintLookup(token?.mint ?? null, 0)
  const backingLookup = useMintLookup(backing?.mint ?? null, 0)
  const tokenFacts = tokenLookup.result?.ok ? tokenLookup.result.facts : null
  const backingFacts = backingLookup.result?.ok ? backingLookup.result.facts : null

  const tokenBase = tokenFacts ? toBaseUnits(tokenAmt, tokenFacts.decimals) : null
  const backingBase = backingFacts ? toBaseUnits(backingAmt, backingFacts.decimals) : null

  const pairing = tokenFacts && backingFacts ? pairingProblem(tokenFacts, backingFacts) : null

  const lockSecs = lockDays * DAY
  const lockValid = lockDays >= MIN_LOCK_DAYS && lockDays <= MAX_LOCK_DAYS

  const ready = Boolean(
    tokenFacts &&
      backingFacts &&
      !pairing &&
      lockValid &&
      tokenBase !== null &&
      backingBase !== null &&
      backingBase > 0n &&
      publicKey,
  )

  // Quote whenever the inputs settle. Debounced — every keystroke would be a chain read.
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
        body: JSON.stringify({ transaction: signed.serialize().toString('base64') }),
      })
      const json = (await res.json()) as { ok: boolean; signature?: string; detail?: string }
      if (json.ok) {
        setResult({ ok: true, text: 'Confirmed on chain', signature: json.signature })
        // The amounts are spent; leaving them filled invites a second identical deposit
        // from a stale quote. The token pair stays, since backing the same token again is
        // the common next action.
        setTokenAmt('')
        setBackingAmt('')
        setQuote(null)
        onCreated?.()
      } else {
        setResult({ ok: false, text: json.detail ?? 'send failed' })
      }
    } catch (e) {
      setResult({ ok: false, text: e instanceof Error ? e.message : String(e) })
    } finally {
      setSubmitting(false)
    }
  }, [quote, signTransaction, onCreated])

  // The single "what is stopping this" line, in the order the user must fix things.
  // `alarm` separates "you have not finished filling this in" from "what you filled in
  // is wrong" — one is guidance, the other is a refusal, and toning them alike trains
  // people to skim past both.
  const blocked = useMemo<{ text: string; alarm: boolean } | null>(() => {
    const say = (text: string, alarm = false) => ({ text, alarm })
    if (!token || !backing)
      return say('Pick a token and a reserve above — search the board or paste any mint address.')
    if (tokenLookup.loading || backingLookup.loading) return say('Reading both mints from the chain…')
    if (tokenLookup.result && !tokenLookup.result.ok)
      return say(`Token mint: ${tokenLookup.result.detail}`, true)
    if (backingLookup.result && !backingLookup.result.ok)
      return say(`Reserve mint: ${backingLookup.result.detail}`, true)
    if (pairing) return say(pairing, true)
    if (!lockValid) return say(`Lock must be between ${MIN_LOCK_DAYS} and ${MAX_LOCK_DAYS} days.`, true)
    if (backingBase === null) return say('Reserve amount is not a valid number for this mint.', true)
    if (backingBase === 0n)
      return say('A vault with no reserve behind it is not a vault — enter a reserve amount.')
    if (tokenBase === null) return say('Token amount is not a valid number for this mint.', true)
    return null
  }, [token, backing, tokenLookup, backingLookup, pairing, lockValid, backingBase, tokenBase])

  return (
    <section className="border border-escrow-border bg-escrow-surface">
      <div className="px-4 py-3 border-b border-escrow-border flex items-center justify-between gap-4 flex-wrap">
        <div>
          <h2 className="text-sm text-escrow-teal uppercase tracking-wider">Create a vault</h2>
          <p className="text-[11px] text-escrow-dim mt-0.5">
            Lock a reserve behind any SPL mint. Non-custodial — only you can withdraw, and
            only after the lock elapses.
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
          lookup={tokenLookup}
          onSelect={onToken}
          exclude={backing?.mint}
        />
        <TokenPicker
          label="Backing it with"
          hint="the reserve asset locked behind it"
          rows={rows}
          selected={backing}
          lookup={backingLookup}
          onSelect={onBacking}
          exclude={token?.mint}
          presets={BACKING_PRESETS}
        />
      </div>

      <div className="grid gap-4 sm:grid-cols-2 px-4 pb-4">
        <AmountField
          label={`Token amount${tokenFacts ? ` (${displaySymbol(tokenFacts.mint, tokenLookup.result?.ok ? tokenLookup.result.label : null)})` : ''}`}
          value={tokenAmt}
          onChange={setTokenAmt}
          facts={tokenFacts}
          pending={tokenLookup.loading}
          chosen={Boolean(token)}
          invalid={tokenAmt !== '' && tokenBase === null}
          note="optional — a pure-reserve deposit is allowed"
        />
        <AmountField
          label={`Reserve amount${backingFacts ? ` (${displaySymbol(backingFacts.mint, backingLookup.result?.ok ? backingLookup.result.label : null)})` : ''}`}
          value={backingAmt}
          onChange={setBackingAmt}
          facts={backingFacts}
          pending={backingLookup.loading}
          chosen={Boolean(backing)}
          invalid={backingAmt !== '' && backingBase === null}
          note="required — must be greater than zero"
        />
      </div>

      <LockPicker
        days={lockDays}
        onDays={setLockDays}
        customDays={customDays}
        onCustomDays={setCustomDays}
      />

      <CostPanel
        quote={quote}
        quoting={quoting}
        connected={connected}
        blocked={blocked}
        tokenBase={tokenBase}
        backingBase={backingBase}
        tokenFacts={tokenFacts}
        backingFacts={backingFacts}
      />

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
            {result.signature && (
              <>
                {' — '}
                <a
                  href={`https://solscan.io/tx/${result.signature}`}
                  target="_blank"
                  rel="noreferrer"
                  className="underline hover:text-escrow-teal-hi"
                >
                  {result.signature.slice(0, 16)}…
                </a>
              </>
            )}
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
  lookup,
  onSelect,
  exclude,
  presets,
}: {
  label: string
  hint: string
  rows: MarketRow[]
  selected: Choice | null
  lookup: { loading: boolean; result: MintResolution | null }
  onSelect: (s: Choice | null) => void
  exclude?: string
  presets?: { symbol: string; mint: string; note: string }[]
}) {
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState(false)
  const boxRef = useRef<HTMLDivElement>(null)

  const trimmed = query.trim()
  const pasted = looksLikeAddress(trimmed) ? trimmed : null
  // Preview a pasted address before it is committed. Same module-level cache the builder
  // reads from, so selecting it afterwards costs nothing further.
  const preview = useMintLookup(pasted)

  const matches = useMemo(() => {
    const q = trimmed.toLowerCase()
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
  }, [rows, trimmed, exclude])

  // Close the dropdown on an outside click. Without this the two pickers can both sit
  // open with their lists overlapping the fields below them.
  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      if (boxRef.current && !boxRef.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', onDown)
    return () => document.removeEventListener('mousedown', onDown)
  }, [open])

  const choose = (c: Choice) => {
    onSelect(c)
    setOpen(false)
    setQuery('')
  }

  if (selected) {
    return (
      <div>
        <div className="text-[10px] text-escrow-dim uppercase tracking-wider">{label}</div>
        <div className="text-[10px] text-escrow-dim mb-1">{hint}</div>
        <SelectedCard selected={selected} lookup={lookup} onClear={() => onSelect(null)} />
      </div>
    )
  }

  const excluded = pasted !== null && pasted === exclude

  return (
    <div ref={boxRef}>
      <div className="text-[10px] text-escrow-dim uppercase tracking-wider">{label}</div>
      <div className="text-[10px] text-escrow-dim mb-1">{hint}</div>

      <input
        value={query}
        onChange={e => {
          setQuery(e.target.value)
          setOpen(true)
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={e => {
          if (e.key === 'Escape') setOpen(false)
          if (e.key === 'Enter' && preview.result?.ok && !excluded) {
            choose({
              mint: preview.result.facts.mint,
              symbol: preview.result.label.symbol,
              from: 'pasted',
            })
          }
        }}
        spellCheck={false}
        placeholder="search the board, or paste any mint address"
        className="w-full bg-escrow-black border border-escrow-border focus:border-escrow-border-hi outline-none px-3 py-2 text-sm text-escrow-text placeholder:text-escrow-dim"
      />

      {/* A pasted address is resolved against the chain and previewed before it is
          committed. The board is a convenience; the program takes any mint. */}
      {pasted && (
        <PastePreview
          mint={pasted}
          lookup={preview}
          excluded={excluded}
          onUse={facts =>
            choose({
              mint: facts.mint,
              symbol: preview.result?.ok ? preview.result.label.symbol : null,
              from: 'pasted',
            })
          }
        />
      )}

      {!pasted && trimmed.length > 0 && trimmed.length < 32 && matches.length === 0 && (
        <p className="mt-1 text-[10px] text-escrow-dim">
          Nothing on the board matches. Paste the full mint address to use a token the
          board has not listed — every SPL mint is eligible.
        </p>
      )}

      {presets && !trimmed && (
        <div className="flex gap-1 flex-wrap mt-2">
          {presets.map(p => (
            <button
              key={p.mint}
              title={p.note}
              onClick={() => choose({ mint: p.mint, symbol: p.symbol, from: 'preset' })}
              className="px-2 py-1 text-[11px] border border-escrow-border text-escrow-muted hover:text-escrow-teal"
            >
              {p.symbol}
            </button>
          ))}
        </div>
      )}

      {open && !pasted && matches.length > 0 && (
        <ul className="mt-1 max-h-56 overflow-y-auto border border-escrow-border divide-y divide-escrow-border/50">
          {matches.map(r => (
            <li key={r.token.mint}>
              <button
                onClick={() =>
                  choose({ mint: r.token.mint, symbol: r.token.symbol, from: 'board' })
                }
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
    </div>
  )
}

/** What the chain says about a pasted address, before it is committed to. */
function PastePreview({
  mint,
  lookup,
  excluded,
  onUse,
}: {
  mint: string
  lookup: { loading: boolean; result: MintResolution | null }
  excluded: boolean
  onUse: (facts: MintFacts) => void
}) {
  if (excluded) {
    return (
      <div className="mt-1 border border-escrow-alarm/50 px-3 py-2 text-[11px] text-escrow-alarm">
        That mint is already selected on the other side. A token cannot back itself.
      </div>
    )
  }
  if (lookup.loading || !lookup.result) {
    return (
      <div className="mt-1 border border-escrow-border px-3 py-2 text-[11px] text-escrow-dim">
        reading {mint.slice(0, 8)}… from the chain
      </div>
    )
  }
  if (!lookup.result.ok) {
    return (
      <div className="mt-1 border border-escrow-alarm/50 px-3 py-2 text-[11px]">
        <div className="text-escrow-alarm uppercase tracking-wider">
          {lookup.result.reason.replace(/_/g, ' ')}
        </div>
        <p className="text-escrow-muted mt-1">{lookup.result.detail}</p>
      </div>
    )
  }

  const { facts, label } = lookup.result
  return (
    <button
      onClick={() => onUse(facts)}
      className="mt-1 w-full text-left border border-escrow-border-hi bg-escrow-black px-3 py-2 hover:bg-escrow-hi/40"
    >
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-escrow-teal-hi text-sm">{displaySymbol(facts.mint, label)}</span>
        <span className="text-[10px] text-escrow-teal uppercase tracking-wider">use this mint</span>
      </div>
      <MintFactLine facts={facts} label={label} />
    </button>
  )
}

function SelectedCard({
  selected,
  lookup,
  onClear,
}: {
  selected: Choice
  lookup: { loading: boolean; result: MintResolution | null }
  onClear: () => void
}) {
  const [copied, setCopied] = useState(false)
  const facts = lookup.result?.ok ? lookup.result.facts : null
  const label = lookup.result?.ok ? lookup.result.label : null

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(selected.mint)
      setCopied(true)
      setTimeout(() => setCopied(false), 1200)
    } catch {
      /* clipboard denied — the address is on screen and selectable regardless */
    }
  }

  return (
    <div
      className={`border bg-escrow-black px-3 py-2 ${
        lookup.result && !lookup.result.ok ? 'border-escrow-alarm/60' : 'border-escrow-border-hi'
      }`}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="text-escrow-teal-hi text-sm">
            {facts ? displaySymbol(facts.mint, label) : selected.symbol || 'resolving…'}
          </div>
          <div className="text-[10px] text-escrow-dim truncate">{selected.mint}</div>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <button onClick={copy} className="text-[10px] text-escrow-muted hover:text-escrow-text">
            {copied ? 'COPIED' : 'COPY'}
          </button>
          <a
            href={SOLSCAN(selected.mint)}
            target="_blank"
            rel="noreferrer"
            className="text-[10px] text-escrow-muted hover:text-escrow-text"
          >
            EXPLORER
          </a>
          <button onClick={onClear} className="text-[10px] text-escrow-muted hover:text-escrow-text">
            CHANGE
          </button>
        </div>
      </div>

      {lookup.loading && (
        <div className="text-[10px] text-escrow-dim mt-1">reading the mint from the chain…</div>
      )}
      {facts && <MintFactLine facts={facts} label={label} />}
      {lookup.result && !lookup.result.ok && (
        <div className="text-[10px] text-escrow-alarm mt-1">{lookup.result.detail}</div>
      )}
    </div>
  )
}

/**
 * The chain's own description of a mint.
 *
 * `decimals` is here because it is the number that converts the amount typed above into
 * the amount actually transferred; showing it makes a mismatch with the user's
 * expectation visible before signing rather than after. The authority fields are the two
 * facts that decide whether locking a reserve behind this token means anything: a live
 * mint authority can print supply against the reserve at will.
 */
function MintFactLine({ facts, label }: { facts: MintFacts; label?: { name: string | null; source: string | null } | null }) {
  return (
    <div className="mt-1 flex gap-x-3 gap-y-0.5 flex-wrap text-[10px]">
      <span className="text-escrow-muted">
        <span className="text-escrow-dim">decimals </span>
        {facts.decimals}
      </span>
      <span className="text-escrow-muted">
        <span className="text-escrow-dim">supply </span>
        {formatUnits(facts.supply, facts.decimals)}
      </span>
      <span className="text-escrow-muted">
        <span className="text-escrow-dim">program </span>
        {PROGRAM_LABEL[facts.program]}
      </span>
      <span className={facts.mintAuthority ? 'text-escrow-alarm' : 'text-escrow-teal'}>
        mint auth {facts.mintAuthority ? 'LIVE' : 'revoked'}
      </span>
      <span className={facts.freezeAuthority ? 'text-escrow-alarm' : 'text-escrow-teal'}>
        freeze {facts.freezeAuthority ? 'LIVE' : 'revoked'}
      </span>
      <span className="text-escrow-dim">slot {facts.slot}</span>
      {label?.source && label.name && (
        <span className="text-escrow-dim truncate max-w-[220px]">
          {label.name} · per {label.source}
        </span>
      )}
      {!label?.source && (
        <span className="text-escrow-dim">unlisted — no token list names this mint</span>
      )}
    </div>
  )
}

function AmountField({
  label,
  value,
  onChange,
  facts,
  pending,
  chosen,
  invalid,
  note,
}: {
  label: string
  value: string
  onChange: (v: string) => void
  facts: MintFacts | null
  pending: boolean
  chosen: boolean
  invalid: boolean
  note: string
}) {
  // Disabled until the chain answers. An amount typed against unknown decimals cannot be
  // converted to base units without guessing, and guessing here moves the wrong quantity.
  const disabled = !facts
  const placeholder = !chosen
    ? 'select a token first'
    : pending
      ? 'reading decimals…'
      : disabled
        ? 'mint unresolved'
        : '0.0'

  return (
    <label className="block">
      <span className="text-[10px] text-escrow-dim uppercase tracking-wider">{label}</span>
      <input
        value={value}
        onChange={e => onChange(e.target.value)}
        disabled={disabled}
        inputMode="decimal"
        placeholder={placeholder}
        className={`w-full mt-1 bg-escrow-black border outline-none px-3 py-2 text-sm text-escrow-text placeholder:text-escrow-dim disabled:opacity-40 ${
          invalid ? 'border-escrow-alarm' : 'border-escrow-border focus:border-escrow-border-hi'
        }`}
      />
      <span className={`text-[10px] ${invalid ? 'text-escrow-alarm' : 'text-escrow-dim'}`}>
        {invalid
          ? `this mint has ${facts?.decimals} decimals — that is more precision than it can hold`
          : note}
      </span>
    </label>
  )
}

function LockPicker({
  days,
  onDays,
  customDays,
  onCustomDays,
}: {
  days: number
  onDays: (d: number) => void
  customDays: string
  onCustomDays: (v: string) => void
}) {
  const isPreset = LOCKS.some(l => l.days === days)
  const customInvalid =
    customDays !== '' &&
    (!/^\d+$/.test(customDays) ||
      Number(customDays) < MIN_LOCK_DAYS ||
      Number(customDays) > MAX_LOCK_DAYS)

  return (
    <div className="px-4 pb-4">
      <div className="text-[10px] text-escrow-dim uppercase tracking-wider mb-2">Lock duration</div>
      <div className="flex gap-1 flex-wrap items-center">
        {LOCKS.map(l => (
          <button
            key={l.days}
            onClick={() => {
              onDays(l.days)
              onCustomDays('')
            }}
            title={l.note}
            className={`px-3 py-1.5 text-xs border ${
              days === l.days && isPreset
                ? 'border-escrow-border-hi text-escrow-teal bg-escrow-hi'
                : 'border-escrow-border text-escrow-muted hover:text-escrow-text'
            }`}
          >
            {l.label}
          </button>
        ))}
        <div className="flex items-center gap-1">
          <input
            value={customDays}
            onChange={e => {
              const v = e.target.value
              onCustomDays(v)
              if (/^\d+$/.test(v)) {
                const n = Number(v)
                if (n >= MIN_LOCK_DAYS && n <= MAX_LOCK_DAYS) onDays(n)
              }
            }}
            inputMode="numeric"
            placeholder="custom"
            className={`w-24 bg-escrow-black border outline-none px-2 py-1.5 text-xs text-escrow-text placeholder:text-escrow-dim ${
              customInvalid ? 'border-escrow-alarm' : 'border-escrow-border focus:border-escrow-border-hi'
            }`}
          />
          <span className="text-[10px] text-escrow-dim">days</span>
        </div>
      </div>

      <p className="text-[10px] text-escrow-dim mt-2">
        {customInvalid
          ? `The program accepts ${MIN_LOCK_DAYS} to ${MAX_LOCK_DAYS} days — anything outside that range is rejected as LockOutOfRange.`
          : `Locking for ${days} day${days === 1 ? '' : 's'}. Withdrawal is impossible until it elapses — including for you. The ${MIN_LOCK_DAYS}-day floor exists so “backed” cannot mean a position opened and closed inside a block for a screenshot.`}
      </p>
    </div>
  )
}

// ── costs ────────────────────────────────────────────────────────────────────

function CostPanel({
  quote,
  quoting,
  connected,
  blocked,
  tokenBase,
  backingBase,
  tokenFacts,
  backingFacts,
}: {
  quote: BuildResponse | null
  quoting: boolean
  connected: boolean
  blocked: { text: string; alarm: boolean } | null
  tokenBase: bigint | null
  backingBase: bigint | null
  tokenFacts: MintFacts | null
  backingFacts: MintFacts | null
}) {
  // Blockers come before the wallet prompt: what you have chosen is wrong is worth
  // knowing whether or not a wallet is attached, and the picker is usable disconnected.
  if (blocked) {
    return (
      <div
        className={`mx-4 mb-4 border px-3 py-2 text-xs ${
          blocked.alarm
            ? 'border-escrow-alarm/50 text-escrow-alarm'
            : 'border-escrow-border text-escrow-dim'
        }`}
      >
        {blocked.text}
      </div>
    )
  }
  if (!connected) {
    return (
      <div className="mx-4 mb-4 border border-escrow-border px-3 py-2 text-xs text-escrow-muted">
        Connect a wallet to see the exact cost and create a vault. Reading the board needs
        nothing.
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
  // The exact quantities the instruction carries, rendered back through the SAME
  // decimals used to build them. If this line does not read as what was typed, the
  // conversion is wrong and the transaction should not be signed.
  const confirming =
    tokenFacts && backingFacts && tokenBase !== null && backingBase !== null
      ? `${formatUnits(backingBase.toString(), backingFacts.decimals)} reserve @ ${backingFacts.decimals} dp` +
        (tokenBase > 0n
          ? ` · ${formatUnits(tokenBase.toString(), tokenFacts.decimals)} token @ ${tokenFacts.decimals} dp`
          : ' · no token side')
      : null

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

      {confirming && (
        <div className="px-3 py-1.5 border-b border-escrow-border/40 text-[10px] text-escrow-muted">
          depositing {confirming}
        </div>
      )}

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
