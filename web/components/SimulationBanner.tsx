'use client'

import { useState } from 'react'
import { useDataSource } from '@/lib/useDataSource'
import { Pairing } from './Pairing'

// Shown whenever the dashboard is running on the built-in simulation engine
// instead of a real sniper. This must stay loud and must not be dismissible: every
// number on screen — PnL, positions, win rate — is synthetic while it's up, and a
// visitor who mistakes it for live trading performance has been misled.
export function SimulationBanner() {
  const source = useDataSource()
  const [pairing, setPairing] = useState(false)

  if (pairing) {
    return (
      <div className="fixed inset-0 z-[100] bg-scema-black overflow-y-auto">
        <button
          onClick={() => setPairing(false)}
          aria-label="Cancel pairing"
          className="absolute top-4 right-4 z-10 text-scema-dim hover:text-scema-muted text-lg"
        >
          ✕
        </button>
        <Pairing onPaired={() => setPairing(false)} />
      </div>
    )
  }

  if (source !== 'simulation') return null

  return (
    <div className="border-b border-scema-amber/40 bg-scema-amber/[0.07] px-4 py-2">
      <div className="max-w-[1600px] mx-auto flex items-center justify-between gap-4 flex-wrap">
        <div className="flex items-center gap-3 flex-wrap text-xs">
          <div className="flex items-center gap-1.5 shrink-0">
            <span className="w-1.5 h-1.5 rounded-full bg-scema-amber animate-pulse" />
            <span className="text-scema-amber font-bold tracking-widest uppercase">
              Simulation
            </span>
          </div>
          <span className="text-scema-muted">
            Demo session — <span className="text-scema-amber">not real trades or funds.</span>{' '}
            The Deep Q*™ network is genuinely running; the market feeding it is synthetic.
          </span>
          <button
            onClick={() => setPairing(true)}
            className="text-xs px-2 py-1 border border-scema-amber/50 text-scema-amber
                       hover:bg-scema-amber/10 font-bold uppercase tracking-widest transition-colors"
          >
            Connect your bot ↗
          </button>
        </div>
      </div>
    </div>
  )
}
