use std::sync::Mutex;

use async_trait::async_trait;
use scematica_nn::{DQNAgent, TradeState};

use crate::error::{Result, ScemaDexError};
use crate::intent::{Intent, Objective};
use crate::policy::{Conviction, RoutePolicy, Solution};
use crate::route::{Route, RouteLeg, Venue};

/// Live pool telemetry mapped onto the agent's 24-feature state vector. Feed
/// this from the bot's listener/scorer so the DQ* policy reasons about real
/// market conditions (liquidity, momentum, deployer reputation) rather than
/// neutral defaults.
#[derive(Clone, Debug, Default)]
pub struct PoolTelemetry {
    pub liquidity_sol: f64,
    /// Fractional price change since pool creation (e.g. `0.5` = +50%).
    pub price_change_pct: f64,
    pub volume_5min_sol: f64,
    pub buy_sell_ratio: f64,
    /// Predictive pool quality, already normalized to `[0,1]`.
    pub pool_score_norm: f64,
    /// EMA deployer rug rate `[0,1]` from the reputation ledger.
    pub deployer_rug_rate: f64,
    pub volume_velocity: f64,
    pub price_velocity: f64,
    pub price_acceleration: f64,
    pub volatility: f64,
}

/// A [`RoutePolicy`] backed by the live Scematica Deep Q* agent.
///
/// The agent's value head supplies the **conviction** that sizes the Conviction
/// Routing bond, turning the SDK's metered-inference primitive into the real
/// learned policy instead of the reference stub. `advise()` needs `&mut self`,
/// so the agent is held behind a `Mutex`; the latest [`PoolTelemetry`] is held
/// the same way and merged into the state at solve time.
pub struct DqRoutePolicy {
    agent: Mutex<DQNAgent>,
    telemetry: Mutex<Option<PoolTelemetry>>,
    /// Venue the single-leg route targets when the agent only emits a sizing
    /// action. A richer integration maps actions onto multi-venue splits.
    venue: Venue,
}

impl DqRoutePolicy {
    pub fn new(agent: DQNAgent) -> Self {
        Self {
            agent: Mutex::new(agent),
            telemetry: Mutex::new(None),
            venue: Venue::Jupiter,
        }
    }

    /// Load a trained agent from a checkpoint (e.g. `scematica-nn-agent.json`).
    pub fn from_checkpoint(path: &str) -> Result<Self> {
        let agent = DQNAgent::load(path)
            .map_err(|e| ScemaDexError::Other(format!("load DQ* checkpoint: {e}")))?;
        Ok(Self::new(agent))
    }

    pub fn with_venue(mut self, venue: Venue) -> Self {
        self.venue = venue;
        self
    }

    /// Push the latest pool telemetry; the next `solve()` reasons over it.
    pub fn update_telemetry(&self, telemetry: PoolTelemetry) {
        if let Ok(mut slot) = self.telemetry.lock() {
            *slot = Some(telemetry);
        }
    }

    /// Project an intent onto the agent's trade-state vector, merging the latest
    /// live telemetry when present. Falls back to objective-biased defaults.
    fn state_for(&self, intent: &Intent) -> TradeState {
        let mut state = TradeState {
            initial_liquidity_sol: intent.amount_in.ui(),
            pool_score_norm: match intent.objective {
                Objective::Price => 0.6,
                Objective::Speed => 0.5,
                Objective::Stealth => 0.7,
            },
            deployer_rug_rate: 0.5,
            ..Default::default()
        };
        if let Some(t) = self.telemetry.lock().ok().and_then(|g| g.clone()) {
            state.initial_liquidity_sol = t.liquidity_sol.max(state.initial_liquidity_sol);
            state.price_change_pct = t.price_change_pct;
            state.volume_5min_sol = t.volume_5min_sol;
            state.buy_sell_ratio = t.buy_sell_ratio;
            state.pool_score_norm = t.pool_score_norm;
            state.deployer_rug_rate = t.deployer_rug_rate;
            state.volume_velocity = t.volume_velocity;
            state.price_velocity = t.price_velocity;
            state.price_acceleration = t.price_acceleration;
            state.volatility = t.volatility;
        }
        state
    }
}

#[async_trait]
impl RoutePolicy for DqRoutePolicy {
    async fn solve(&self, intent: &Intent) -> Result<Solution> {
        let state = self.state_for(intent);
        let (action, q_values) = {
            let mut agent = self
                .agent
                .lock()
                .map_err(|_| ScemaDexError::Other("DQ* agent lock poisoned".into()))?;
            agent.advise(&state)
        };

        // Conviction = best_q / sum|q| — the agent's own confidence metric, [0,1].
        let sum_abs: f64 = q_values.iter().map(|v| v.abs()).sum();
        let best = q_values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(f64::NEG_INFINITY, f64::max);
        let conviction = if sum_abs > 1e-9 && best.is_finite() {
            (best.abs() / sum_abs).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let leg = RouteLeg {
            venue: self.venue,
            input_mint: intent.input_mint.clone(),
            output_mint: intent.output_mint.clone(),
            split_bps: 10_000,
            expected_out: intent.amount_in,
        };
        Ok(Solution {
            intent_digest: intent.digest(),
            route: Route {
                legs: vec![leg],
                expected_out: intent.amount_in,
            },
            conviction: Conviction::clamped(conviction),
            rationale: format!(
                "DQ* action={} conviction={conviction:.2}",
                action.label()
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo_intent;

    #[tokio::test]
    async fn dq_policy_produces_valid_solution() {
        let policy = DqRoutePolicy::new(DQNAgent::default());
        let sol = policy.solve(&demo_intent()).await.unwrap();
        assert!(sol.route.splits_valid());
        assert!((0.0..=1.0).contains(&sol.conviction.0));
        assert!(sol.rationale.starts_with("DQ*"));
    }

    #[tokio::test]
    async fn telemetry_is_accepted_and_solve_still_valid() {
        let policy = DqRoutePolicy::new(DQNAgent::default());
        policy.update_telemetry(PoolTelemetry {
            liquidity_sol: 42.0,
            price_change_pct: 0.3,
            volume_5min_sol: 12.0,
            buy_sell_ratio: 2.5,
            pool_score_norm: 0.85,
            deployer_rug_rate: 0.1,
            volume_velocity: 0.4,
            price_velocity: 0.2,
            price_acceleration: 0.1,
            volatility: 0.2,
        });
        let sol = policy.solve(&demo_intent()).await.unwrap();
        assert!(sol.route.splits_valid());
    }
}
