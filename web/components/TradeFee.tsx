'use client'

import { useFeeManager } from '@/lib/useFeeManager'

export function TradeFee() {
  const { pendingLamports, pendingCount, totalOwedSol } = useFeeManager()

  if (pendingLamports === 0) return null

  return (
    <div className="flex items-center gap-2 text-xs text-scema-dim">
      <span className="w-1 h-1 rounded-full bg-scema-amber animate-pulse inline-block" />
      <span title={`${pendingCount} trade${pendingCount !== 1 ? 's' : ''} — 1% protocol fee`}>
        {totalOwedSol.toFixed(4)} SOL fee
      </span>
    </div>
  )
}
