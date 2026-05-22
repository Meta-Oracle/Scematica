'use client'

import dynamic from 'next/dynamic'
import { type ReactNode } from 'react'

// Wallet adapter uses browser APIs (window.solana, localStorage, Standard Wallet
// registration) that don't exist on the server. Rendering it during SSR produces
// different HTML than the client's first paint → React hydration error #418.
// Importing with ssr: false keeps server and client renders identical until
// JS hydrates, then the wallet provider mounts cleanly on the client.
const WalletProviderWrapper = dynamic(
  () => import('./WalletProvider').then(m => ({ default: m.WalletProviderWrapper })),
  { ssr: false }
)

export function Providers({ children }: { children: ReactNode }) {
  return <WalletProviderWrapper>{children}</WalletProviderWrapper>
}
