'use client'

import { useState } from 'react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { useScemaGate, SCEMA_REQUIRED } from '@/lib/ScemaGateContext'
import { useActiveWallet } from '@/lib/useActiveWallet'
import { useMobileWallet } from '@/lib/MobileWalletContext'
import { isNative } from '@/lib/net'
import { WALLET_LABELS, type WalletProvider } from '@/lib/mobileWallet'

function shortAddr(addr: string) {
  return `${addr.slice(0, 4)}…${addr.slice(-4)}`
}

function fmtScema(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`
  if (n >= 1_000)     return `${(n / 1_000).toFixed(1)}k`
  return n.toFixed(0)
}

// Native connect: opens Phantom/Solflare/Backpack via the deeplink protocol.
function MobileConnect() {
  const { connected, connecting, error, connect, disconnect } = useMobileWallet()
  const { publicKey } = useActiveWallet()
  const [open, setOpen] = useState(false)

  if (connected && publicKey) {
    return (
      <button
        onClick={disconnect}
        className="flex items-center gap-1.5 px-3 py-1.5 border border-scema-border text-xs font-mono text-scema-muted"
      >
        {shortAddr(publicKey.toBase58())}
        <span className="text-scema-dim">✕</span>
      </button>
    )
  }

  return (
    <div className="relative">
      <button
        onClick={() => setOpen(o => !o)}
        disabled={connecting}
        className="px-3 py-1.5 border border-scema-red/70 text-scema-red-hi bg-scema-red/10 text-xs font-bold tracking-wide disabled:opacity-50"
      >
        {connecting ? 'CONNECTING…' : 'CONNECT WALLET'}
      </button>
      {open && (
        <div className="absolute right-0 mt-1 z-50 min-w-[140px] bg-scema-black border border-scema-border shadow-lg">
          {(['phantom', 'solflare', 'backpack'] as WalletProvider[]).map(p => (
            <button
              key={p}
              onClick={() => { setOpen(false); connect(p) }}
              className="block w-full text-left px-3 py-2 text-xs text-scema-muted hover:bg-scema-dim/20"
            >
              {WALLET_LABELS[p]}
            </button>
          ))}
        </div>
      )}
      {error && (
        <div className="absolute right-0 mt-1 text-xs text-scema-red-hi max-w-[200px]">{error}</div>
      )}
    </div>
  )
}

export function WalletStatus() {
  const { publicKey, connected } = useActiveWallet()
  const { scemaBalance, solBalance, gated } = useScemaGate()

  return (
    <div className="flex items-center gap-3">
      {connected && publicKey ? (
        <>
          {/* Gate status badge */}
          <div className={`hidden sm:flex items-center gap-1.5 px-2 py-1 border text-xs ${
            gated === true
              ? 'border-scema-green/40 text-scema-green bg-scema-green/5'
              : gated === false
              ? 'border-scema-red/50 text-scema-red-hi bg-scema-red-bg'
              : 'border-scema-dim text-scema-muted'
          }`}>
            <span className={`w-1.5 h-1.5 rounded-full inline-block ${
              gated === true  ? 'bg-scema-green animate-pulse' :
              gated === false ? 'bg-scema-red-hi' : 'bg-scema-muted animate-pulse'
            }`}/>
            {gated === true ? 'GATED ✓' : gated === false ? 'NO SCEMA' : '…'}
          </div>

          {/* Balances */}
          <div className="hidden md:flex flex-col items-end gap-0 leading-tight">
            <span className="text-scema-muted text-xs font-mono">{shortAddr(publicKey.toBase58())}</span>
            <div className="flex gap-2 text-xs font-mono">
              {solBalance !== null && (
                <span className="text-scema-text tabular-nums">
                  {solBalance.toFixed(3)} <span className="text-scema-dim">SOL</span>
                </span>
              )}
              {scemaBalance !== null ? (
                <span className={`tabular-nums ${scemaBalance >= SCEMA_REQUIRED ? 'text-scema-green' : 'text-scema-red-hi'}`}>
                  {fmtScema(scemaBalance)} <span className="text-scema-dim">SCEMA</span>
                </span>
              ) : (
                <span className="text-scema-dim">… SCEMA</span>
              )}
            </div>
          </div>
        </>
      ) : null}

      {isNative() ? <MobileConnect /> : <WalletMultiButton />}
    </div>
  )
}
