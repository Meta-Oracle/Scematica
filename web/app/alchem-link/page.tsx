import type { Metadata } from 'next'

import { AlchemConsole } from '@/components/alchem/AlchemConsole'

// The web build of alchem-link. Deliberately outside MobileGate and the wallet/token
// gate that wrap the sniper dashboard: this page reads public oracle data, holds no
// funds, and issues no control POSTs, so there is nothing here to gate on.

export const metadata: Metadata = {
  title: 'ALCHEM-LINK — Alchemy × Chainlink Oracle Console',
  description:
    'Live Chainlink price feeds with staleness verdicts, RPC diagnostics, and an on-chain verified registry across six networks.',
  keywords: ['Chainlink', 'Alchemy', 'oracle', 'price feeds', 'RPC', 'web3'],
}

export default function AlchemLinkPage() {
  return <AlchemConsole />
}
