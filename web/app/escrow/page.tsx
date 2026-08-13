import type { Metadata } from 'next'

import { EscrowConsole } from '@/components/escrow/EscrowConsole'

// The Escrow Market proof-of-reserve page. Like /alchem-link, deliberately outside the
// wallet and token gate that wrap the sniper dashboard: this page reads public on-chain
// state, holds no funds and issues no control POSTs, so there is nothing to gate on —
// and gating a page whose entire purpose is public verifiability would defeat it.

export const metadata: Metadata = {
  title: 'SCEMA ESCROW MARKET — Proof of Reserve',
  description:
    'Time-locked, non-custodial backing for any Solana token. Verify on-chain how much reserve stands behind a token, measured at a named slot, with no prices and no oracle.',
  keywords: ['escrow', 'proof of reserve', 'Solana', 'non-custodial', 'vault', 'BTC backing'],
}

export default function EscrowPage() {
  return <EscrowConsole />
}
