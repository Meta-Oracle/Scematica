use serde::{Deserialize, Serialize};

/// Number of features in the state vector fed to the Q-network.
/// v1.1.0: expanded from 18 → 24 with peak PnL, pool quality, deployer rug rate,
/// volume velocity, price velocity, and price acceleration.
pub const STATE_DIM: usize = 24;

/// Market + position context captured at decision time.
/// All numeric fields use real-world units; `to_vec()` normalises them to [0, 1].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TradeState {
    /// How long the pool has been live (seconds).
    pub pool_age_secs: f64,
    /// SOL liquidity deposited at pool creation.
    pub initial_liquidity_sol: f64,
    /// Price change since pool creation (fractional, e.g. 0.5 = +50%).
    pub price_change_pct: f64,
    /// SOL volume traded in the last 5 minutes.
    pub volume_5min_sol: f64,
    /// Ratio of buy txs to sell txs in recent window.
    pub buy_sell_ratio: f64,
    /// LP tokens burned — strong safety signal.
    pub lp_burned: bool,
    /// Mint authority renounced — token cannot be inflated.
    pub mint_renounced: bool,
    /// Unrealised PnL on the current position (fractional).
    pub current_pnl_pct: f64,
    /// How long the current position has been open (seconds).
    pub position_age_secs: f64,
    /// Cumulative PnL for the day in SOL (can be negative).
    pub daily_pnl_sol: f64,
    /// Win streak length (positive) at this point in time.
    pub consecutive_wins: i32,
    /// Loss streak length (positive) at this point in time.
    pub consecutive_losses: i32,
    /// Current wallet SOL balance.
    pub sol_balance_sol: f64,
    /// Market regime: -1 bear, 0 neutral, 1 bull.
    pub regime: i32,
    /// Recent price volatility (std-dev / mean, unitless).
    pub volatility: f64,
    /// Bid-ask spread as a fraction of price.
    pub spread_pct: f64,
    /// UTC hour normalised to [0, 1].
    pub time_of_day_norm: f64,
    /// Number of open positions.
    pub open_positions: i32,

    // ── v1.1.0 new features ───────────────────────────────────────────────────

    /// Highest PnL seen since position entry (fractional, e.g. 0.8 = 80% peak).
    /// Enables the agent to reason about exit efficiency: how far off peak is the
    /// current exit?  0.0 if no position is open.
    pub peak_pnl_pct: f64,

    /// Pool predictive quality score normalised to [0, 1] (raw 0–100).
    /// Lets the agent learn to be more/less aggressive based on pool quality.
    pub pool_score_norm: f64,

    /// EMA deployer rug rate from the reputation ledger, [0, 1].
    /// 0 = no rugs recorded, 1 = all rugs. Defaults to 0.5 if unknown.
    pub deployer_rug_rate: f64,

    /// Rate of change of volume_5min_sol between the last two observations.
    /// Positive = volume growing (pump phase), negative = drying up (dump risk).
    /// Normalised: raw delta / 20 SOL, clamped to [-1, 1].
    pub volume_velocity: f64,

    /// First derivative of price_change_pct between consecutive observations.
    /// Positive = accelerating upward, negative = decelerating / reversing.
    /// Clamped to [-1, 1].
    pub price_velocity: f64,

    /// Second derivative of price_change_pct (change in velocity).
    /// Positive = still accelerating, negative = inflection point (momentum fading).
    /// Clamped to [-1, 1].
    pub price_acceleration: f64,
}

impl TradeState {
    /// Returns a normalised `[0, 1]` vector of length `STATE_DIM`.
    pub fn to_vec(&self) -> Vec<f64> {
        vec![
            (self.pool_age_secs / 3_600.0).min(1.0),
            (self.initial_liquidity_sol / 100.0).min(1.0),
            self.price_change_pct.clamp(-1.0, 3.0) / 3.0,
            (self.volume_5min_sol / 50.0).min(1.0),
            (self.buy_sell_ratio / 5.0).min(1.0),
            if self.lp_burned { 1.0 } else { 0.0 },
            if self.mint_renounced { 1.0 } else { 0.0 },
            self.current_pnl_pct.clamp(-1.0, 2.0) / 2.0 + 0.5,
            (self.position_age_secs / 3_600.0).min(1.0),
            self.daily_pnl_sol.clamp(-2.0, 2.0) / 2.0 + 0.5,
            (self.consecutive_wins as f64 / 10.0).min(1.0),
            (self.consecutive_losses as f64 / 10.0).min(1.0),
            (self.sol_balance_sol / 10.0).min(1.0),
            (self.regime as f64 + 1.0) / 2.0,
            self.volatility.clamp(0.0, 1.0),
            (self.spread_pct / 0.1).min(1.0),
            self.time_of_day_norm.clamp(0.0, 1.0),
            (self.open_positions as f64 / 5.0).min(1.0),
            // v1.1.0 features
            self.peak_pnl_pct.clamp(0.0, 5.0) / 5.0,
            self.pool_score_norm.clamp(0.0, 1.0),
            self.deployer_rug_rate.clamp(0.0, 1.0),
            self.volume_velocity.clamp(-1.0, 1.0) * 0.5 + 0.5,
            self.price_velocity.clamp(-1.0, 1.0) * 0.5 + 0.5,
            self.price_acceleration.clamp(-1.0, 1.0) * 0.5 + 0.5,
        ]
    }

    /// Build a state from flat data available in scematica-trades.jsonl + metrics snapshot.
    /// New v1.1.0 fields default to zero / neutral when not provided by the replay loop.
    pub fn from_trade_fields(
        pnl_pct: f64,
        position_age_secs: f64,
        daily_pnl_sol: f64,
        consecutive_wins: i32,
        consecutive_losses: i32,
        sol_balance_sol: f64,
        open_positions: i32,
    ) -> Self {
        use chrono::Timelike;
        let hour = chrono::Utc::now().hour() as f64;
        Self {
            current_pnl_pct: pnl_pct,
            position_age_secs,
            daily_pnl_sol,
            consecutive_wins,
            consecutive_losses,
            sol_balance_sol,
            open_positions,
            time_of_day_norm: hour / 24.0,
            deployer_rug_rate: 0.5, // neutral unknown
            ..Default::default()
        }
    }

    /// Build a rich state during live trading with all v1.1.0 fields populated.
    pub fn from_live_fields(
        pnl_pct: f64,
        peak_pnl_pct: f64,
        position_age_secs: f64,
        daily_pnl_sol: f64,
        consecutive_wins: i32,
        consecutive_losses: i32,
        sol_balance_sol: f64,
        open_positions: i32,
        pool_score_norm: f64,
        deployer_rug_rate: f64,
        volume_velocity: f64,
        price_velocity: f64,
        price_acceleration: f64,
    ) -> Self {
        use chrono::Timelike;
        let hour = chrono::Utc::now().hour() as f64;
        Self {
            current_pnl_pct: pnl_pct,
            peak_pnl_pct,
            position_age_secs,
            daily_pnl_sol,
            consecutive_wins,
            consecutive_losses,
            sol_balance_sol,
            open_positions,
            pool_score_norm,
            deployer_rug_rate,
            volume_velocity,
            price_velocity,
            price_acceleration,
            time_of_day_norm: hour / 24.0,
            ..Default::default()
        }
    }
}
