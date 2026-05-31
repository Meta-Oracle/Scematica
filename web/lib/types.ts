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

export interface NNAdvice {
  action: string
  action_index: number
  q_values: [string, number][]
  top_reason: string
  confidence: number
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

export interface PoolDecision {
  timestamp: string
  mint: string
  pool: string
  quote_mint: string
  decision: 'accepted' | 'rejected' | 'ignored' | string
  stage: string
  reason: string
  pool_size_sol: number
  pool_age_secs: number
  velocity_sol_per_sec: number
  buy_pressure_ratio: number
  pool_score: number
  pumpfun_score: number
  inflow_rate_sol_per_sec: number
  high_speed: boolean
  dex_boosted: boolean
  dex_boost_usd: number
  social_count: number
  effective_min_score: number
  dq_action: string
  dq_confidence: number
  utc_hour: number
}

export interface TxTelemetry {
  timestamp: string
  executor: string
  tx_kind: 'buy' | 'sell' | 'unknown' | string
  signature: string
  confirmed: boolean
  error: string
  attempts: number
  instruction_count: number
  compute_unit_limit: number
  compute_unit_price: number
  compute_unit_price_hard_cap: number
  loaded_accounts_data_size_limit: number
  skip_preflight: boolean
  high_speed: boolean
  elapsed_ms: number
  blockhash_fetch_ms_total: number
  send_confirm_ms_total: number
  retry_delay_ms_total: number
  timeout_count: number
  rate_limit_count: number
  slippage_error_count: number
  blockhash_error_count: number
}

export interface HealthStatus {
  api: string
  sniper_running: boolean
  sniper_pid?: number
}

export interface IntelligenceSnapshot {
  nn: NNStats
  advice: NNAdvice
  decisions: PoolDecision[]
  telemetry: TxTelemetry[]
  paths?: Record<string, string>
}
