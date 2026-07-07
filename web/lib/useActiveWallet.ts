'use client'

import { useWallet } from '@solana/wallet-adapter-react'
import type { PublicKey } from '@solana/web3.js'
import { isNative } from './net'
import { useMobileWallet } from './MobileWalletContext'

// One wallet identity for the rest of the app: the Phantom-deeplink wallet on native, the
// browser wallet-adapter on web. Both hooks are called unconditionally (rules of hooks);
// `isNative()` is stable for the session, so the branch is safe.
export function useActiveWallet(): { publicKey: PublicKey | null; connected: boolean } {
  const web = useWallet()
  const mobile = useMobileWallet()
  if (isNative()) return { publicKey: mobile.publicKey, connected: mobile.connected }
  return { publicKey: web.publicKey ?? null, connected: web.connected }
}
