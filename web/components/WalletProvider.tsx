'use client'

import { useMemo } from 'react'
import { ConnectionProvider, WalletProvider } from '@solana/wallet-adapter-react'
import { WalletAdapterNetwork } from '@solana/wallet-adapter-base'
import { WalletModalProvider } from '@solana/wallet-adapter-react-ui'
import { clusterApiUrl } from '@solana/web3.js'
import { ScemaGateProvider } from '@/lib/ScemaGateContext'
import { MobileWalletProvider } from '@/lib/MobileWalletContext'
import { UiModeProvider } from '@/lib/UiModeContext'

// Import wallet adapter CSS
import '@solana/wallet-adapter-react-ui/styles.css'

const RPC_ENDPOINT =
  process.env.NEXT_PUBLIC_RPC_ENDPOINT ||
  'https://mainnet.helius-rpc.com/?api-key=b1d54eff-a0db-4e72-84da-61def55d5d55'

export function WalletProviderWrapper({ children }: { children: React.ReactNode }) {
  const network = WalletAdapterNetwork.Mainnet
  const endpoint = RPC_ENDPOINT || clusterApiUrl(network)

  // Empty array: on web, Phantom/Backpack/Solflare auto-register via Wallet Standard.
  // On native (Capacitor WebView) there are no extensions and the Mobile Wallet Adapter
  // web bridge refuses to run in a WebView, so native wallet connect is handled by
  // MobileWalletProvider (Phantom-deeplink protocol) instead.
  const wallets = useMemo(() => [], [])

  return (
    <ConnectionProvider endpoint={endpoint}>
      <WalletProvider wallets={wallets} autoConnect>
        <WalletModalProvider>
          <MobileWalletProvider>
            <UiModeProvider>
              <ScemaGateProvider>{children}</ScemaGateProvider>
            </UiModeProvider>
          </MobileWalletProvider>
        </WalletModalProvider>
      </WalletProvider>
    </ConnectionProvider>
  )
}
