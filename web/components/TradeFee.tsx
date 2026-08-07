'use client'

import { useFeeManager } from '@/lib/useFeeManager'
import { useWallet } from '@solana/wallet-adapter-react'
import { LAMPORTS_PER_SOL } from '@solana/web3.js'

const SCEMA_DECIMALS = 6

export function TradeFee() {
  const { connected } = useWallet()
  const { pendingSolLamports, pendingScemaRaw, pendingCount, totalPaidSol, status, payNow } = useFeeManager()

  if (!connected) return null

  const pendingSol   = pendingSolLamports / LAMPORTS_PER_SOL
  const pendingScema = pendingScemaRaw / 10 ** SCEMA_DECIMALS

  return (
    <div className="flex items-center gap-2 text-xs">
      {totalPaidSol > 0 && status === 'idle' && (
        <span className="text-scema-dim hidden sm:inline">
          paid {totalPaidSol.toFixed(4)} SOL
        </span>
      )}

      {status === 'paying' && (
        <span className="text-scema-amber animate-pulse">◈ paying…</span>
      )}

      {status === 'paid' && (
        <span className="text-scema-green">✓ fee sent</span>
      )}

      {status === 'error' && (
        <span className="text-scema-red-hi">✗ fee failed</span>
      )}

      {(pendingSolLamports > 0 || pendingScemaRaw > 0) && status === 'idle' && (
        <button
          onClick={payNow}
          title={`${pendingCount} trade${pendingCount !== 1 ? 's' : ''} — click to pay now`}
          className="flex items-center gap-1 px-2 py-0.5 border border-scema-amber/60
                     text-scema-amber hover:bg-scema-amber/10 transition-colors"
        >
          <span className="w-1 h-1 rounded-full bg-scema-amber animate-pulse inline-block" />
          {pendingSol.toFixed(4)} SOL + {pendingScema.toFixed(0)} SCEMA
        </button>
      )}
    </div>
  )
}
