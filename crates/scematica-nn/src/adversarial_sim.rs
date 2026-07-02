//! Adversarial pool simulator — a generative digital-twin of Solana launch pools.
//!
//! Live sniping is a brutal teacher: most of what the agent needs to learn (how a
//! rug *feels* in the moments before it drains, how a honeypot silently traps
//! capital) is exactly the experience that costs real SOL to acquire. This module
//! synthesizes those hostile scenarios **offline**, as a gym-style RL environment,
//! so the agent can be hardened against them *before* it ever touches mainnet.
//!
//! The generative process is parameterised by a [`ScarProfile`] — aggregate
//! statistics of how pools actually fail. In production that profile is derived
//! from the **Scar Market** (`scemadex-sdk`), which certifies verified failure
//! records from slashed conviction bonds: the only un-fakeable failure data on
//! chain. Feeding real scar statistics in means the simulated adversary tracks
//! the *current* meta of how deployers are rugging, not a static caricature.
//!
//! ```text
//! Scar Market (verified failures) ─► ScarProfile ─► AdversarialPoolSim ─► DQNAgent
//! ```
//!
//! Uses only in-crate types (`TradeState`, `TradeAction`, `shape_reward`), so it
//! adds no dependencies and can drive either the scalar or distributional agent
//! and its world model.

use crate::action::TradeAction;
use crate::state::TradeState;
use rand::Rng;
use serde::{Deserialize, Serialize};

/// The kinds of pool lifecycle the simulator can generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PoolArchetype {
    /// Healthy pool: gentle drift up with noise, LP burned, mint renounced.
    Legit,
    /// Fast pump then a hard dump — profitable only for a fast exit.
    PumpDump,
    /// Rises to lure buyers, then the deployer pulls liquidity to ~zero.
    Rug,
    /// Price looks fine but sells fail — proceeds ≈ 0, capital trapped.
    Honeypot,
    /// Slow persistent bleed — no single crash, just relentless decline.
    SlowBleed,
}

/// Aggregate failure statistics that shape the generated pool population.
///
/// Rates are fractions of the pool population and must sum to ≤ 1.0 (the
/// remainder is [`PoolArchetype::Legit`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScarProfile {
    pub rug_rate: f64,
    pub honeypot_rate: f64,
    pub pump_dump_rate: f64,
    pub slow_bleed_rate: f64,
    /// Mean seconds from pool open to the rug pull.
    pub mean_time_to_rug_secs: f64,
    /// Mean peak gain (fraction, e.g. 0.5 = +50%) reached before a rug/dump.
    pub mean_peak_before_rug_pct: f64,
    /// Mean final gain (fraction) of a legit pool over the episode.
    pub mean_legit_return_pct: f64,
    /// Per-step multiplicative price noise stddev.
    pub volatility: f64,
}

impl Default for ScarProfile {
    /// A realistic-but-hostile default distribution (roughly the memecoin meta:
    /// most launches fail one way or another). Tune from live scar data.
    fn default() -> Self {
        Self {
            rug_rate: 0.35,
            honeypot_rate: 0.10,
            pump_dump_rate: 0.30,
            slow_bleed_rate: 0.10,
            // remainder 0.15 → legit
            mean_time_to_rug_secs: 180.0,
            mean_peak_before_rug_pct: 0.6,
            mean_legit_return_pct: 0.8,
            volatility: 0.06,
        }
    }
}

impl ScarProfile {
    /// Build a profile from observed scar counts (as the Scar Market would
    /// surface them) plus measured means. `legit` is the count of pools that did
    /// *not* fail. Rates are normalised over the total population.
    #[allow(clippy::too_many_arguments)]
    pub fn from_observations(
        rug: u64,
        honeypot: u64,
        pump_dump: u64,
        slow_bleed: u64,
        legit: u64,
        mean_time_to_rug_secs: f64,
        mean_peak_before_rug_pct: f64,
        mean_legit_return_pct: f64,
        volatility: f64,
    ) -> Self {
        let total = (rug + honeypot + pump_dump + slow_bleed + legit).max(1) as f64;
        Self {
            rug_rate: rug as f64 / total,
            honeypot_rate: honeypot as f64 / total,
            pump_dump_rate: pump_dump as f64 / total,
            slow_bleed_rate: slow_bleed as f64 / total,
            mean_time_to_rug_secs: mean_time_to_rug_secs.max(1.0),
            mean_peak_before_rug_pct: mean_peak_before_rug_pct.max(0.0),
            mean_legit_return_pct,
            volatility: volatility.clamp(0.0, 1.0),
        }
    }

    fn sample_archetype(&self, rng: &mut impl Rng) -> PoolArchetype {
        let r: f64 = rng.gen();
        let mut acc = self.rug_rate;
        if r < acc {
            return PoolArchetype::Rug;
        }
        acc += self.honeypot_rate;
        if r < acc {
            return PoolArchetype::Honeypot;
        }
        acc += self.pump_dump_rate;
        if r < acc {
            return PoolArchetype::PumpDump;
        }
        acc += self.slow_bleed_rate;
        if r < acc {
            return PoolArchetype::SlowBleed;
        }
        PoolArchetype::Legit
    }
}

/// Result of one environment step.
pub struct StepResult {
    pub state: TradeState,
    pub reward: f64,
    pub done: bool,
}

/// A gym-style environment producing adversarial pool episodes.
///
/// Convention: one *episode* = one pool. The agent observes the pool, may buy
/// (entering a position), then manages the exit as the price path unfolds
/// according to the sampled archetype. Reward is realised on sell / episode end
/// via [`crate::agent::DQNAgent::shape_reward`], on the same `/100` scale the
/// live observer loop uses.
pub struct AdversarialPoolSim {
    profile: ScarProfile,
    // ── Current episode state ────────────────────────────────────────────────
    archetype: PoolArchetype,
    step_idx: u32,
    max_steps: u32,
    time_to_rug_steps: u32,
    peak_pct: f64,
    price: f64,      // as a multiple of entry-reference (1.0 = flat)
    peak_seen: f64,  // running max of price
    liquidity_sol: f64,
    in_position: bool,
    entry_price: f64,
    position_frac: f64, // fraction of position still held (1.0 → 0.0)
    realized_pnl_pct: f64,
    done: bool,
}

impl AdversarialPoolSim {
    /// One simulated step ≈ one minute, so `hold_steps` maps cleanly to the
    /// minute-based timing bonus in `shape_reward`.
    const STEP_SECS: f64 = 60.0;

    pub fn new(profile: ScarProfile) -> Self {
        Self {
            profile,
            archetype: PoolArchetype::Legit,
            step_idx: 0,
            max_steps: 30,
            time_to_rug_steps: 3,
            peak_pct: 0.0,
            price: 1.0,
            peak_seen: 1.0,
            liquidity_sol: 5.0,
            in_position: false,
            entry_price: 1.0,
            position_frac: 0.0,
            realized_pnl_pct: 0.0,
            done: false,
        }
    }

    pub fn profile(&self) -> &ScarProfile {
        &self.profile
    }

    pub fn current_archetype(&self) -> PoolArchetype {
        self.archetype
    }

    /// Start a new pool episode, sampling the archetype from the profile.
    pub fn reset(&mut self) -> TradeState {
        let mut rng = rand::thread_rng();
        let archetype = self.profile.sample_archetype(&mut rng);
        self.reset_with_archetype(archetype)
    }

    /// Start a new episode with a specific archetype (deterministic; for tests
    /// and targeted curriculum training).
    pub fn reset_with_archetype(&mut self, archetype: PoolArchetype) -> TradeState {
        let mut rng = rand::thread_rng();
        self.archetype = archetype;
        self.step_idx = 0;
        self.price = 1.0;
        self.peak_seen = 1.0;
        self.in_position = false;
        self.entry_price = 1.0;
        self.position_frac = 0.0;
        self.realized_pnl_pct = 0.0;
        self.done = false;
        self.max_steps = 30;
        self.liquidity_sol = rng.gen_range(2.0..20.0);

        // Randomise archetype-specific dynamics around the profile means.
        let ttr = (self.profile.mean_time_to_rug_secs / Self::STEP_SECS
            * rng.gen_range(0.5..1.5))
        .round()
        .max(1.0) as u32;
        self.time_to_rug_steps = ttr.min(self.max_steps - 1);
        self.peak_pct = match archetype {
            PoolArchetype::PumpDump | PoolArchetype::Rug => {
                (self.profile.mean_peak_before_rug_pct * rng.gen_range(0.5..1.5)).max(0.05)
            }
            PoolArchetype::Legit => {
                (self.profile.mean_legit_return_pct * rng.gen_range(0.5..1.5)).max(0.0)
            }
            _ => rng.gen_range(0.0..0.2),
        };

        self.build_state()
    }

    /// Advance the pool one step given the agent's action, returning the new
    /// observation, realised reward, and whether the episode is over.
    pub fn step(&mut self, action: TradeAction) -> StepResult {
        if self.done {
            return StepResult {
                state: self.build_state(),
                reward: 0.0,
                done: true,
            };
        }

        let mut rng = rand::thread_rng();
        let mut reward = 0.0;

        // ── Entry ─────────────────────────────────────────────────────────────
        if !self.in_position && action.is_buy() {
            self.in_position = true;
            self.entry_price = self.price;
            self.position_frac = 1.0;
        }

        // ── Price dynamics for this step (archetype-driven) ────────────────────
        self.advance_price(&mut rng);

        // ── Exit handling ──────────────────────────────────────────────────────
        if self.in_position && action.is_sell() {
            let sell_frac = match action {
                TradeAction::SellPartial => 0.5_f64.min(self.position_frac),
                TradeAction::SellAll => self.position_frac,
                _ => 0.0,
            };
            if sell_frac > 0.0 {
                reward += self.realize(sell_frac);
            }
        }

        self.step_idx += 1;

        // ── Terminal conditions ────────────────────────────────────────────────
        let rugged = matches!(self.archetype, PoolArchetype::Rug)
            && self.step_idx >= self.time_to_rug_steps;
        let timed_out = self.step_idx >= self.max_steps;
        let flat = self.in_position && self.position_frac <= 1e-6;

        if rugged || timed_out || flat {
            // Force-liquidate any remaining position at the (possibly crashed) price.
            if self.in_position && self.position_frac > 1e-6 {
                reward += self.realize(self.position_frac);
            }
            self.done = true;
        }

        StepResult {
            state: self.build_state(),
            reward,
            done: self.done,
        }
    }

    /// Evolve `self.price` one step according to the archetype.
    fn advance_price(&mut self, rng: &mut impl Rng) {
        let noise = 1.0 + rng.gen_range(-self.profile.volatility..self.profile.volatility);
        let t = self.step_idx as f64;
        match self.archetype {
            PoolArchetype::Legit => {
                // Approach the target return smoothly.
                let target = 1.0 + self.peak_pct;
                self.price += (target - self.price) * 0.15;
                self.price *= noise;
            }
            PoolArchetype::PumpDump => {
                let peak_step = (self.time_to_rug_steps as f64).max(1.0);
                if t < peak_step {
                    let target = 1.0 + self.peak_pct;
                    self.price += (target - self.price) * 0.5;
                } else {
                    // Dump: decay toward a deep loss.
                    self.price += (0.2 - self.price) * 0.5;
                }
                self.price *= noise;
            }
            PoolArchetype::Rug => {
                if self.step_idx + 1 >= self.time_to_rug_steps {
                    self.price = 0.02; // liquidity pulled → ~worthless
                } else {
                    let target = 1.0 + self.peak_pct;
                    self.price += (target - self.price) * 0.5;
                    self.price *= noise;
                }
            }
            PoolArchetype::Honeypot => {
                // Quoted price looks fine (small drift up), but sells won't fill —
                // handled in `realize`. Keep the observation deceptively healthy.
                self.price += (1.0 + self.peak_pct - self.price) * 0.2;
                self.price *= noise;
            }
            PoolArchetype::SlowBleed => {
                self.price *= 0.94 * noise;
            }
        }
        self.price = self.price.max(0.0);
        self.peak_seen = self.peak_seen.max(self.price);
    }

    /// Realise `frac` of the position at the current price and return the shaped
    /// reward for that portion. Honeypots fill at ≈0 regardless of quoted price.
    fn realize(&mut self, frac: f64) -> f64 {
        let effective_price = if matches!(self.archetype, PoolArchetype::Honeypot) {
            self.entry_price * 0.02 // sell reverts / dust out
        } else {
            self.price
        };
        let pnl_frac = (effective_price - self.entry_price) / self.entry_price;
        let pnl_pct = pnl_frac * 100.0;
        self.realized_pnl_pct = pnl_pct;
        self.position_frac = (self.position_frac - frac).max(0.0);
        let hold_minutes = self.step_idx; // 1 step ≈ 1 minute
        // Scale by fraction sold and normalise like the live observer (/100).
        frac * crate::agent::DQNAgent::shape_reward(pnl_pct, hold_minutes) / 100.0
    }

    /// Project the current pool + position into the agent's 24-feature state.
    fn build_state(&self) -> TradeState {
        let price_change_pct = self.price - 1.0;
        let cur_pnl = if self.in_position {
            (self.price - self.entry_price) / self.entry_price
        } else {
            0.0
        };
        // Deceptive signals: honeypots/rugs advertise health (renounced, burned).
        let looks_safe = !matches!(self.archetype, PoolArchetype::SlowBleed);
        TradeState {
            pool_age_secs: self.step_idx as f64 * Self::STEP_SECS,
            initial_liquidity_sol: self.liquidity_sol,
            price_change_pct,
            volume_5min_sol: self.liquidity_sol * (0.5 + price_change_pct.abs()),
            buy_sell_ratio: if matches!(self.archetype, PoolArchetype::Honeypot) {
                20.0 // no sells clearing is a honeypot tell
            } else {
                2.0
            },
            lp_burned: looks_safe,
            mint_renounced: looks_safe,
            current_pnl_pct: cur_pnl,
            position_age_secs: if self.in_position {
                self.step_idx as f64 * Self::STEP_SECS
            } else {
                0.0
            },
            sol_balance_sol: 5.0,
            regime: 1,
            volatility: self.profile.volatility.min(1.0),
            open_positions: if self.in_position { 1 } else { 0 },
            peak_pnl_pct: (self.peak_seen - 1.0).max(0.0),
            pool_score_norm: 0.5,
            // A high advertised rug-rate is the honest tell the agent must learn.
            deployer_rug_rate: match self.archetype {
                PoolArchetype::Rug | PoolArchetype::Honeypot => 0.7,
                PoolArchetype::PumpDump => 0.4,
                _ => 0.1,
            },
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::STATE_DIM;

    #[test]
    fn profile_from_observations_normalises_rates() {
        let p = ScarProfile::from_observations(35, 10, 30, 10, 15, 180.0, 0.6, 0.8, 0.06);
        let sum = p.rug_rate + p.honeypot_rate + p.pump_dump_rate + p.slow_bleed_rate;
        assert!(sum <= 1.0 + 1e-9);
        assert!((p.rug_rate - 0.35).abs() < 1e-9);
    }

    #[test]
    fn reset_yields_full_state_vector() {
        let mut sim = AdversarialPoolSim::new(ScarProfile::default());
        let s = sim.reset();
        assert_eq!(s.to_vec().len(), STATE_DIM);
    }

    #[test]
    fn episode_terminates_within_max_steps() {
        let mut sim = AdversarialPoolSim::new(ScarProfile::default());
        sim.reset_with_archetype(PoolArchetype::Legit);
        let mut steps = 0;
        loop {
            let r = sim.step(TradeAction::Hold);
            steps += 1;
            if r.done {
                break;
            }
            assert!(steps < 100, "episode should terminate");
        }
        assert!(steps <= 30);
    }

    #[test]
    fn holding_a_rug_to_the_end_loses_money() {
        // Buy, then hold through the rug → forced liquidation at ~0 → negative reward.
        let mut sim = AdversarialPoolSim::new(ScarProfile::default());
        sim.reset_with_archetype(PoolArchetype::Rug);
        let mut total = 0.0;
        // First step buys; subsequent steps hold.
        let mut action = TradeAction::BuyStandard;
        loop {
            let r = sim.step(action);
            total += r.reward;
            action = TradeAction::Hold;
            if r.done {
                break;
            }
        }
        assert!(total < 0.0, "holding a rug should lose money, got {total}");
    }

    #[test]
    fn honeypot_sell_does_not_recover_capital() {
        // Even actively selling, a honeypot fills at ~0 → a loss.
        let mut sim = AdversarialPoolSim::new(ScarProfile::default());
        sim.reset_with_archetype(PoolArchetype::Honeypot);
        let mut total = 0.0;
        let mut action = TradeAction::BuyStandard;
        let mut i = 0;
        loop {
            let r = sim.step(action);
            total += r.reward;
            // Try to bail after entering.
            action = if i >= 1 { TradeAction::SellAll } else { TradeAction::Hold };
            i += 1;
            if r.done {
                break;
            }
        }
        assert!(total < 0.0, "honeypot should trap capital, got {total}");
    }
}
