'use client'

import { useWallet } from '@solana/wallet-adapter-react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { useScemaGate, SCEMA_REQUIRED } from '@/lib/ScemaGateContext'
import { BUY_URL } from '@/lib/scemaLinks'

export function GatedControls({ children }: { children: React.ReactNode }) {
  const { connected } = useWallet()
  const { scemaBalance, gated } = useScemaGate()

  // Not connected
  if (!connected) {
    return (
      <div className="panel flex items-center justify-center gap-4 py-5 px-4">
        <div className="flex flex-col items-center gap-3 text-center">
          <div className="flex items-center gap-2 text-scema-muted text-xs tracking-widest uppercase">
            <span className="w-1.5 h-1.5 rounded-full bg-scema-dim" />
            Wallet required to send control commands
          </div>
          <WalletMultiButton />
          <p className="text-scema-dim text-xs">
            Keyboard shortcuts active when connected (S · D · H · R · 1-4)
          </p>
        </div>
      </div>
    )
  }

  // Connected, checking SCEMA balance
  if (gated === null) {
    return (
      <div className="panel flex items-center justify-center py-5 px-4">
        <span className="text-scema-dim text-xs animate-pulse">◈ Verifying SCEMA balance…</span>
      </div>
    )
  }

  // Connected but insufficient SCEMA
  if (!gated) {
    const held   = scemaBalance ?? 0
    const needed = Math.max(0, SCEMA_REQUIRED - held)
    return (
      <div className="panel flex items-center justify-center gap-4 py-5 px-4">
        <div className="flex flex-col items-center gap-2 text-center">
          <div className="flex items-center gap-2 text-scema-red-hi text-xs tracking-widest uppercase">
            <span className="w-1.5 h-1.5 rounded-full bg-scema-red-hi animate-pulse" />
            Insufficient $SCEMA
          </div>
          <p className="text-scema-muted text-xs">
            You hold{' '}
            <span className="text-scema-text font-mono tabular-nums">
              {held.toLocaleString(undefined, { maximumFractionDigits: 0 })}
            </span>{' '}
            SCEMA — need{' '}
            <span className="text-scema-red-hi font-mono tabular-nums">
              {SCEMA_REQUIRED.toLocaleString()}
            </span>
          </p>
          <p className="text-scema-dim text-xs">
            Acquire{' '}
            <span className="font-mono tabular-nums">
              {needed.toLocaleString(undefined, { maximumFractionDigits: 0 })}
            </span>{' '}
            more $SCEMA to unlock controls
          </p>
          <a
            href={BUY_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="text-xs px-3 py-1 border border-scema-red/40 text-scema-red-hi
                       hover:bg-scema-red/10 transition-colors"
          >
            Buy $SCEMA on Jupiter ↗
          </a>
        </div>
      </div>
    )
  }

  // Gated ✓
  return <>{children}</>
}
