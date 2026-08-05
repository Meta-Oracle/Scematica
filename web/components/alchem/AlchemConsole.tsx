'use client'

import Link from 'next/link'
import { useState } from 'react'

import { DoctorPanel } from './DoctorPanel'
import { FeedBoard } from './FeedBoard'
import { ReferencePanel } from './ReferencePanel'
import { RegistryPanel } from './RegistryPanel'
import { DEFAULT_NETWORK, listNetworks } from '@/lib/alchem/networks'

// The web build of alchem-link, laid out like the terminal it mirrors: network picker in
// the chrome, live feeds as the primary surface, diagnostics beside it, reference below.
//
// Network lives in this component rather than the URL because every panel below keys its
// poll on it — one piece of state, and switching it tears down the previous network's
// timers through the shared store's refcount. `listNetworks()` is a static table with no
// server-only imports, so reading it in a client component is safe.

const NETWORKS = listNetworks()

export function AlchemConsole() {
  const [network, setNetwork] = useState(DEFAULT_NETWORK)

  return (
    <div className="alchem-root flex flex-col">
      {/* ── Header ───────────────────────────────────────────────────────── */}
      <header className="sticky top-0 z-50 border-b border-alchem-border bg-alchem-black/95 backdrop-blur-sm">
        <div className="max-w-[1600px] mx-auto px-4 py-3 flex items-center justify-between gap-4">
          <div className="flex items-center gap-3 shrink-0">
            <div className="relative w-7 h-7 flex items-center justify-center">
              <div className="absolute inset-0 border border-alchem-blue rotate-45" />
              <span className="text-alchem-blue-hi text-xs font-bold relative z-10">A</span>
            </div>
            <div className="flex flex-col leading-tight">
              <span className="glow-blue font-bold tracking-[0.3em] text-sm">ALCHEM-LINK</span>
              <span className="text-alchem-dim text-xs tracking-widest">
                ALCHEMY × CHAINLINK ORACLE CONSOLE
              </span>
            </div>
          </div>

          <Link
            href="/"
            className="text-alchem-dim hover:text-alchem-blue text-xs tracking-widest transition-colors shrink-0"
          >
            ← SCEMATICA
          </Link>
        </div>

        {/* ── Network picker ─────────────────────────────────────────────── */}
        <div className="max-w-[1600px] mx-auto px-4 pb-2 flex flex-wrap items-center gap-1.5">
          <span className="text-alchem-dim text-[0.6rem] uppercase tracking-widest mr-1">
            Network
          </span>
          {NETWORKS.map(net => (
            <button
              key={net.key}
              onClick={() => setNetwork(net.key)}
              title={`${net.label} · chain id ${net.chainId}`}
              className={`px-2.5 py-1 text-[0.65rem] border transition-colors ${
                network === net.key
                  ? 'border-alchem-blue text-alchem-blue-hi bg-alchem-blue/10 shadow-blue-sm'
                  : 'border-alchem-border text-alchem-muted hover:border-alchem-border-hi hover:text-alchem-text'
              }`}
            >
              {net.key}
            </button>
          ))}
        </div>
      </header>

      <main className="flex-1 max-w-[1600px] mx-auto w-full px-3 py-4 flex flex-col gap-4">
        {/* Live board is the primary surface — the staleness verdict is the product. */}
        <section className="grid grid-cols-1 lg:grid-cols-3 gap-3 lg:h-[30rem]">
          <div className="lg:col-span-2 min-h-[24rem] lg:min-h-0">
            <FeedBoard network={network} />
          </div>
          <div className="min-h-[20rem] lg:min-h-0">
            <DoctorPanel network={network} />
          </div>
        </section>

        <section className="min-h-[16rem]">
          <RegistryPanel network={network} />
        </section>

        <section>
          <ReferencePanel />
        </section>

        <footer className="border-t border-alchem-border/50 pt-4 pb-6 text-center">
          <p className="text-alchem-dim text-[0.65rem] tracking-widest">
            Same reader as the terminal —{' '}
            <span className="text-alchem-blue-dim">pip install alchem-link</span>
            {' · '}
            <span className="text-alchem-blue-dim">alchem-link-ui</span>
          </p>
          <p className="text-alchem-dim text-[0.6rem] mt-1.5">
            Reads live Chainlink aggregators server-side. Prices are for development and
            monitoring — verify a heartbeat before trading on it.
          </p>
        </footer>
      </main>
    </div>
  )
}
