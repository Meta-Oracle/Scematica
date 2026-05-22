// Matches MetricsSnapshot in crates/scematica-core/src/metrics.rs
export interface Metrics {
  trades_attempted: number
  trades_confirmed: number
  trades_failed: number
  arb_opportunities_found: number
  arb_executed: number
  total_pnl_lamports: number   // divide by 1e9 for SOL
  pools_tracked: number
  uptime_secs: number
}

export interface Pool {
  mint: string
  score: number
  size_sol: number
  age_secs: number
  passed_filters: boolean
  timestamp: number
}

export interface FilterStats {
  pools_seen: number
  pools_passed: number
  rejections: Record<string, number>
}

export interface NNStats {
  step_count: number
  train_steps: number
  epsilon: number
  ready_to_advise: boolean
  total_reward: number
  replay_size: number
  avg_loss: number
  target_updates: number
  last_q_values: number[]
}

export interface Trade {
  timestamp: string          // ISO-8601
  kind: 'BUY' | 'SELL' | 'ARB'
  mint: string
  symbol: string
  amount: number             // SOL for BUY/ARB; token units for SELL
  pnl: number                // realised SOL (0 for buys)
  pnl_pct: number            // percentage (e.g. -0.5 means -0.5%)
  status: string             // "✓" | "✗"
  signature: string
  dex: string
  hops: number
  position_age_secs: number
}

export interface HealthStatus {
  api: string
  sniper_running: boolean
  sniper_pid?: number
}
