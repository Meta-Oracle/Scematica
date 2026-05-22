'use client'

import { createContext, useContext, useEffect, useState, type ReactNode } from 'react'
import { useWallet, useConnection } from '@solana/wallet-adapter-react'
import { PublicKey, LAMPORTS_PER_SOL } from '@solana/web3.js'
import { getAssociatedTokenAddressSync, getAccount, TOKEN_2022_PROGRAM_ID } from '@solana/spl-token'

const SCEMA_MINT_PK = new PublicKey('AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump')
export const SCEMA_REQUIRED = 250_000
export const SCEMA_MINT = 'AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump'

export interface ScemaGateCtx {
  scemaBalance: number | null   // null = not yet fetched (wallet disconnected or loading)
  solBalance:   number | null
  gated:        boolean | null  // null = loading; true = holds ≥250k; false = insufficient
}

const Ctx = createContext<ScemaGateCtx>({ scemaBalance: null, solBalance: null, gated: null })

export function ScemaGateProvider({ children }: { children: ReactNode }) {
  const { publicKey, connected } = useWallet()
  const { connection } = useConnection()
  const [scemaBalance, setScemaBalance] = useState<number | null>(null)
  const [solBalance, setSolBalance]     = useState<number | null>(null)
  const [gated, setGated]               = useState<boolean | null>(null)

  useEffect(() => {
    if (!publicKey || !connected) {
      setScemaBalance(null); setSolBalance(null); setGated(null)
      return
    }
    let cancelled = false

    async function refresh() {
      // SOL balance
      try {
        const lam = await connection.getBalance(publicKey!)
        if (!cancelled) setSolBalance(lam / LAMPORTS_PER_SOL)
      } catch {}

      // SCEMA balance (Token-2022)
      try {
        const ata  = getAssociatedTokenAddressSync(SCEMA_MINT_PK, publicKey!, false, TOKEN_2022_PROGRAM_ID)
        const acct = await getAccount(connection, ata, 'confirmed', TOKEN_2022_PROGRAM_ID)
        const bal  = Number(acct.amount) / 1e6
        if (!cancelled) {
          setScemaBalance(bal)
          setGated(bal >= SCEMA_REQUIRED)
        }
      } catch {
        // ATA doesn't exist → zero balance → not gated
        if (!cancelled) { setScemaBalance(0); setGated(false) }
      }
    }

    refresh()
    const iv = setInterval(refresh, 15_000)
    return () => { cancelled = true; clearInterval(iv) }
  }, [publicKey, connected, connection])

  return <Ctx.Provider value={{ scemaBalance, solBalance, gated }}>{children}</Ctx.Provider>
}

export function useScemaGate(): ScemaGateCtx { return useContext(Ctx) }
