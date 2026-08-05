'use client'

import { useState } from 'react'
import { BUY_URL, DEXSCREENER_URL, PUMPFUN_URL, SCEMA_MINT as CA } from '@/lib/scemaLinks'

export function CABanner() {
  const [copied, setCopied] = useState(false)

  function copy() {
    navigator.clipboard.writeText(CA).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    })
  }

  return (
    <div className="relative overflow-hidden border-y border-scema-red-dim bg-scema-red-bg/40 py-2.5">
      {/* Scrolling background text */}
      <div className="absolute inset-0 flex items-center overflow-hidden opacity-[0.04] pointer-events-none select-none">
        <div className="flex gap-8 whitespace-nowrap text-scema-red text-xs"
          style={{ animation: 'marquee 24s linear infinite' }}>
          {Array.from({length: 10}).map((_, i) => (
            <span key={i}>$SCEMA · {CA} · BUY ON JUPITER · 250K REQUIRED · </span>
          ))}
        </div>
      </div>

      <div className="relative z-10 flex flex-wrap items-center justify-center gap-x-4 gap-y-2 px-4">

        {/* Token name badge */}
        <div className="flex items-center gap-1.5">
          <span className="text-scema-red-hi font-bold text-xs tracking-widest">$SCEMA</span>
          <span className="text-scema-red-dim text-xs">·</span>
          <span className="text-scema-muted text-xs tracking-widest">CONTRACT ADDRESS</span>
        </div>

        {/* Address */}
        <div className="flex items-center gap-1 bg-black/50 border border-scema-red-dim/50 px-3 py-1">
          <span className="text-scema-red-hi text-xs font-mono">{CA.slice(0, 6)}</span>
          <span className="text-scema-muted text-xs font-mono hidden sm:inline">{CA.slice(6, -6)}</span>
          <span className="text-scema-red-hi text-xs font-mono">{CA.slice(-6)}</span>
        </div>

        {/* Copy */}
        <button
          onClick={copy}
          className={`text-xs px-3 py-1 border font-mono uppercase tracking-widest transition-all duration-150 ${
            copied
              ? 'border-scema-green/60 text-scema-green bg-scema-green/10'
              : 'border-scema-red-dim text-scema-muted hover:border-scema-red hover:text-scema-red-hi hover:bg-scema-red/10'
          }`}
        >
          {copied ? '✓ COPIED' : '⎘ COPY'}
        </button>

        {/* Buy — Jupiter, not pump.fun: see lib/scemaLinks.ts */}
        <a
          href={BUY_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="text-xs px-3 py-1 border border-scema-red bg-scema-red/20 text-scema-red-hi
                     hover:bg-scema-red hover:text-white font-bold uppercase tracking-widest
                     transition-all duration-150"
        >
          ⚡ BUY
        </a>

        {/* Bonding-curve UI, for anyone who wants it. */}
        <a
          href={PUMPFUN_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="text-xs px-2 py-1 border border-transparent text-scema-muted
                     hover:border-scema-red-dim hover:text-scema-red-hi transition-all duration-150
                     uppercase tracking-widest"
        >
          ↗ PUMP
        </a>

        {/* Chart */}
        <a
          href={DEXSCREENER_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="text-xs px-2 py-1 border border-transparent text-scema-muted
                     hover:border-scema-red-dim hover:text-scema-red-hi transition-all duration-150
                     uppercase tracking-widest"
        >
          ↗ CHART
        </a>
      </div>

      <style jsx>{`
        @keyframes marquee {
          0%   { transform: translateX(0); }
          100% { transform: translateX(-50%); }
        }
      `}</style>
    </div>
  )
}
