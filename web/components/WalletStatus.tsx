'use client'

import { useEffect, useState } from 'react'
import { useWallet, useConnection } from '@solana/wallet-adapter-react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { PublicKey, LAMPORTS_PER_SOL } from '@solana/web3.js'
import { getAssociatedTokenAddress, getAccount } from '@solana/spl-token'

const SCEMA_MINT = new PublicKey('AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump')
const SCEMA_REQUIRED = 250_000

function shortAddr(addr: string) {
  return `${addr.slice(0, 4)}…${addr.slice(-4)}`
}

export function WalletStatus() {
  const { publicKey, connected } = useWallet()
  const { connection } = useConnection()
  const [sol, setSol] = useState<number | null>(null)
  const [scema, setScema] = useState<number | null>(null)
  const [gated, setGated] = useState<boolean | null>(null)

  useEffect(() => {
    if (!publicKey || !connected) {
      setSol(null); setScema(null); setGated(null)
      return
    }

    let cancelled = false

    async function fetchBalances() {
      try {
        const lamports = await connection.getBalance(publicKey!)
        if (!cancelled) setSol(lamports / LAMPORTS_PER_SOL)
      } catch {}

      try {
        const ata = await getAssociatedTokenAddress(SCEMA_MINT, publicKey!, false)
        const acct = await getAccount(connection, ata)
        const amount = Number(acct.amount) / 1e6
        if (!cancelled) {
          setScema(amount)
          setGated(amount >= SCEMA_REQUIRED)
        }
      } catch {
        if (!cancelled) { setScema(0); setGated(false) }
      }
    }

    fetchBalances()
    const iv = setInterval(fetchBalances, 15_000)
    return () => { cancelled = true; clearInterval(iv) }
  }, [publicKey, connected, connection])

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
              gated === true ? 'bg-scema-green animate-pulse' :
              gated === false ? 'bg-scema-red-hi' : 'bg-scema-muted'
            }`}/>
            {gated === true ? 'GATED ✓' : gated === false ? 'NO SCEMA' : '...'}
          </div>

          {/* Balances */}
          <div className="hidden md:flex flex-col items-end gap-0 leading-tight">
            <span className="text-scema-muted text-xs">{shortAddr(publicKey.toBase58())}</span>
            <div className="flex gap-2 text-xs">
              {sol !== null && (
                <span className="text-scema-text">{sol.toFixed(3)} <span className="text-scema-muted">SOL</span></span>
              )}
              {scema !== null && (
                <span className={scema >= SCEMA_REQUIRED ? 'text-scema-green' : 'text-scema-red-hi'}>
                  {scema >= 1000 ? `${(scema/1000).toFixed(0)}k` : scema.toFixed(0)} <span className="text-scema-muted">SCEMA</span>
                </span>
              )}
            </div>
          </div>
        </>
      ) : null}

      <WalletMultiButton />
    </div>
  )
}
