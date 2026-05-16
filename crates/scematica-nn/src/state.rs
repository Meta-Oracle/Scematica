use serde::{Deserialize, Serialize};

/// Number of features in the state vector fed to the Q-network.
pub const STATE_DIM: usize = 18;

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
        ]
    }

    /// Build a state from flat data available in scematica-trades.jsonl + metrics snapshot.
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
            ..Default::default()
        }
    }
}
