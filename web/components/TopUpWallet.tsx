'use client'

import { useState } from 'react'
import { useWallet, useConnection } from '@solana/wallet-adapter-react'
import { PublicKey, SystemProgram, Transaction, LAMPORTS_PER_SOL } from '@solana/web3.js'

// Bot wallet address (from config.toml)
const BOT_WALLET = new PublicKey('CvLUHUooCN8k3vJunor9qwX7oJNCt8Q6VhyKi5EMBKet')

const PRESETS = [0.05, 0.1, 0.25, 0.5]

export function TopUpWallet() {
  const { publicKey, sendTransaction, connected } = useWallet()
  const { connection } = useConnection()
  const [amount, setAmount] = useState('')
  const [status, setStatus] = useState<'idle' | 'sending' | 'confirmed' | 'error'>('idle')
  const [sig, setSig] = useState('')
  const [errMsg, setErrMsg] = useState('')

  async function send() {
    if (!publicKey || !connected) return
    const sol = parseFloat(amount)
    if (isNaN(sol) || sol <= 0) return

    setStatus('sending')
    setSig('')
    setErrMsg('')

    try {
      const lamports = Math.floor(sol * LAMPORTS_PER_SOL)
      const tx = new Transaction().add(
        SystemProgram.transfer({
          fromPubkey: publicKey,
          toPubkey: BOT_WALLET,
          lamports,
        })
      )
      const { blockhash } = await connection.getLatestBlockhash()
      tx.recentBlockhash = blockhash
      tx.feePayer = publicKey

      const signature = await sendTransaction(tx, connection)
      setSig(signature)

      await connection.confirmTransaction(signature, 'confirmed')
      setStatus('confirmed')
      setAmount('')
      setTimeout(() => setStatus('idle'), 4000)
    } catch (e: unknown) {
      setErrMsg(e instanceof Error ? e.message : 'Transaction failed')
      setStatus('error')
      setTimeout(() => setStatus('idle'), 5000)
    }
  }

  if (!connected) {
    return (
      <div className="panel flex items-center justify-center h-full text-scema-dim text-xs">
        Connect wallet to top up
      </div>
    )
  }

  return (
    <div className="panel h-full">
      <div className="panel-header">Top Up Bot Wallet</div>
      <div className="p-3 flex flex-col gap-2">
        <p className="text-scema-dim text-xs">
          Send SOL from your Phantom wallet to the sniper bot.
        </p>
        <p className="text-scema-muted text-xs font-mono truncate">
          → {BOT_WALLET.toBase58().slice(0, 8)}…{BOT_WALLET.toBase58().slice(-8)}
        </p>

        {/* Presets */}
        <div className="flex gap-1.5">
          {PRESETS.map(p => (
            <button
              key={p}
              onClick={() => setAmount(String(p))}
              className={`text-xs px-2 py-1 border transition-colors ${
                amount === String(p)
                  ? 'border-scema-red text-scema-red-hi bg-scema-red/10'
                  : 'border-scema-dim text-scema-muted hover:border-scema-red-dim hover:text-scema-text'
              }`}
            >
              {p} SOL
            </button>
          ))}
        </div>

        {/* Custom amount */}
        <div className="flex gap-2">
          <input
            type="number"
            min="0.001"
            step="0.01"
            placeholder="Amount SOL"
            value={amount}
            onChange={e => setAmount(e.target.value)}
            className="flex-1 bg-transparent border border-scema-dim text-scema-text text-xs px-3 py-1.5
                       focus:outline-none focus:border-scema-red-dim placeholder:text-scema-dim font-mono"
          />
          <button
            onClick={send}
            disabled={status === 'sending' || !amount}
            className={`px-4 py-1.5 text-xs font-bold uppercase tracking-widest border transition-all ${
              status === 'sending'
                ? 'border-scema-dim text-scema-dim cursor-wait'
                : status === 'confirmed'
                ? 'border-scema-green text-scema-green bg-scema-green/10'
                : status === 'error'
                ? 'border-scema-red-hi text-scema-red-hi'
                : 'border-scema-red text-scema-red-hi bg-scema-red/10 hover:bg-scema-red/20'
            }`}
          >
            {status === 'sending'   ? 'SENDING…'   :
             status === 'confirmed' ? '✓ SENT'     :
             status === 'error'     ? 'FAILED'     : 'SEND'}
          </button>
        </div>

        {/* Feedback */}
        {status === 'confirmed' && sig && (
          <a
            href={`https://solscan.io/tx/${sig}`}
            target="_blank"
            rel="noopener noreferrer"
            className="text-scema-green text-xs hover:underline truncate"
          >
            ✓ {sig.slice(0, 20)}… — view on Solscan ↗
          </a>
        )}
        {status === 'error' && (
          <p className="text-scema-red-hi text-xs truncate">{errMsg}</p>
        )}
      </div>
    </div>
  )
}
