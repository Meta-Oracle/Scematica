import type { Metadata } from 'next'

import { MeshTerminal } from '@/components/mesh/MeshTerminal'

// Scematica Mesh — the running system's own topology.
//
// Reads GET /api/mesh, which is served by `crates/scematica-mesh` through the Rust API.
// Unlike every other panel on this site there is NO simulated fallback: a fabricated
// metric is a fake number, but a fabricated topology asserts that a particular set of
// units exists and is healthy on the operator's machine. See the `case 'mesh'` comment in
// app/api/[...slug]/route.ts.

export const metadata: Metadata = {
  title: 'SCEMATICA MESH — the agent’s own topology, and the gate over it',
  description:
    'Every decision-making unit in the running Scematica system — learners, filters, risk breakers, reasoners — with what each last decided, whether it can be seen at all, and the agentic coherence gate Ψ = C · K · (1 − R) computed over them.',
  keywords: [
    'agentic architecture',
    'neural mesh',
    'observability',
    'coherence gate',
    'Solana trading bot',
    'reinforcement learning',
  ],
}

export default function MeshPage() {
  return <MeshTerminal />
}
