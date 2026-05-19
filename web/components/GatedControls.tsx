'use client'

import { useEffect, useState } from 'react'
import { useWallet, useConnection } from '@solana/wallet-adapter-react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { PublicKey, SystemProgram, Transaction, LAMPORTS_PER_SOL } from '@solana/web3.js'

const FEE_RECIPIENT = new PublicKey('CvLUHUooCN8k3vJunor9qwX7oJNCt8Q6VhyKi5EMBKet')
const FEE_LAMPORTS  = Math.floor(0.01 * LAMPORTS_PER_SOL)

function sessionKey(pubkey: string) {
  return `scema_unlock_${pubkey}`
}

export function GatedControls({ children }: { children: React.ReactNode }) {
  const { connected, publicKey, sendTransaction } = useWallet()
  const { connection } = useConnection()
  const [paid, setPaid] = useState(false)
  const [status, setStatus] = useState<'idle' | 'sending' | 'confirmed' | 'error'>('idle')
  const [errMsg, setErrMsg] = useState('')

  // Check session storage on wallet change
  useEffect(() => {
    if (!publicKey) { setPaid(false); return }
    const stored = typeof sessionStorage !== 'undefined'
      ? sessionStorage.getItem(sessionKey(publicKey.toBase58()))
      : null
    setPaid(!!stored)
  }, [publicKey])

  async function payFee() {
    if (!publicKey || !sendTransaction) return
    setStatus('sending')
    setErrMsg('')
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

      sessionStorage.setItem(sessionKey(publicKey.toBase58()), sig)
      setStatus('confirmed')
      setTimeout(() => setPaid(true), 600)
    } catch (e: unknown) {
      setErrMsg(e instanceof Error ? e.message : 'Transaction failed')
      setStatus('error')
      setTimeout(() => setStatus('idle'), 6000)
    }
  }

  // ── Not connected ─────────────────────────────────────────────────────────
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
            Controls write directly to sniper IPC files — keyboard shortcuts active when unlocked
          </p>
        </div>
      </div>
    )
  }

  // ── Connected but fee not paid ─────────────────────────────────────────────
  if (!paid) {
    return (
      <div className="panel flex items-center justify-center py-6 px-4">
        <div className="flex flex-col items-center gap-4 text-center max-w-xs">
          <div className="flex items-center gap-2 text-xs tracking-widest uppercase text-scema-muted">
            <span className="w-1.5 h-1.5 rounded-full bg-scema-red-hi animate-pulse" />
            Controls locked
          </div>

          <div className="border border-scema-border p-4 flex flex-col gap-1 w-full">
            <div className="flex justify-between text-xs text-scema-dim">
              <span>Session unlock fee</span>
              <span className="text-scema-red-hi font-bold font-mono">0.01 SOL</span>
            </div>
            <div className="flex justify-between text-xs text-scema-dim">
              <span>Recipient</span>
              <span className="font-mono text-scema-muted">CvLU…Ket</span>
            </div>
            <div className="flex justify-between text-xs text-scema-dim">
              <span>Duration</span>
              <span className="text-scema-muted">browser session</span>
            </div>
          </div>

          <button
            onClick={payFee}
            disabled={status === 'sending' || status === 'confirmed'}
            className={`w-full px-6 py-2.5 text-xs font-bold uppercase tracking-widest border transition-all duration-150 ${
              status === 'sending'
                ? 'border-scema-dim text-scema-dim cursor-wait'
                : status === 'confirmed'
                ? 'border-scema-green text-scema-green bg-scema-green/10'
                : status === 'error'
                ? 'border-scema-red-hi text-scema-red-hi bg-scema-red/10'
                : 'border-scema-red text-scema-red-hi bg-scema-red/10 hover:bg-scema-red/20 shadow-red-sm'
            }`}
          >
            {status === 'sending'   ? '◈ SENDING…'         :
             status === 'confirmed' ? '✓ UNLOCKING…'       :
             status === 'error'     ? '✗ FAILED — RETRY'   :
                                      '⚡ PAY 0.01 SOL — UNLOCK'}
          </button>

          {status === 'error' && errMsg && (
            <p className="text-scema-red-hi text-xs truncate w-full text-center">{errMsg}</p>
          )}

          <p className="text-scema-dim text-xs leading-relaxed">
            One-time fee per session. Controls write IPC commands directly to the running sniper process.
          </p>
        </div>
      </div>
    )
  }

  // ── Paid — render controls ─────────────────────────────────────────────────
  return <>{children}</>
}
