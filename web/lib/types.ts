export interface Metrics {
  trades_attempted: number
  trades_confirmed: number
  pnl_sol: number
  win_rate: number
  uptime_secs: number
  open_positions: number
  daily_pnl_sol: number
  session_wins: number
  session_losses: number
  wallet_balance_sol: number
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
  epsilon: number
  ready_to_advise: boolean
  total_reward: number
  replay_size: number
  train_steps: number
}

export interface Trade {
  timestamp: string          // ISO-8601 from Rust
  kind: 'BUY' | 'SELL' | 'ARB'
  mint: string
  symbol: string
  amount: number             // quote SOL spent / received
  pnl: number                // realised PnL in SOL (0 for buys)
  pnl_pct: number
  status: string             // "✓" confirmed | "✗" failed
  signature: string          // tx signature
  dex: string
  hops: number
  position_age_secs: number
}

export interface HealthStatus {
  api: string
  sniper_running: boolean
  sniper_pid?: number
}
