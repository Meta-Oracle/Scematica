'use client'

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
import { PublicKey } from '@solana/web3.js'
import { isNative } from './net'
import {
  buildConnectUrl,
  disconnectWallet,
  getConnectedAddress,
  handleConnectRedirect,
  type WalletProvider,
} from './mobileWallet'

interface MobileWalletCtx {
  publicKey: PublicKey | null
  connected: boolean
  connecting: boolean
  error: string | null
  connect: (provider: WalletProvider) => void
  disconnect: () => void
}

const Ctx = createContext<MobileWalletCtx>({
  publicKey: null,
  connected: false,
  connecting: false,
  error: null,
  connect: () => {},
  disconnect: () => {},
})

// Native-only wallet: connects via the Phantom deeplink protocol (lib/mobileWallet). On
// web it's an inert passthrough — the wallet-adapter path handles browsers. Listens for
// the `scematica://wallet?...` deep link the wallet app redirects to after approval.
export function MobileWalletProvider({ children }: { children: ReactNode }) {
  const [address, setAddress] = useState<string | null>(null)
  const [connecting, setConnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!isNative()) return
    const saved = getConnectedAddress()
    if (saved) setAddress(saved)

    let remove: (() => void) | undefined
    import('@capacitor/app')
      .then(({ App }) =>
        App.addListener('appUrlOpen', ({ url }) => {
          if (!url.includes('wallet')) return // only our connect redirect
          try {
            const q = new URL(url).searchParams
            const addr = handleConnectRedirect(q)
            setAddress(addr)
            setError(null)
          } catch (e) {
            setError(e instanceof Error ? e.message : 'connect failed')
          } finally {
            setConnecting(false)
          }
        }),
      )
      .then(handle => {
        remove = () => handle.remove()
      })
      .catch(() => {})

    return () => remove?.()
  }, [])

  const connect = useCallback((provider: WalletProvider) => {
    setConnecting(true)
    setError(null)
    const url = buildConnectUrl(provider)
    // `_blank` makes Capacitor hand the universal link to the OS, which routes it to the
    // wallet app; the app returns via the `scematica://wallet` deep link above.
    window.open(url, '_blank')
  }, [])

  const disconnect = useCallback(() => {
    disconnectWallet()
    setAddress(null)
    setError(null)
  }, [])

  const publicKey = useMemo(() => {
    try {
      return address ? new PublicKey(address) : null
    } catch {
      return null
    }
  }, [address])

  const value = useMemo(
    () => ({ publicKey, connected: !!publicKey, connecting, error, connect, disconnect }),
    [publicKey, connecting, error, connect, disconnect],
  )

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>
}

export const useMobileWallet = () => useContext(Ctx)
