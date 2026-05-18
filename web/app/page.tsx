import { CABanner } from '@/components/CABanner'
import { FilterStatsPanel } from '@/components/FilterStats'
import { Links } from '@/components/Links'
import { LogStream } from '@/components/LogStream'
import { MetricsPanel } from '@/components/MetricsPanel'
import { NNStatus } from '@/components/NNStatus'
import { PoolRadar } from '@/components/PoolRadar'
import { WalletStatus } from '@/components/WalletStatus'

export default function Home() {
  return (
    <div className="flex flex-col min-h-screen">

      {/* ── Header ─────────────────────────────────────────────────────────── */}
      <header className="sticky top-0 z-50 border-b border-scema-border bg-scema-black/95 backdrop-blur-sm">
        <div className="flex items-center justify-between px-4 py-3 max-w-[1600px] mx-auto gap-4">

          {/* Logo */}
          <div className="flex items-center gap-3 shrink-0">
            {/* Red diamond logo */}
            <div className="relative w-7 h-7 flex items-center justify-center">
              <div className="absolute inset-0 border border-scema-red rotate-45 animate-glow-pulse" />
              <span className="text-scema-red-hi text-xs font-bold relative z-10">S</span>
            </div>
            <div className="flex flex-col leading-tight">
              <span className="text-scema-red-hi font-bold tracking-[0.3em] text-sm animate-text-glow">
                SCEMATICA
              </span>
              <span className="text-scema-dim text-xs tracking-widest">SOLANA SNIPER PROTOCOL</span>
            </div>
          </div>

          {/* Status dot */}
          <div className="hidden lg:flex items-center gap-2 text-xs text-scema-muted">
            <span className="w-1.5 h-1.5 rounded-full bg-scema-red-hi animate-pulse" />
            MAINNET
            <span className="text-scema-dim mx-1">·</span>
            <span className="text-scema-dim">RAYDIUM AMM V4</span>
          </div>

          {/* Wallet */}
          <WalletStatus />
        </div>
      </header>

      {/* ── CA Banner ──────────────────────────────────────────────────────── */}
      <CABanner />

      {/* ── Main content ───────────────────────────────────────────────────── */}
      <main className="flex-1 max-w-[1600px] mx-auto w-full px-3 py-4 flex flex-col gap-4">

        {/* Metrics row */}
        <section>
          <p className="text-xs text-scema-muted tracking-widest uppercase mb-2">◈ Live Metrics</p>
          <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-2">
            <MetricsPanel />
          </div>
        </section>

        {/* NN status bar */}
        <NNStatus />

        {/* Pool radar + Filter stats */}
        <section className="grid grid-cols-1 lg:grid-cols-3 gap-3" style={{ height: '380px' }}>
          <div className="lg:col-span-2 h-full">
            <PoolRadar />
          </div>
          <div className="h-full">
            <FilterStatsPanel />
          </div>
        </section>

        {/* Log stream */}
        <section style={{ height: '320px' }}>
          <LogStream />
        </section>
      </main>

      {/* ── Footer / Links ─────────────────────────────────────────────────── */}
      <Links />
    </div>
  )
}
