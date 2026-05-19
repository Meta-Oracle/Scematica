import { CABanner }        from '@/components/CABanner'
import { FilterStatsPanel } from '@/components/FilterStats'
import { GatedControls }    from '@/components/GatedControls'
import { HealthBadge }      from '@/components/HealthBadge'
import { Links }            from '@/components/Links'
import { LogStream }        from '@/components/LogStream'
import { MetricsPanel }     from '@/components/MetricsPanel'
import { NNStatus }         from '@/components/NNStatus'
import { OpenPositions }    from '@/components/OpenPositions'
import { PnlChart }         from '@/components/PnlChart'
import { PoolRadar }        from '@/components/PoolRadar'
import { SniperControls }   from '@/components/SniperControls'
import { TradeFee }         from '@/components/TradeFee'
import { TradesHistory }    from '@/components/TradesHistory'
import { TopUpWallet }      from '@/components/TopUpWallet'
import { WalletStatus }     from '@/components/WalletStatus'

export default function Home() {
  return (
    <div className="flex flex-col min-h-screen">

      {/* ── Header ─────────────────────────────────────────────────────── */}
      <header className="sticky top-0 z-50 border-b border-scema-border bg-scema-black/95 backdrop-blur-sm">
        <div className="flex items-center justify-between px-4 py-3 max-w-[1600px] mx-auto gap-4">

          {/* Logo */}
          <div className="flex items-center gap-3 shrink-0">
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

          {/* Center info */}
          <div className="hidden lg:flex items-center gap-2 text-xs text-scema-muted">
            <span className="w-1.5 h-1.5 rounded-full bg-scema-red-hi animate-pulse" />
            MAINNET
            <span className="text-scema-dim mx-1">·</span>
            <span className="text-scema-dim">RAYDIUM AMM V4</span>
            <span className="text-scema-dim mx-1">·</span>
            <span className="text-scema-dim">3-LAYER AI</span>
          </div>

          {/* Right cluster */}
          <div className="flex items-center gap-3 ml-auto">
            <HealthBadge />
            <TradeFee />
            <WalletStatus />
          </div>
        </div>
      </header>

      {/* ── CA Banner ──────────────────────────────────────────────────── */}
      <CABanner />

      {/* ── Main ───────────────────────────────────────────────────────── */}
      <main className="flex-1 max-w-[1600px] mx-auto w-full px-3 py-4 flex flex-col gap-4">

        {/* Live metrics — 5 cards */}
        <section>
          <p className="text-xs text-scema-muted tracking-widest uppercase mb-2">◈ Live Metrics</p>
          <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-2">
            <MetricsPanel />
          </div>
        </section>

        {/* NN agent status */}
        <NNStatus />

        {/* Sniper controls — gated */}
        <section>
          <p className="text-xs text-scema-muted tracking-widest uppercase mb-2">
            ◈ Controls <span className="text-scema-dim ml-1">(connect wallet to unlock)</span>
          </p>
          <GatedControls>
            <SniperControls />
          </GatedControls>
        </section>

        {/* Pool Radar + Filter Stats */}
        <section className="grid grid-cols-1 lg:grid-cols-3 gap-3 h-80">
          <div className="lg:col-span-2 min-h-0">
            <PoolRadar />
          </div>
          <div className="min-h-0">
            <FilterStatsPanel />
          </div>
        </section>

        {/* Trades + Open Positions */}
        <section className="grid grid-cols-1 lg:grid-cols-3 gap-3 h-72">
          <div className="lg:col-span-2 min-h-0">
            <TradesHistory />
          </div>
          <div className="min-h-0">
            <OpenPositions />
          </div>
        </section>

        {/* PnL Chart + Top Up */}
        <section className="grid grid-cols-1 lg:grid-cols-3 gap-3 h-52">
          <div className="lg:col-span-2 min-h-0">
            <PnlChart />
          </div>
          <div className="min-h-0">
            <TopUpWallet />
          </div>
        </section>

        {/* Log Stream */}
        <section className="h-72">
          <LogStream />
        </section>

      </main>

      <Links />
    </div>
  )
}
