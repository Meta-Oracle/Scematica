'use client'

import { useCallback, useEffect, useMemo, useState } from 'react'
import { useConnection, useWallet } from '@solana/wallet-adapter-react'
import { isNative } from '@/lib/net'
import { useDiscovery } from '@/lib/useDiscovery'
import {
  LAMPORTS_PER_SOL,
  SwapError,
  WSOL_MINT,
  executeSwap,
  fmtPriceImpact,
  getQuote,
  type Quote,
} from '@/lib/swap'

// Non-custodial manual execution.
//
// The route comes from Jupiter, the signature comes from the user's own wallet, and
// nothing in between ever holds a key. That makes it the only execution path that works
// with no backend at all — but a wallet prompt costs seconds, so it is explicitly NOT
// sniping and the UI says so rather than letting the framing imply otherwise.

const SLIPPAGE_PRESETS = [100, 300, 500, 1000] as const
const AMOUNT_PRESETS = [0.01, 0.05, 0.1, 0.25] as const

function shortMint(m: string) {
  return m ? `${m.slice(0, 4)}…${m.slice(-4)}` : ''
}

export function ManualSwap() {
  const { publicKey, connected, sendTransaction } = useWallet()
  const { connection } = useConnection()
  const discovery = useDiscovery()

  const [mint, setMint] = useState('')
  const [amountSol, setAmountSol] = useState(0.01)
  const [slippageBps, setSlippageBps] = useState<number>(300)
  const [quote, setQuote] = useState<Quote | null>(null)
  const [status, setStatus] = useState<'idle' | 'quoting' | 'swapping' | 'done' | 'error'>('idle')
  const [message, setMessage] = useState('')
  const [signature, setSignature] = useState('')

  // Candidates the discovery pipeline actually accepted, best score first.
  const candidates = useMemo(
    () => discovery.evaluations
      .filter(e => e.decision === 'accepted')
      .sort((a, b) => b.score - a.score)
      .slice(0, 12),
    [discovery.evaluations],
  )

  const selected = useMemo(
    () => discovery.evaluations.find(e => e.pool.mint === mint) ?? null,
    [discovery.evaluations, mint],
  )

  // A new mint or size invalidates the previous route.
  useEffect(() => { setQuote(null); setStatus('idle'); setMessage('') }, [mint, amountSol, slippageBps])

  const fetchQuote = useCallback(async () => {
    if (!mint) return
    setStatus('quoting'); setMessage('')
    try {
      const q = await getQuote({
        inputMint: WSOL_MINT,
        outputMint: mint,
        amount: Math.floor(amountSol * LAMPORTS_PER_SOL),
        slippageBps,
      })
      setQuote(q); setStatus('idle')
    } catch (err) {
      setQuote(null); setStatus('error')
      setMessage(err instanceof SwapError ? err.message : 'Could not fetch a quote')
    }
  }, [mint, amountSol, slippageBps])

  const doSwap = useCallback(async () => {
    if (!quote || !publicKey || !sendTransaction) return
    setStatus('swapping'); setMessage('')
    try {
      const res = await executeSwap(quote, publicKey, connection, sendTransaction)
      setSignature(res.signature)
      setStatus('done')
      setMessage('Swap confirmed')
    } catch (err) {
      setStatus('error')
      setMessage(err instanceof SwapError ? err.message : 'Swap failed')
    }
  }, [quote, publicKey, sendTransaction, connection])

  const outUi = quote && selected
    ? Number(quote.outAmount) / 10 ** selected.pool.decimals
    : null

  // The Capacitor shell connects wallets by deeplink but cannot sign — that path has no
  // signing bridge, so don't offer a button that can only fail.
  if (isNative()) {
    return (
      <div className="panel p-3 text-xs text-scema-dim">
        <span className="text-scema-amber font-bold tracking-widest">MANUAL SWAP</span>
        <p className="mt-1 leading-relaxed">
          Wallet signing isn&apos;t available inside the app shell. Use the dashboard in a
          browser, or pair your own instance to let the sniper execute.
        </p>
      </div>
    )
  }

  return (
    <div className="panel flex flex-col">
      <div className="panel-header justify-between">
        <span>Manual Swap</span>
        <span
          title="Your wallet signs each swap, so a human is in the loop for seconds. Real sniping needs a paired instance that signs locally."
          className="text-scema-amber border border-scema-amber/40 px-1.5 leading-tight text-xs"
        >
          NOT SNIPING
        </span>
      </div>

      <div className="px-3 py-2 text-[0.65rem] text-scema-dim leading-relaxed border-b border-scema-border">
        Routes from Jupiter, signed by your own wallet. No keys leave this device and no
        backend is involved — but the wallet prompt costs seconds, so treat this as
        manual execution on something the radar surfaced, not a snipe.
      </div>

      {!connected && (
        <div className="p-4 text-xs text-scema-dim text-center">
          Connect a wallet to quote and swap.
        </div>
      )}

      {connected && (
        <div className="flex flex-col gap-3 p-3">
          {/* Token */}
          <div className="flex flex-col gap-1">
            <span className="text-xs text-scema-dim tracking-wider">TOKEN</span>
            {candidates.length > 0 && (
              <div className="flex flex-wrap gap-1.5 mb-1">
                {candidates.map(c => (
                  <button
                    key={c.pool.mint}
                    onClick={() => setMint(c.pool.mint)}
                    title={`${c.pool.name} · score ${c.score.toFixed(1)} · ${c.pool.sizeSol.toFixed(1)} SOL`}
                    className={`px-2 py-0.5 text-xs border transition-colors ${
                      mint === c.pool.mint
                        ? 'border-scema-red/70 text-scema-red-hi bg-scema-red/10'
                        : 'border-scema-dim text-scema-dim hover:text-scema-muted'
                    }`}
                  >
                    {c.pool.symbol}
                  </button>
                ))}
              </div>
            )}
            <input
              value={mint}
              onChange={e => setMint(e.target.value.trim())}
              placeholder="or paste a mint address"
              spellCheck={false}
              autoCapitalize="none"
              autoCorrect="off"
              className="bg-scema-dim/20 border border-scema-border px-2 py-1.5 text-xs font-mono
                         text-scema-muted outline-none focus:border-scema-red/60"
            />
          </div>

          {/* Amount */}
          <div className="flex flex-col gap-1">
            <span className="text-xs text-scema-dim tracking-wider">AMOUNT (SOL)</span>
            <div className="flex flex-wrap gap-1.5">
              {AMOUNT_PRESETS.map(a => (
                <button
                  key={a}
                  onClick={() => setAmountSol(a)}
                  className={`px-2 py-0.5 text-xs border transition-colors ${
                    amountSol === a
                      ? 'border-scema-red/70 text-scema-red-hi bg-scema-red/10'
                      : 'border-scema-dim text-scema-dim hover:text-scema-muted'
                  }`}
                >
                  {a}
                </button>
              ))}
              <input
                type="number"
                min={0.001}
                step={0.001}
                value={amountSol}
                onChange={e => setAmountSol(Math.max(0.001, parseFloat(e.target.value) || 0.001))}
                className="w-24 bg-scema-dim/20 border border-scema-border px-2 py-0.5 text-xs
                           tabular-nums text-scema-muted outline-none focus:border-scema-red/60"
              />
            </div>
          </div>

          {/* Slippage */}
          <div className="flex flex-col gap-1">
            <span className="text-xs text-scema-dim tracking-wider">SLIPPAGE</span>
            <div className="flex flex-wrap gap-1.5">
              {SLIPPAGE_PRESETS.map(bps => (
                <button
                  key={bps}
                  onClick={() => setSlippageBps(bps)}
                  className={`px-2 py-0.5 text-xs border transition-colors ${
                    slippageBps === bps
                      ? 'border-scema-red/70 text-scema-red-hi bg-scema-red/10'
                      : 'border-scema-dim text-scema-dim hover:text-scema-muted'
                  }`}
                >
                  {bps / 100}%
                </button>
              ))}
            </div>
          </div>

          {/* Quote. A pasted mint isn't in the feed, so its decimals are unknown — show
              the raw base-unit amount rather than guessing a scale and misreporting it. */}
          {quote && (
            <div className="border border-scema-border px-2 py-1.5 text-xs flex flex-col gap-0.5">
              <div className="flex justify-between">
                <span className="text-scema-dim">YOU RECEIVE</span>
                <span className="text-scema-text tabular-nums font-bold">
                  {outUi !== null && selected
                    ? `${outUi.toLocaleString(undefined, { maximumFractionDigits: 4 })} ${selected.pool.symbol}`
                    : `${Number(quote.outAmount).toLocaleString()} base units`}
                </span>
              </div>
              <div className="flex justify-between">
                <span className="text-scema-dim">PRICE IMPACT</span>
                <span className="text-scema-muted tabular-nums">{fmtPriceImpact(quote)}</span>
              </div>
            </div>
          )}

          {/* Actions */}
          <div className="flex gap-2">
            <button
              onClick={fetchQuote}
              disabled={!mint || status === 'quoting' || status === 'swapping'}
              className="flex-1 py-1.5 text-xs font-bold tracking-widest border border-scema-border
                         text-scema-muted hover:border-scema-muted disabled:opacity-40 transition-colors"
            >
              {status === 'quoting' ? 'QUOTING…' : 'QUOTE'}
            </button>
            <button
              onClick={doSwap}
              disabled={!quote || status === 'swapping'}
              className="flex-1 py-1.5 text-xs font-bold tracking-widest border border-scema-red/70
                         text-scema-red-hi bg-scema-red/10 hover:bg-scema-red/20
                         disabled:opacity-40 transition-colors"
            >
              {status === 'swapping' ? 'SIGNING…' : 'SWAP'}
            </button>
          </div>

          {message && (
            <div className={`text-xs ${status === 'error' ? 'text-scema-red-hi' : 'text-scema-green'}`}>
              {message}
              {status === 'done' && signature && (
                <>
                  {' · '}
                  <a
                    href={`https://solscan.io/tx/${signature}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="underline hover:text-scema-text"
                  >
                    {shortMint(signature)}
                  </a>
                </>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
