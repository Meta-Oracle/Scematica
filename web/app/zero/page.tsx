import type { Metadata } from 'next'

import { ZeroTerminal } from '@/components/zero/ZeroTerminal'

// Scematica Zero. Deliberately outside the SCEMA token gate that wraps the sniper
// dashboard, for the same reason /escrow and /alchem-link are: reading is the part that
// has to be public, and a verifiable track record nobody can open is a screenshot.
// Arming autonomy is gated — see lib/zero/gatekeep.ts, which also says plainly that a
// client-side check with no server behind it is a default rather than a boundary.

export const metadata: Metadata = {
  title: 'SCEMATICA ZERO — the loop in your browser, with no local API',
  description:
    'A Solana trading loop that runs entirely in a browser tab: your own RPC key, your own wallet, no install and no custody. Every decision is sealed as a verifiable record — including the ones it declined.',
  keywords: ['Solana', 'trading bot', 'browser', 'non-custodial', 'verifiable', 'Scematica'],
}

export default function ZeroPage() {
  return <ZeroTerminal />
}
