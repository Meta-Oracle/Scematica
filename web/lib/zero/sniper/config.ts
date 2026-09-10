// The sniper's configuration, as the sniper actually runs it.
//
// ⚠️  PORT. Authoritative sources, in this order:
//   • `config.toml` at the repo root — what the bot runs today.
//   • `crates/scematica-core/src/config.rs` — the `Default` impl behind it.
//
// `check:zero` asserts `SNIPER_CONFIG` equals `config_toml` in
// `fixtures/sniper-parity.json` field for field, and that the rate-mode table matches.
// Editing a number here without editing `config.toml` fails the check; editing
// `config.toml` fails `cargo test -p scematica-sniper zero_parity` until the fixture is
// regenerated. There is no third place to put a threshold.
//
// ── Why these are not "sensible defaults" ────────────────────────────────────
//
// Zero's previous config was a plausible-looking set of round numbers, and every one of
// them differed from the bot:
//
//   take profit      100  →  175      stop loss           15  →  10
//   pullback exit     25  →   15      momentum min peak  140  →  200
//   max positions      3  →    1      no-pump timeout     30  →  15
//
// None of those is a small difference. A 100% take-profit closes a position the sniper
// would still be holding through its escalation ladder; a 15% stop cuts one the sniper
// would ride out. Two bots, one name. The `no-pump` pair is the sharpest: Zero exited on
// "±5% after 30s" against the bot's "peak below 8% after 15s", which is a different
// question asked at a different time about a different quantity.

/** Every value the exit ladder and the entry gate branch on. */
export interface SniperConfig {
  quoteAmount: number
  takeProfitPct: number
  stopLossPct: number
  trailingStopLossPct: number
  buySlippagePct: number
  sellSlippagePct: number
  priceCheckIntervalMs: number
  priceCheckDurationMs: number
  /** 0 means unlimited in Rust; Zero treats 0 the same way. */
  maxConcurrentPositions: number
  minPoolScore: number
  kellySizing: boolean
  kellyFraction: number
  kellyLookback: number
  momentumHold: boolean
  momentumWindowChecks: number
  momentumEscalationThresholdPct: number
  momentumEscalationFactor: number
  momentumMaxEscalations: number
  momentumMinPeakPct: number
  momentumPullbackExitPct: number
  adaptivePullback: boolean
  velocityDecayExit: boolean
  velocityDecayWindow: number
  velocityDecayMinPnlPct: number
  velocityDecayDropThreshold: number
  tieredPartialTp: boolean
  /** `[trigger_pct, sell_pct_of_remaining]`, in order. */
  tieredPartialTpLevels: Array<[number, number]>
  flashCrashPct: number
  /** Consecutive checks above entry before the stop moves to near-breakeven. */
  profitLockChecks: number
  whaleExitVaultDropPct: number
  volumeExhaustionPct: number
  noPumpTimeoutSecs: number
  noPumpMinGainPct: number
  profitFirstMode: boolean
  profitFirstFloorPct: number
  walletTargetSol: number
  maxPositionHoldMins: number
  mintCooldownSecs: number
  coherenceBreaker: boolean
  minPoolSize: number
  maxPoolSize: number
}

/**
 * `config.toml`'s `[sniper]` section, which is what the bot on this machine runs.
 *
 * Note `adaptivePullback: false` and `tieredPartialTp: false` — both are `true` in
 * `config.rs`'s defaults and both are turned off here. The ladder in `exit-ladder.ts`
 * implements them anyway, because a rule that is off in one config and on in another is
 * still a rule, and a port that only covers the currently-enabled half stops being a port
 * the first time somebody flips a flag.
 */
export const SNIPER_CONFIG: SniperConfig = {
  quoteAmount: 0.01,
  takeProfitPct: 175.0,
  stopLossPct: 10.0,
  trailingStopLossPct: 20.0,
  buySlippagePct: 3.0,
  sellSlippagePct: 5.0,
  priceCheckIntervalMs: 250,
  priceCheckDurationMs: 1_800_000,
  maxConcurrentPositions: 1,
  minPoolScore: 65.0,
  kellySizing: false,
  kellyFraction: 0.25,
  kellyLookback: 20,
  momentumHold: true,
  momentumWindowChecks: 5,
  momentumEscalationThresholdPct: 8.0,
  momentumEscalationFactor: 1.8,
  momentumMaxEscalations: 6,
  momentumMinPeakPct: 200.0,
  momentumPullbackExitPct: 15.0,
  adaptivePullback: false,
  velocityDecayExit: true,
  velocityDecayWindow: 3,
  velocityDecayMinPnlPct: 100.0,
  velocityDecayDropThreshold: 1.5,
  tieredPartialTp: false,
  tieredPartialTpLevels: [
    [100.0, 15.0],
    [300.0, 20.0],
    [600.0, 25.0],
  ],
  flashCrashPct: 0.0,
  profitLockChecks: 0,
  whaleExitVaultDropPct: 0.0,
  volumeExhaustionPct: 0.0,
  noPumpTimeoutSecs: 15,
  noPumpMinGainPct: 8.0,
  profitFirstMode: true,
  profitFirstFloorPct: 25.0,
  walletTargetSol: 0.15,
  maxPositionHoldMins: 60,
  mintCooldownSecs: 1800,
  coherenceBreaker: true,
  minPoolSize: 10.0,
  maxPoolSize: 150.0,
}

// ── rate modes ───────────────────────────────────────────────────────────────

/**
 * One of `config.rs`'s seven `RateMode` profiles.
 *
 * Zero had none of these, which meant its single hard-coded TP/SL pair matched no mode
 * the operator could actually select on the dashboard. The dashboard writes
 * `scematica-rate-mode.json` and the sniper hot-reloads it; Zero has no such file to
 * read, so the mode is a local selection — but the *table* is the bot's, so picking
 * "Degen" in a browser tab means what picking it in the TUI means.
 */
export interface RateMode {
  name: string
  order: number
  quoteAmount: number
  walletPct: number
  takeProfitPct: number
  stopLossPct: number
  momentumMaxEscalations: number
  enabled: boolean
}

export const RATE_MODES: RateMode[] = [
  { name: 'Micro',      order: 1, quoteAmount: 0.005, walletPct: 0.3,  takeProfitPct: 50.0,      stopLossPct: 8.0,  momentumMaxEscalations: 3,  enabled: true },
  { name: 'Bearish',    order: 2, quoteAmount: 0.003, walletPct: 0.5,  takeProfitPct: 75.0,      stopLossPct: 10.0, momentumMaxEscalations: 6,  enabled: true },
  { name: 'Safe',       order: 3, quoteAmount: 0.005, walletPct: 0.8,  takeProfitPct: 100.0,     stopLossPct: 12.0, momentumMaxEscalations: 8,  enabled: true },
  { name: 'Balanced',   order: 4, quoteAmount: 0.01,  walletPct: 1.5,  takeProfitPct: 175.0,     stopLossPct: 12.0, momentumMaxEscalations: 6,  enabled: true },
  { name: 'Aggressive', order: 5, quoteAmount: 0.02,  walletPct: 3.0,  takeProfitPct: 300.0,     stopLossPct: 15.0, momentumMaxEscalations: 12, enabled: true },
  { name: 'Degen',      order: 6, quoteAmount: 0.04,  walletPct: 6.0,  takeProfitPct: 450.0,     stopLossPct: 25.0, momentumMaxEscalations: 12, enabled: true },
  { name: 'Moon',       order: 7, quoteAmount: 0.1,   walletPct: 12.0, takeProfitPct: 100000.0,  stopLossPct: 60.0, momentumMaxEscalations: 12, enabled: true },
]

export const ACTIVE_MODE_NAME = 'Balanced'

/**
 * Apply a rate mode, exactly as the sniper's `live_params` watcher does.
 *
 * A mode overrides four fields and nothing else — notably NOT `momentum_min_peak_pct` or
 * `momentum_pullback_exit_pct`, which is why `configProblem` has to be re-checked after
 * a switch rather than only at startup. Micro's 50% take-profit against a 200% momentum
 * floor is a legal configuration in which the pullback exit can never fire, and the bot
 * ships that way.
 */
export function withRateMode(config: SniperConfig, modeName: string): SniperConfig {
  const mode = RATE_MODES.find(m => m.name === modeName)
  if (!mode) return config
  return {
    ...config,
    quoteAmount: mode.quoteAmount,
    takeProfitPct: mode.takeProfitPct,
    stopLossPct: mode.stopLossPct,
    momentumMaxEscalations: mode.momentumMaxEscalations,
  }
}

/**
 * Relationships that must hold, checked rather than documented.
 *
 * `momentum_min_peak_pct` must exceed `take_profit_pct + momentum_pullback_exit_pct`, or
 * the pullback exit is unsatisfiable: the peak arms only above the momentum floor, and
 * `exit_gate_met` blocks it below the take-profit, so any position high enough to arm it
 * has already closed. That relationship has been broken in the Rust config before, which
 * is why it is an assertion.
 *
 * Returns a WARNING rather than an error, and the distinction is load-bearing: the
 * shipped rate modes violate it (Micro's 50 + 15 against a floor of 200), so refusing
 * would make five of seven selectable modes unusable. The bot runs in that state; Zero
 * reports it and runs too. What it must not do is fire a rule it knows cannot fire and
 * let the operator believe otherwise.
 */
export function configProblem(c: SniperConfig): string | null {
  if (c.stopLossPct <= 0 || c.stopLossPct >= 100) return 'stopLossPct must be within (0, 100)'
  if (c.maxConcurrentPositions < 0) return 'maxConcurrentPositions cannot be negative'
  if (c.momentumHold && c.momentumMinPeakPct <= c.takeProfitPct + c.momentumPullbackExitPct) {
    return (
      `pullback exit unreachable: momentumMinPeakPct (${c.momentumMinPeakPct}) must exceed ` +
      `takeProfitPct + momentumPullbackExitPct (${c.takeProfitPct + c.momentumPullbackExitPct}). ` +
      'The position takes profit before the peak can arm the rule.'
    )
  }
  return null
}
