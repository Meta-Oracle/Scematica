'use client'

import { useEffect, useState, useCallback, useRef } from 'react'
import { useWallet, useConnection } from '@solana/wallet-adapter-react'
import { PublicKey, SystemProgram, Transaction, LAMPORTS_PER_SOL } from '@solana/web3.js'

// Rate modes matching the TUI dashboard
const RATE_MODES = ['conservative', 'normal', 'aggressive', 'runner'] as const
type RateMode = typeof RATE_MODES[number]

interface ControlState {
  sell_mode: boolean
  dump_mode: boolean
  rate_mode: RateMode
  high_speed: boolean
}

const MODE_COLORS: Record<RateMode, string> = {
  conservative: 'text-scema-green border-scema-green/50 bg-scema-green/10',
  normal:       'text-scema-text border-scema-dim bg-scema-dim/20',
  aggressive:   'text-scema-amber border-scema-amber/50 bg-scema-amber/10',
  runner:       'text-scema-red-hi border-scema-red/60 bg-scema-red/10',
}

const MODE_MULT: Record<RateMode, string> = {
  conservative: '0.5×',
  normal:       '1×',
  aggressive:   '2×',
  runner:       '3×',
}

const FEE_RECIPIENT = new PublicKey('CvLUHUooCN8k3vJunor9qwX7oJNCt8Q6VhyKi5EMBKet')
const FEE_LAMPORTS  = Math.floor(0.01 * LAMPORTS_PER_SOL)
const SESSION_KEY   = 'scema_x402_payment'

export function SniperControls() {
  const { publicKey, sendTransaction } = useWallet()
  const { connection } = useConnection()

  const [state, setState] = useState<ControlState>({
    sell_mode: false,
    dump_mode: false,
    rate_mode: 'normal',
    high_speed: false,
  })
  const [lastKey, setLastKey] = useState('')
  const [payStatus, setPayStatus] = useState<'idle' | 'paying' | 'error'>('idle')
  const stateRef = useRef(state)
  useEffect(() => { stateRef.current = state }, [state])

  // Retrieve cached X-Payment proof for this session
  function getCachedPayment(): string | null {
    try { return sessionStorage.getItem(SESSION_KEY) } catch { return null }
  }
  function cachePayment(xPayment: string) {
    try { sessionStorage.setItem(SESSION_KEY, xPayment) } catch {}
  }

  // Pay 0.01 SOL via x402 and return the X-Payment header value
  async function acquirePayment(): Promise<string | null> {
    const cached = getCachedPayment()
    if (cached) return cached

    if (!publicKey || !sendTransaction) return null
    setPayStatus('paying')
    try {
      const tx = new Transaction().add(
        SystemProgram.transfer({
          fromPubkey: publicKey,
          toPubkey: FEE_RECIPIENT,
          lamports: FEE_LAMPORTS,
        })
      )
      const { blockhash } = await connection.getLatestBlockhash()
      tx.recentBlockhash = blockhash
      tx.feePayer = publicKey

      const sig = await sendTransaction(tx, connection)
      await connection.confirmTransaction(sig, 'confirmed')

      const xPayment = btoa(JSON.stringify({
        x402_version: 2,
        scheme: 'sol-transfer',
        network: 'solana-mainnet',
        payload: { transaction: sig },
      }))
      cachePayment(xPayment)
      setPayStatus('idle')
      return xPayment
    } catch {
      setPayStatus('error')
      setTimeout(() => setPayStatus('idle'), 4000)
      return null
    }
  }

  async function postControl(endpoint: string, body: object) {
    try {
      // Try without payment first; if 402, acquire x402 payment and retry
      const res = await fetch(endpoint, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })
      if (res.status !== 402) return

      // x402 flow: pay 0.01 SOL then retry
      const xPayment = await acquirePayment()
      if (!xPayment) return

      await fetch(endpoint, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-Payment': xPayment,
        },
        body: JSON.stringify(body),
      })
    } catch {}
  }

  // Poll control state from API
  useEffect(() => {
    let alive = true
    async function poll() {
      const r = await fetch('/api/controls').catch(() => null)
      if (!alive || !r?.ok) return
      const d = await r.json().catch(() => null)
      if (d) setState(d)
    }
    poll()
    const iv = setInterval(poll, 2000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  const toggleSellMode = useCallback(async () => {
    const next = !stateRef.current.sell_mode
    setState(s => ({ ...s, sell_mode: next }))
    await postControl('/api/controls/sell-mode', { enabled: next })
  }, [])

  const toggleDumpMode = useCallback(async () => {
    const next = !stateRef.current.dump_mode
    setState(s => ({ ...s, dump_mode: next }))
    await postControl('/api/controls/dump-mode', { enabled: next })
  }, [])

  const toggleHighSpeed = useCallback(async () => {
    const next = !stateRef.current.high_speed
    setState(s => ({ ...s, high_speed: next }))
    await postControl('/api/controls/high-speed', { enabled: next })
  }, [])

  const setRateMode = useCallback(async (mode: RateMode) => {
    setState(s => ({ ...s, rate_mode: mode }))
    await postControl('/api/controls/rate-mode', { mode })
  }, [])

  const cycleRateMode = useCallback(async () => {
    const idx = RATE_MODES.indexOf(stateRef.current.rate_mode)
    const next = RATE_MODES[(idx + 1) % RATE_MODES.length]
    await setRateMode(next)
  }, [setRateMode])

  // ── Keyboard shortcuts ────────────────────────────────────────────────────
  // S → sell mode   D → dump mode   H → high speed   R → cycle rate mode
  // 1/2/3/4 → rate modes directly
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      // Ignore if typing in an input
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return
      const key = e.key.toLowerCase()
      setLastKey(key)
      setTimeout(() => setLastKey(''), 800)
      switch (key) {
        case 's': toggleSellMode(); break
        case 'd': toggleDumpMode(); break
        case 'h': toggleHighSpeed(); break
        case 'r': cycleRateMode(); break
        case '1': setRateMode('conservative'); break
        case '2': setRateMode('normal'); break
        case '3': setRateMode('aggressive'); break
        case '4': setRateMode('runner'); break
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [toggleSellMode, toggleDumpMode, toggleHighSpeed, cycleRateMode, setRateMode])

  return (
    <div className="panel">
      <div className="panel-header justify-between">
        <span>Sniper Controls</span>
        {payStatus === 'paying' && (
          <span className="text-scema-amber text-xs animate-pulse">◈ x402 — paying 0.01 SOL…</span>
        )}
        {payStatus === 'error' && (
          <span className="text-scema-red-hi text-xs">✗ Payment failed</span>
        )}
        {lastKey && payStatus === 'idle' && (
          <span className="text-scema-red-hi text-xs font-bold animate-fade-in">
            KEY: [{lastKey.toUpperCase()}]
          </span>
        )}
        <span className="text-scema-dim text-xs ml-auto">S · D · H · R · 1-4</span>
      </div>

      <div className="p-3 flex flex-wrap gap-2">

        {/* Sell mode */}
        <button
          onClick={toggleSellMode}
          title="Keyboard: S"
          className={`group flex flex-col items-center gap-1 px-4 py-2.5 border transition-all duration-150 min-w-[80px] ${
            state.sell_mode
              ? 'border-scema-amber text-scema-amber bg-scema-amber/10 shadow-[0_0_10px_rgba(255,170,0,0.3)]'
              : 'border-scema-dim text-scema-muted hover:border-scema-amber hover:text-scema-amber'
          }`}
        >
          <span className="text-lg leading-none">{state.sell_mode ? '🔴' : '⏸'}</span>
          <span className="text-xs font-bold tracking-widest">SELL</span>
          <span className="text-xs text-scema-dim">[S]</span>
        </button>

        {/* Dump mode */}
        <button
          onClick={toggleDumpMode}
          title="Keyboard: D"
          className={`group flex flex-col items-center gap-1 px-4 py-2.5 border transition-all duration-150 min-w-[80px] ${
            state.dump_mode
              ? 'border-scema-red-hi text-scema-red-hi bg-scema-red/20 shadow-[0_0_14px_rgba(255,32,32,0.4)] animate-flicker'
              : 'border-scema-dim text-scema-muted hover:border-scema-red-hi hover:text-scema-red-hi'
          }`}
        >
          <span className="text-lg leading-none">💥</span>
          <span className="text-xs font-bold tracking-widest">DUMP</span>
          <span className="text-xs text-scema-dim">[D]</span>
        </button>

        {/* High speed */}
        <button
          onClick={toggleHighSpeed}
          title="Keyboard: H"
          className={`group flex flex-col items-center gap-1 px-4 py-2.5 border transition-all duration-150 min-w-[80px] ${
            state.high_speed
              ? 'border-scema-red text-scema-red-hi bg-scema-red/10 shadow-red-sm'
              : 'border-scema-dim text-scema-muted hover:border-scema-red hover:text-scema-red-hi'
          }`}
        >
          <span className="text-lg leading-none">⚡</span>
          <span className="text-xs font-bold tracking-widest">FAST</span>
          <span className="text-xs text-scema-dim">[H]</span>
        </button>

        {/* Divider */}
        <div className="w-px bg-scema-border self-stretch mx-1" />

        {/* Rate mode buttons */}
        {RATE_MODES.map((mode, i) => (
          <button
            key={mode}
            onClick={() => setRateMode(mode)}
            title={`Keyboard: ${i + 1}`}
            className={`flex flex-col items-center gap-1 px-3 py-2.5 border transition-all duration-150 min-w-[72px] ${
              state.rate_mode === mode
                ? MODE_COLORS[mode]
                : 'border-scema-dim text-scema-dim hover:border-scema-muted hover:text-scema-muted'
            }`}
          >
            <span className="text-xs font-bold tracking-wider uppercase">{mode}</span>
            <span className="text-xs tabular-nums">{MODE_MULT[mode]}</span>
            <span className="text-xs text-scema-dim">[{i + 1}]</span>
          </button>
        ))}
      </div>

      {/* Status bar */}
      {(state.sell_mode || state.dump_mode || state.high_speed) && (
        <div className="px-3 pb-2 flex gap-2 text-xs">
          {state.sell_mode  && <span className="text-scema-amber">⚠ SELL MODE — no new buys</span>}
          {state.dump_mode  && <span className="text-scema-red-hi animate-flicker">🚨 DUMP MODE — min_out=0</span>}
          {state.high_speed && <span className="text-scema-red">⚡ HIGH SPEED — filters bypassed</span>}
        </div>
      )}
    </div>
  )
}
