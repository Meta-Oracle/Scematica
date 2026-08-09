import type { Metadata } from 'next'

import { ScylarTerminal } from '@/components/scylar/ScylarTerminal'

// Scylar — the avatar chat terminal. A third product on the same site, alongside the
// sniper dashboard and /alchem-link.
//
// Deliberately outside MobileGate and the wallet/token gate that wrap the dashboard:
// this page holds no funds, reads no wallet, and issues no control POSTs. There is
// nothing here to gate on, and gating it would only stop people from meeting her.

export const metadata: Metadata = {
  title: 'SCYLAR — Resident Intelligence',
  description:
    'Talk to Scylar, the resident intelligence of the Scematica terminal — an animated avatar chat running on free-tier LLM inference.',
  keywords: ['Scylar', 'Scematica', 'AI avatar', 'chat', 'Solana', 'terminal'],
}

export default function ScylarTerminalPage() {
  return <ScylarTerminal />
}
