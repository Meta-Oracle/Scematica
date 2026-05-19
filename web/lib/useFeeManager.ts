'use client'

import { useEffect, useRef, useState } from 'react'
import { api } from './api'
import type { Trade } from './types'

const FEE_PCT = 0.01 // 1% of each confirmed sell amount

const LS_PAID = 'scema_fee_paid_sigs'

function tradeKey(t: Trade): string {
  return t.signature || `${t.mint}-${t.timestamp}`
}

function loadPaidSet(): Set<string> {
  try {
    const raw = localStorage.getItem(LS_PAID)
    return new Set(raw ? JSON.parse(raw) : [])
  } catch { return new Set() }
}

export interface FeeState {
  pendingLamports: number
  pendingCount: number
  totalOwedSol: number
}

export function useFeeManager(): FeeState {
  const [pendingLamports, setPendingLamports] = useState(0)
  const [pendingCount, setPendingCount]       = useState(0)
  const [totalOwedSol, setTotalOwedSol]       = useState(0)

  const pendingRef = useRef(0)

  useEffect(() => { pendingRef.current = pendingLamports }, [pendingLamports])

  useEffect(() => {
    let alive = true

    async function checkTrades() {
      const result = await api.trades(200)
      if (!alive || !result) return

      const paid = loadPaidSet()
      let added = 0
      const newSigs: string[] = []

      for (const t of result.trades) {
        if (t.kind !== 'SELL') continue
        if (t.status !== '✓') continue

        const key = tradeKey(t)
        if (paid.has(key)) continue

        const pnlPct = t.pnl_pct ?? 0
        if (pnlPct === 0) continue
        const solReceived = (t.pnl ?? 0) * (100 + pnlPct) / pnlPct
        if (solReceived <= 0) continue
        const feeLam = Math.floor(solReceived * FEE_PCT * 1_000_000_000)
        if (feeLam < 1_000) continue

        added += feeLam
        newSigs.push(key)
      }

      if (newSigs.length === 0) return

      const total = pendingRef.current + added
      pendingRef.current = total
      setPendingLamports(total)
      setPendingCount(c => c + newSigs.length)
      setTotalOwedSol(total / 1_000_000_000)
    }

    checkTrades()
    const iv = setInterval(checkTrades, 8_000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  return { pendingLamports, pendingCount, totalOwedSol }
}
