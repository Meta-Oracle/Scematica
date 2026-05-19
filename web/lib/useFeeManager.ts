'use client'

import { useEffect, useRef, useState, useCallback } from 'react'
import { useWallet, useConnection } from '@solana/wallet-adapter-react'
import { PublicKey, SystemProgram, Transaction, LAMPORTS_PER_SOL } from '@solana/web3.js'
import { api } from './api'
import type { Trade } from './types'

const FEE_RECIPIENT  = new PublicKey('CvLUHUooCN8k3vJunor9qwX7oJNCt8Q6VhyKi5EMBKet')
const FEE_PCT        = 0.01                          // 1% of each confirmed sell amount
const BATCH_THRESHOLD = Math.floor(0.0005 * LAMPORTS_PER_SOL) // send when ≥ 0.0005 SOL (~6 confirmed sells)
const MIN_FEE        = 1_000                         // skip dust below 1000 lamports

const LS_PAID   = 'scema_fee_paid_sigs'
const LS_PENDING = 'scema_fee_pending'

function tradeKey(t: Trade): string {
  return t.signature || `${t.mint}-${t.timestamp}`
}

function loadPaidSet(): Set<string> {
  try {
    const raw = localStorage.getItem(LS_PAID)
    return new Set(raw ? JSON.parse(raw) : [])
  } catch { return new Set() }
}

function savePaidSet(s: Set<string>) {
  try {
    // Keep last 1000 entries to avoid unbounded growth
    const arr = Array.from(s).slice(-1000)
    localStorage.setItem(LS_PAID, JSON.stringify(arr))
  } catch {}
}

export interface FeeState {
  pendingLamports: number   // accumulated unpaid fee
  pendingCount: number      // number of trades not yet paid
  totalPaidSol: number      // session total paid
  status: 'idle' | 'paying' | 'paid' | 'error'
  payNow: () => Promise<void>
}

export function useFeeManager(): FeeState {
  const { publicKey, sendTransaction, connected } = useWallet()
  const { connection } = useConnection()

  const [pendingLamports, setPendingLamports] = useState(0)
  const [pendingCount, setPendingCount]       = useState(0)
  const [totalPaidSol, setTotalPaidSol]       = useState(0)
  const [status, setStatus] = useState<FeeState['status']>('idle')

  const pendingRef   = useRef(0)
  const pendingSigsRef = useRef<string[]>([])

  useEffect(() => { pendingRef.current = pendingLamports }, [pendingLamports])

  async function send(lamports: number, sigs: string[]) {
    if (!publicKey || !sendTransaction || lamports < MIN_FEE) return
    setStatus('paying')
    try {
      const tx = new Transaction().add(
        SystemProgram.transfer({
          fromPubkey: publicKey,
          toPubkey: FEE_RECIPIENT,
          lamports,
        })
      )
      const { blockhash } = await connection.getLatestBlockhash()
      tx.recentBlockhash = blockhash
      tx.feePayer = publicKey

      const txSig = await sendTransaction(tx, connection)
      await connection.confirmTransaction(txSig, 'confirmed')

      const paid = loadPaidSet()
      sigs.forEach(s => paid.add(s))
      savePaidSet(paid)

      pendingRef.current = 0
      pendingSigsRef.current = []
      setPendingLamports(0)
      setPendingCount(0)
      setTotalPaidSol(p => p + lamports / LAMPORTS_PER_SOL)
      setStatus('paid')
      setTimeout(() => setStatus('idle'), 3000)
    } catch {
      setStatus('error')
      setTimeout(() => setStatus('idle'), 5000)
    }
  }

  const payNow = useCallback(async () => {
    if (pendingRef.current >= MIN_FEE) {
      await send(pendingRef.current, pendingSigsRef.current)
    }
  }, [publicKey, sendTransaction]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!connected || !publicKey) {
      setPendingLamports(0)
      setPendingCount(0)
      pendingRef.current = 0
      pendingSigsRef.current = []
      return
    }

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
        if (pendingSigsRef.current.includes(key)) continue

        // SELL amount field is token count, not SOL. Derive actual SOL received:
        //   pnl_pct = (sol_received - entry) / entry * 100
        //   => sol_received = pnl * (100 + pnl_pct) / pnl_pct
        const pnlPct = t.pnl_pct ?? 0
        if (pnlPct === 0) continue
        const solReceived = (t.pnl ?? 0) * (100 + pnlPct) / pnlPct
        if (solReceived <= 0) continue
        const feeLam = Math.floor(solReceived * FEE_PCT * LAMPORTS_PER_SOL)
        if (feeLam < MIN_FEE) continue

        added += feeLam
        newSigs.push(key)
      }

      if (newSigs.length === 0) return

      const total = pendingRef.current + added
      pendingRef.current = total
      pendingSigsRef.current = [...pendingSigsRef.current, ...newSigs]
      setPendingLamports(total)
      setPendingCount(c => c + newSigs.length)

      // Auto-batch when threshold reached
      if (total >= BATCH_THRESHOLD) {
        await send(total, pendingSigsRef.current)
      }
    }

    checkTrades()
    const iv = setInterval(checkTrades, 8_000)
    return () => { alive = false; clearInterval(iv) }
  }, [connected, publicKey]) // eslint-disable-line react-hooks/exhaustive-deps

  return { pendingLamports, pendingCount, totalPaidSol, status, payNow }
}
