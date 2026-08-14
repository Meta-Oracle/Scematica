import type { Metadata } from 'next'

import { MarketTerminal } from '@/components/escrow/MarketTerminal'

// The Escrow Market. Like /alchem-link, deliberately outside the TOKEN gate that wraps
// the sniper dashboard — this page reads public on-chain state and gating a page whose
// purpose is public verifiability would defeat it. It does connect a wallet, but only to
// let you create a vault of your own; reading requires nothing.

export const metadata: Metadata = {
  title: 'SCEMA ESCROW MARKET — Live Solana launches, ranked by what backs them',
  description:
    'A live Solana token market ranked by verifiable on-chain backing rather than volume. Launches from pump.fun, Raydium and Meteora beside the reserve actually held for each token, measured at a named slot, with no prices in the custody figures and no oracle.',
  keywords: [
    'escrow',
    'proof of reserve',
    'Solana',
    'non-custodial',
    'vault',
    'BTC backing',
    'token launches',
    'pump.fun',
    'Raydium',
    'DEX',
  ],
}

export default function EscrowPage() {
  return <MarketTerminal />
}
