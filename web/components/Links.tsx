import React from 'react'
import { SCEMA_MINT as CA } from '@/lib/ScemaGateContext'

const LINKS = [
  {
    label: 'TELEGRAM',
    href: 'https://t.me/Scematica',
    icon: (
      <svg viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4">
        <path d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z"/>
      </svg>
    ),
    desc: 'Join the community',
  },
  {
    label: 'X / TWITTER',
    href: 'https://x.com/scematica',
    icon: (
      <svg viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4">
        <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-4.714-6.231-5.401 6.231H2.747l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"/>
      </svg>
    ),
    desc: 'Follow updates',
  },
  {
    label: 'GITHUB',
    href: 'https://github.com/Meta-Oracle/Scematica',
    icon: (
      <svg viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4">
        <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12"/>
      </svg>
    ),
    desc: 'Source code',
  },
]

export function Links() {
  return (
    <section className="border-t border-scema-border mt-2 py-8 px-4 bg-scema-dark/40">
      <div className="max-w-4xl mx-auto flex flex-col items-center gap-6">

        {/* Buy button — prominent CTA */}
        <a
          href={`https://pump.fun/coin/${CA}`}
          target="_blank"
          rel="noopener noreferrer"
          className="group relative flex items-center gap-3 px-8 py-3 bg-scema-red border border-scema-red-hi
                     text-white font-bold text-sm tracking-widest uppercase
                     hover:bg-scema-red-hi hover:shadow-red-lg
                     transition-all duration-200 animate-glow-pulse"
        >
          {/* Corner accents */}
          <span className="absolute top-0 left-0 w-2 h-2 border-t-2 border-l-2 border-white/30" />
          <span className="absolute top-0 right-0 w-2 h-2 border-t-2 border-r-2 border-white/30" />
          <span className="absolute bottom-0 left-0 w-2 h-2 border-b-2 border-l-2 border-white/30" />
          <span className="absolute bottom-0 right-0 w-2 h-2 border-b-2 border-r-2 border-white/30" />

          <svg viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4">
            <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 14.5v-9l6 4.5-6 4.5z"/>
          </svg>
          BUY $SCEMA
          <span className="text-white/60 text-xs font-normal normal-case">pump.fun ↗</span>
        </a>

        {/* Divider */}
        <div className="flex items-center gap-3 w-full max-w-xs">
          <div className="flex-1 h-px bg-scema-border" />
          <span className="text-scema-dim text-xs tracking-widest">COMMUNITY</span>
          <div className="flex-1 h-px bg-scema-border" />
        </div>

        {/* Community links — separated clearly */}
        <div className="flex flex-col sm:flex-row justify-center items-stretch gap-3 w-full max-w-lg">
          {LINKS.map((link) => (
            <a
              key={link.label}
              href={link.href}
              target="_blank"
              rel="noopener noreferrer"
              className="group flex items-center gap-3 px-5 py-3 border border-scema-red-dim bg-scema-red-bg/20
                         hover:border-scema-red hover:bg-scema-red/10 hover:shadow-red-sm
                         transition-all duration-200 flex-1 justify-center sm:flex-col sm:items-center sm:text-center"
            >
              <span className="text-scema-red-dim group-hover:text-scema-red-hi transition-colors">
                {link.icon}
              </span>
              <div className="flex flex-col leading-tight">
                <span className="text-xs font-bold tracking-widest text-scema-muted group-hover:text-scema-red-hi transition-colors">
                  {link.label}
                </span>
                <span className="text-xs text-scema-dim group-hover:text-scema-muted transition-colors">
                  {link.desc}
                </span>
              </div>
            </a>
          ))}
        </div>

        <p className="text-center text-scema-dim text-xs tracking-widest mt-2">
          SCEMATICA © 2026 · SOLANA MAINNET · 250K $SCEMA REQUIRED TO RUN
        </p>
      </div>
    </section>
  )
}
