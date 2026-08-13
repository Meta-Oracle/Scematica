import type { Metadata } from 'next'

import { EscrowConsole } from '@/components/escrow/EscrowConsole'
import { MarketTerminal } from '@/components/escrow/MarketTerminal'

// The Escrow Market. Like /alchem-link, deliberately outside the wallet and token gate
// that wrap the sniper dashboard: this page reads public on-chain state, holds no funds
// and issues no control POSTs, so there is nothing to gate on — and gating a page whose
// entire purpose is public verifiability would defeat it.
//
// Two surfaces, in deliberate order. The market board comes first because the argument
// is comparative: here are the live launches, and here is how little of it is backed.
// The single-vault verifier sits underneath for the case where you already know the two
// mints and want the full reading with slot and RPC provenance.

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
  return (
    <div className="escrow-root min-h-screen bg-escrow-black">
      <MarketTerminal />

      <section className="max-w-[1600px] mx-auto px-4 pb-16">
        <div className="border border-escrow-border bg-escrow-surface p-4">
          <h2 className="text-sm text-escrow-teal uppercase tracking-wider mb-1">
            Verify a single vault
          </h2>
          <p className="text-escrow-dim text-xs mb-4 max-w-3xl">
            The board above answers &ldquo;is anything behind this?&rdquo; for every token
            it can see. This answers it exhaustively for one pair — both legs, the vault
            PDA, the slot and the RPC that served it — so you can re-derive the figures
            yourself without trusting this page.
          </p>
          <EscrowConsole embedded />
        </div>
      </section>
    </div>
  )
}
