import { AdvancedOnly }    from '@/components/AdvancedOnly'
import { CABanner }        from '@/components/CABanner'
import { DecisionLedger }  from '@/components/DecisionLedger'
import { ExecutionQuality } from '@/components/ExecutionQuality'
import { FilterStatsPanel } from '@/components/FilterStats'
import { GatedControls }    from '@/components/GatedControls'
import { HealthBadge }      from '@/components/HealthBadge'
import { Links }            from '@/components/Links'
import { LogStream }        from '@/components/LogStream'
import { MobileGate }       from '@/components/MobileGate'
import { MetricsPanel }     from '@/components/MetricsPanel'
import { NNStatus }         from '@/components/NNStatus'
import { OpenPositions }    from '@/components/OpenPositions'
import { PnlChart }         from '@/components/PnlChart'
import { PoolRadar }        from '@/components/PoolRadar'
import { SniperControls }   from '@/components/SniperControls'
import { TradeFee }         from '@/components/TradeFee'
import { TradesHistory }    from '@/components/TradesHistory'
import { OfflineBanner }    from '@/components/OfflineBanner'
import { WalletStatus }     from '@/components/WalletStatus'

// Panel row: grid children need both h-full and min-h-0 so that
// panels can resolve h-full and overflow-y-auto constrains correctly.
function Row2({ left, right, height = 'h-72' }: {
  left: React.ReactNode
  right: React.ReactNode
  height?: string
}) {
  return (
    <section className={`grid grid-cols-1 lg:grid-cols-3 gap-3 ${height} overflow-hidden`}>
      <div className="lg:col-span-2 h-full min-h-0 overflow-hidden">{left}</div>
      <div className="h-full min-h-0 overflow-hidden">{right}</div>
    </section>
  )
}

export default function Home() {
  return (
    <MobileGate>
    <div className="flex flex-col min-h-screen">

      {/* ── Header ─────────────────────────────────────────────────────── */}
      <header className="sticky top-0 z-50 border-b border-scema-border bg-scema-black/95 backdrop-blur-sm">
        <div className="flex items-center justify-between px-4 py-3 max-w-[1600px] mx-auto gap-4">
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
          <div className="hidden lg:flex items-center gap-2 text-xs text-scema-muted">
            <span className="w-1.5 h-1.5 rounded-full bg-scema-red-hi animate-pulse" />
            MAINNET
            <span className="text-scema-dim mx-1">·</span>
            <span className="text-scema-dim">RAYDIUM AMM V4</span>
            <span className="text-scema-dim mx-1">·</span>
            <span className="text-scema-dim">3-LAYER AI</span>
          </div>
          <div className="flex items-center gap-3 ml-auto">
            <HealthBadge />
            <TradeFee />
            <WalletStatus />
          </div>
        </div>
      </header>

      <CABanner />
      <OfflineBanner />

      <main className="flex-1 max-w-[1600px] mx-auto w-full px-3 py-4 flex flex-col gap-4">

        {/* Live metrics */}
        <section>
          <p className="text-xs text-scema-muted tracking-widest uppercase mb-2">◈ Live Metrics</p>
          <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-2">
            <MetricsPanel />
          </div>
        </section>

        {/* Intelligence (advanced) */}
        <AdvancedOnly>
          <section>
            <p className="text-xs text-scema-muted tracking-widest uppercase mb-2">◈ Intelligence</p>
            <NNStatus />
          </section>
        </AdvancedOnly>

        {/* Controls */}
        <section>
          <p className="text-xs text-scema-muted tracking-widest uppercase mb-2">
            ◈ Controls <span className="text-scema-dim ml-1">(connect wallet to unlock)</span>
          </p>
          <GatedControls>
            <SniperControls />
          </GatedControls>
        </section>

        {/* Pool Radar + Filter Stats (advanced) */}
        <AdvancedOnly>
          <Row2 height="h-80"
            left={<PoolRadar />}
            right={<FilterStatsPanel />}
          />
        </AdvancedOnly>

        {/* Intelligence ledgers (advanced) */}
        <AdvancedOnly>
          <Row2 height="h-72"
            left={<DecisionLedger />}
            right={<ExecutionQuality />}
          />
        </AdvancedOnly>

        {/* Trades + Open Positions */}
        <Row2 height="h-72"
          left={<TradesHistory />}
          right={<OpenPositions />}
        />

        {/* PnL Chart */}
        <section className="h-52 overflow-hidden">
          <PnlChart />
        </section>

        {/* Log Stream (advanced) */}
        <AdvancedOnly>
          <section className="h-72 overflow-hidden">
            <LogStream />
          </section>
        </AdvancedOnly>

      </main>

      <Links />
    </div>
    </MobileGate>
  )
}
