'use client'

import { useWallet } from '@solana/wallet-adapter-react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { useScemaGate, SCEMA_REQUIRED } from '@/lib/ScemaGateContext'

function shortAddr(addr: string) {
  return `${addr.slice(0, 4)}…${addr.slice(-4)}`
}

function fmtScema(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`
  if (n >= 1_000)     return `${(n / 1_000).toFixed(1)}k`
  return n.toFixed(0)
}

export function WalletStatus() {
  const { publicKey, connected } = useWallet()
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

      <WalletMultiButton />
    </div>
  )
}
