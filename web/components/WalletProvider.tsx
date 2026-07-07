'use client'

import { useEffect, useMemo } from 'react'
import { ConnectionProvider, WalletProvider } from '@solana/wallet-adapter-react'
import { WalletAdapterNetwork } from '@solana/wallet-adapter-base'
import { WalletModalProvider } from '@solana/wallet-adapter-react-ui'
import { clusterApiUrl } from '@solana/web3.js'
import { ScemaGateProvider } from '@/lib/ScemaGateContext'
import { isNative } from '@/lib/net'

// Import wallet adapter CSS
import '@solana/wallet-adapter-react-ui/styles.css'

const RPC_ENDPOINT =
  process.env.NEXT_PUBLIC_RPC_ENDPOINT ||
  'https://mainnet.helius-rpc.com/?api-key=b1d54eff-a0db-4e72-84da-61def55d5d55'

// On a native (Capacitor) build there are no browser wallet extensions, so nothing
// registers as a Standard Wallet and the token-gate can't connect. Register **Mobile
// Wallet Adapter** as a Standard Wallet — the existing WalletProvider auto-detects it,
// and connecting deep-links to the installed Phantom/Solflare app. Dynamically imported
// and native-only, so the web bundle and its extension flow are untouched.
let mwaRegistered = false
function registerMobileWalletAdapter() {
  if (mwaRegistered || typeof window === 'undefined' || !isNative()) return
  mwaRegistered = true
  import('@solana-mobile/wallet-standard-mobile')
    .then(mwa => {
      mwa.registerMwa({
        appIdentity: {
          name: 'Scematica',
          uri: 'https://github.com/Meta-Oracle/Scematica',
          icon: 'favicon.ico',
        },
        authorizationCache: mwa.createDefaultAuthorizationCache(),
        chains: ['solana:mainnet'],
        chainSelector: mwa.createDefaultChainSelector(),
        onWalletNotFound: mwa.createDefaultWalletNotFoundHandler(),
      })
    })
    .catch(() => {
      /* MWA unavailable (e.g. no wallet app) — extension/web path still works */
    })
}

export function WalletProviderWrapper({ children }: { children: React.ReactNode }) {
  const network = WalletAdapterNetwork.Mainnet
  const endpoint = RPC_ENDPOINT || clusterApiUrl(network)

  // Empty array: Phantom, Backpack, Solflare etc. auto-register via
  // the Wallet Standard protocol — no explicit adapters needed. On native, MWA is
  // registered the same way (see registerMobileWalletAdapter).
  const wallets = useMemo(() => [], [])

  useEffect(() => { registerMobileWalletAdapter() }, [])

  return (
    <ConnectionProvider endpoint={endpoint}>
      <WalletProvider wallets={wallets} autoConnect>
        <WalletModalProvider>
          <ScemaGateProvider>
            {children}
          </ScemaGateProvider>
        </WalletModalProvider>
      </WalletProvider>
    </ConnectionProvider>
  )
}
