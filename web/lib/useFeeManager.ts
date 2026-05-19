'use client'

import { useEffect, useRef, useState, useCallback } from 'react'
import { useWallet, useConnection } from '@solana/wallet-adapter-react'
import { PublicKey, SystemProgram, Transaction, LAMPORTS_PER_SOL } from '@solana/web3.js'
import {
  TOKEN_2022_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  createTransferInstruction,
  createAssociatedTokenAccountIdempotentInstruction,
} from '@solana/spl-token'
import { api } from './api'
import type { Trade } from './types'

const FEE_RECIPIENT  = new PublicKey('CvLUHUooCN8k3vJunor9qwX7oJNCt8Q6VhyKi5EMBKet')
const SCEMA_MINT     = new PublicKey('AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump')
const SCEMA_DECIMALS = 6

const SOL_FEE_PCT         = 0.01                                    // 1% of SOL received
const SCEMA_FEE_PER_TRADE = 1_000 * 10 ** SCEMA_DECIMALS            // 1000 SCEMA per confirmed sell
const BATCH_THRESHOLD     = Math.floor(0.0005 * LAMPORTS_PER_SOL)   // auto-send at 0.0005 SOL
const MIN_SOL_FEE         = 1_000                                    // skip dust

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

function savePaidSet(s: Set<string>) {
  try {
    const arr = Array.from(s).slice(-1000)
    localStorage.setItem(LS_PAID, JSON.stringify(arr))
  } catch {}
}

export interface FeeState {
  pendingSolLamports: number
  pendingScemaRaw:    number
  pendingCount:       number
  totalPaidSol:       number
  status: 'idle' | 'paying' | 'paid' | 'error'
  payNow: () => Promise<void>
}

export function useFeeManager(): FeeState {
  const { publicKey, sendTransaction, connected } = useWallet()
  const { connection } = useConnection()

  const [pendingSolLamports, setPendingSolLamports] = useState(0)
  const [pendingScemaRaw, setPendingScemaRaw]       = useState(0)
  const [pendingCount, setPendingCount]             = useState(0)
  const [totalPaidSol, setTotalPaidSol]             = useState(0)
  const [status, setStatus] = useState<FeeState['status']>('idle')

  const solRef   = useRef(0)
  const scemaRef = useRef(0)
  const sigsRef  = useRef<string[]>([])

  useEffect(() => { solRef.current = pendingSolLamports }, [pendingSolLamports])
  useEffect(() => { scemaRef.current = pendingScemaRaw }, [pendingScemaRaw])

  async function send(solLamports: number, scemaRaw: number, sigs: string[]) {
    if (!publicKey || !sendTransaction) return
    if (solLamports < MIN_SOL_FEE && scemaRaw === 0) return
    setStatus('paying')
    try {
      const tx = new Transaction()

      // 1% SOL fee
      if (solLamports >= MIN_SOL_FEE) {
        tx.add(SystemProgram.transfer({
          fromPubkey: publicKey,
          toPubkey:   FEE_RECIPIENT,
          lamports:   solLamports,
        }))
      }

      // 1% SCEMA fee (Token-2022)
      if (scemaRaw > 0) {
        const senderAta    = getAssociatedTokenAddressSync(SCEMA_MINT, publicKey,     false, TOKEN_2022_PROGRAM_ID)
        const recipientAta = getAssociatedTokenAddressSync(SCEMA_MINT, FEE_RECIPIENT, false, TOKEN_2022_PROGRAM_ID)

        // Create recipient ATA if it doesn't exist yet (idempotent)
        tx.add(createAssociatedTokenAccountIdempotentInstruction(
          publicKey, recipientAta, FEE_RECIPIENT, SCEMA_MINT, TOKEN_2022_PROGRAM_ID
        ))

        tx.add(createTransferInstruction(
          senderAta, recipientAta, publicKey, BigInt(scemaRaw), [], TOKEN_2022_PROGRAM_ID
        ))
      }

      const { blockhash } = await connection.getLatestBlockhash()
      tx.recentBlockhash = blockhash
      tx.feePayer        = publicKey

      const txSig = await sendTransaction(tx, connection)
      await connection.confirmTransaction(txSig, 'confirmed')

      const paid = loadPaidSet()
      sigs.forEach(s => paid.add(s))
      savePaidSet(paid)

      solRef.current   = 0
      scemaRef.current = 0
      sigsRef.current  = []
      setPendingSolLamports(0)
      setPendingScemaRaw(0)
      setPendingCount(0)
      setTotalPaidSol(p => p + solLamports / LAMPORTS_PER_SOL)
      setStatus('paid')
      setTimeout(() => setStatus('idle'), 3000)
    } catch {
      setStatus('error')
      setTimeout(() => setStatus('idle'), 5000)
    }
  }

  const payNow = useCallback(async () => {
    if (solRef.current >= MIN_SOL_FEE || scemaRef.current > 0) {
      await send(solRef.current, scemaRef.current, sigsRef.current)
    }
  }, [publicKey, sendTransaction]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!connected || !publicKey) {
      setPendingSolLamports(0)
      setPendingScemaRaw(0)
      setPendingCount(0)
      solRef.current   = 0
      scemaRef.current = 0
      sigsRef.current  = []
      return
    }

    let alive = true

    async function checkTrades() {
      const result = await api.trades(200)
      if (!alive || !result) return

      const paid = loadPaidSet()
      let addedSol   = 0
      let addedScema = 0
      const newSigs: string[] = []

      for (const t of result.trades) {
        if (t.kind !== 'SELL') continue
        if (t.status !== '✓') continue

        const key = tradeKey(t)
        if (paid.has(key)) continue
        if (sigsRef.current.includes(key)) continue

        // Derive SOL received: pnl_pct = (sol_received - entry) / entry * 100
        const pnlPct = t.pnl_pct ?? 0
        if (pnlPct === 0) continue
        const solReceived = (t.pnl ?? 0) * (100 + pnlPct) / pnlPct
        if (solReceived <= 0) continue

        const feeLam = Math.floor(solReceived * SOL_FEE_PCT * LAMPORTS_PER_SOL)
        if (feeLam < MIN_SOL_FEE) continue

        addedSol   += feeLam
        addedScema += SCEMA_FEE_PER_TRADE
        newSigs.push(key)
      }

      if (newSigs.length === 0) return

      const totalSol   = solRef.current   + addedSol
      const totalScema = scemaRef.current + addedScema
      solRef.current   = totalSol
      scemaRef.current = totalScema
      sigsRef.current  = [...sigsRef.current, ...newSigs]
      setPendingSolLamports(totalSol)
      setPendingScemaRaw(totalScema)
      setPendingCount(c => c + newSigs.length)

      if (totalSol >= BATCH_THRESHOLD) {
        await send(totalSol, totalScema, sigsRef.current)
      }
    }

    checkTrades()
    const iv = setInterval(checkTrades, 8_000)
    return () => { alive = false; clearInterval(iv) }
  }, [connected, publicKey]) // eslint-disable-line react-hooks/exhaustive-deps

  return { pendingSolLamports, pendingScemaRaw, pendingCount, totalPaidSol, status, payNow }
}
