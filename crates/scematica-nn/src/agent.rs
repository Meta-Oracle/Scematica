use crate::{
    action::{TradeAction, ACTION_DIM},
    network::QNetwork,
    replay::{PrioritizedReplayBuffer, Transition},
    state::{TradeState, STATE_DIM},
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

/// Public snapshot of agent state, written to `scematica-nn-stats.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStats {
    pub step_count: usize,
    pub train_steps: usize,
    pub epsilon: f64,
    pub replay_size: usize,
    pub total_reward: f64,
    pub avg_loss: f64,
    pub target_updates: usize,
    /// True once at least one training step has produced usable network weights.
    pub ready_to_advise: bool,
    pub last_action: Option<String>,
    pub last_q_values: Vec<f64>,
}

/// Explanation of why the agent chose an action, for Feature 3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeDecisionExplanation {
    pub action: String,
    pub action_index: usize,
    /// (action_label, q_value) pairs for every action.
    pub q_values: Vec<(String, f64)>,
    /// Human-readable explanation of the dominant Q-value.
    pub top_reason: String,
    /// max_q / sum_abs_q — how confident the network is.
    pub confidence: f64,
}

// Checkpoint — does not include replay buffer (too large to serialise)
#[derive(Serialize, Deserialize)]
struct Checkpoint {
    online_net: QNetwork,
    target_net: QNetwork,
    epsilon: f64,
    step_count: usize,
    train_steps: usize,
    total_reward: f64,
    target_updates: usize,
    /// Regime-specific network pairs, keyed by regime label.
    regime_nets: HashMap<String, (QNetwork, QNetwork)>,
    active_regime: String,
    /// State and action dimensions recorded at save time.
    /// On load, if these don't match current STATE_DIM/ACTION_DIM the checkpoint
    /// is silently discarded (weights would be wrong shapes).
    #[serde(default)]
    state_dim: usize,
    #[serde(default)]
    action_dim: usize,
}

/// Double Deep Q* agent.
///
/// Architecture: Dueling DQN — STATE_DIM → 128 → 64 → {V(s), A(s,a)}
///   Q(s,a) = V(s) + A(s,a) - mean(A(s,a))
/// Uses Double DQN: online net selects actions, target net evaluates them.
pub struct DQNAgent {
    online_net: QNetwork,
    target_net: QNetwork,
    replay: PrioritizedReplayBuffer,
    pub epsilon: f64,
    epsilon_min: f64,
    epsilon_decay: f64,
    /// Discount factor γ for future rewards.
    gamma: f64,
    lr: f64,
    batch_size: usize,
    /// How many steps between target network weight copies.
    target_update_freq: usize,
    step_count: usize,
    train_steps: usize,
    total_reward: f64,
    recent_losses: Vec<f64>,
    target_updates: usize,
    last_action: Option<TradeAction>,
    last_q_values: Vec<f64>,
    // Feature 1: regime-aware branching
    /// One (online, target) QNetwork pair per regime label.
    pub regime_nets: HashMap<String, (QNetwork, QNetwork)>,
    /// Currently active market regime label.
    pub active_regime: String,
    // Feature 2: adversarial simulation
    /// When true, train_step injects adversarial scenarios every 100 steps.
    pub auto_inject_adversarial: bool,
    /// N-step return buffer: pending transitions before multi-step target is computed.
    /// Each element is (state_vec, action_idx, reward, next_state_vec, done).
    n_step_buffer: Vec<(Vec<f64>, usize, f64, Vec<f64>, bool)>,
    /// How many steps to accumulate before computing the n-step return. Default 5.
    n_step: usize,
    /// Tournament hyperparams stored per variant for evolutionary mutation.
    pub tournament_hyperparams: Vec<(f64, f64, f64)>, // (epsilon_decay, lr, gamma)
}

impl DQNAgent {
    pub fn new() -> Self {
        Self::with_hyperparams(0.9995, 1e-3, 0.99)
    }

    /// Create an agent with custom hyper-parameters.
    /// Used by `AgentTournament` to build the conservative / balanced / aggressive variants.
    /// Uses Dueling DQN architecture: shared trunk [STATE_DIM→128→64] + V/A heads.
    pub fn with_hyperparams(epsilon_decay: f64, lr: f64, gamma: f64) -> Self {
        // Dueling DQN: shared trunk without the output layer; heads are separate.
        let trunk_sizes = [STATE_DIM, 128, 64];
        let online_net = QNetwork::new_dueling(&trunk_sizes, ACTION_DIM);
        let mut target_net = QNetwork::new_dueling(&trunk_sizes, ACTION_DIM);
        target_net.copy_from(&online_net);
        Self {
            online_net,
            target_net,
            replay: PrioritizedReplayBuffer::new(10_000),
            epsilon: 1.0,
            epsilon_min: 0.05,
            epsilon_decay,
            gamma,
            lr,
            batch_size: 64,
            target_update_freq: 200,
            step_count: 0,
            train_steps: 0,
            total_reward: 0.0,
            recent_losses: Vec::new(),
            target_updates: 0,
            last_action: None,
            last_q_values: vec![0.0; ACTION_DIM],
            regime_nets: HashMap::new(),
            active_regime: "unknown".to_string(),
            auto_inject_adversarial: false,
            n_step_buffer: Vec::new(),
            n_step: 5,
            tournament_hyperparams: vec![
                (0.9998, 5e-4, 0.995), // conservative
                (0.9995, 1e-3, 0.990), // balanced
                (0.9990, 2e-3, 0.980), // aggressive
            ],
        }
    }

    // ── Decision ────────────────────────────────────────────────────────────

    /// Epsilon-greedy action selection.
    /// When `active_regime` is a known regime AND `epsilon < 0.3`, the
    /// regime-specific online network is used; otherwise falls back to the
    /// global network.
    pub fn select_action(&mut self, state: &TradeState) -> TradeAction {
        let sv = state.to_vec();

        // Feature 1: use regime-specific net when confident enough
        let use_regime_net = self.epsilon < 0.3
            && self.active_regime != "unknown"
            && self.regime_nets.contains_key(&self.active_regime);

        let q = if use_regime_net {
            let regime = self.active_regime.clone();
            self.regime_nets[&regime].0.forward(&sv)
        } else {
            self.online_net.forward(&sv)
        };

        self.last_q_values = q.clone();

        let action = if rand::thread_rng().gen::<f64>() < self.epsilon {
            TradeAction::from_index(rand::thread_rng().gen_range(0..ACTION_DIM))
        } else {
            let best = q
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(0);
            TradeAction::from_index(best)
        };
        self.last_action = Some(action);
        action
    }

    /// Best greedy action without exploring (for advice mode, no epsilon).
    pub fn greedy_action(&self, state: &TradeState) -> (TradeAction, Vec<f64>) {
        let q = self.online_net.forward(&state.to_vec());
        let best = q
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);
        (TradeAction::from_index(best), q)
    }

    /// Greedy advice for live trading. Unlike `select_action`, this never explores,
    /// but it still records the action/Q-values so stats and the dashboard reflect
    /// the latest pool the sniper asked about.
    pub fn advise(&mut self, state: &TradeState) -> (TradeAction, Vec<f64>) {
        let (mut action, q) = self.greedy_action(state);
        if q.iter().all(|v| v.is_finite() && v.abs() <= 1e-9) {
            action = TradeAction::Hold;
        }
        self.last_q_values = q.clone();
        self.last_action = Some(action);
        (action, q)
    }

    // ── Learning ────────────────────────────────────────────────────────────

    /// Record a transition with N-step return accumulation.
    ///
    /// Transitions are buffered until `n_step` samples are collected, then a
    /// multi-step return G_t = r_t + γ·r_{t+1} + … + γ^{n-1}·r_{t+n-1} + γ^n·V(s_{t+n})
    /// is computed and pushed to the replay buffer.  For terminal transitions
    /// (`done=true`) the buffer is flushed immediately so episodes don't bleed.
    pub fn observe(
        &mut self,
        state: TradeState,
        action: TradeAction,
        reward: f64,
        next_state: TradeState,
        done: bool,
    ) {
        self.total_reward += reward;
        let sv = state.to_vec();
        let nsv = next_state.to_vec();
        self.n_step_buffer
            .push((sv, action.index(), reward, nsv, done));

        // Flush on terminal or once we have n_step samples
        if done || self.n_step_buffer.len() >= self.n_step {
            self.flush_n_step_buffer();
        }

        self.epsilon = (self.epsilon * self.epsilon_decay).max(self.epsilon_min);
        self.step_count += 1;
    }

    /// Compute multi-step returns from the pending buffer and push to replay.
    fn flush_n_step_buffer(&mut self) {
        if self.n_step_buffer.is_empty() {
            return;
        }

        // Walk from the front: each entry gets a n-step return looking forward
        let n = self.n_step_buffer.len();
        for start in 0..n {
            let (ref s0, a0, _, _, _) = self.n_step_buffer[start].clone();
            let mut g = 0.0;
            let mut gamma_k = 1.0;
            let mut terminal = false;
            let mut final_next = self.n_step_buffer[start].3.clone();
            let mut final_done = self.n_step_buffer[start].4;

            for k in start..n {
                let (_, _, rk, ref nsk, dk) = self.n_step_buffer[k].clone();
                g += gamma_k * rk;
                gamma_k *= self.gamma;
                final_next = nsk.clone();
                final_done = dk;
                if dk {
                    terminal = true;
                    break;
                }
            }

            self.replay.push(Transition {
                state: s0.clone(),
                action: a0,
                reward: g,
                next_state: final_next,
                done: terminal || final_done,
            });
        }
        self.n_step_buffer.clear();
    }

    /// Sample a prioritized mini-batch and run one Double DQN gradient step with IS weights.
    /// Updates replay priorities based on per-transition TD errors.
    /// Returns average batch loss, or `None` if the buffer is too small.
    ///
    /// Feature 1: also trains the active regime-specific network.
    /// Feature 2: injects adversarial scenarios every 100 steps when
    ///            `auto_inject_adversarial` is true.
    pub fn train_step(&mut self) -> Option<f64> {
        if self.replay.len() < self.batch_size {
            return None;
        }

        // Feature 2: periodic adversarial injection + action rebalancing
        if self.auto_inject_adversarial && self.train_steps % 100 == 0 {
            self.inject_adversarial_scenarios(2);
        }
        // Action rebalancing: inject Hold + SellPartial every 50 steps so those
        // actions remain represented even when real trades are all SellAll.
        if self.train_steps % 50 == 0 {
            self.inject_action_balance();
        }

        let batch = self.replay.sample(self.batch_size);
        let mut total_loss = 0.0;
        let mut td_errors = Vec::with_capacity(batch.transitions.len());

        // --- global network training ---
        for (t, &is_weight) in batch.transitions.iter().zip(batch.weights.iter()) {
            if t.state.is_empty() {
                td_errors.push(0.0);
                continue;
            }

            // Double DQN: online net picks best next action, target net evaluates it
            let next_q_online = self.online_net.forward(&t.next_state);
            let best_next = next_q_online
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(0);

            let current_q = self.online_net.forward(&t.state);
            let td_target = if t.done {
                t.reward
            } else {
                let nq_target = self.target_net.forward(&t.next_state);
                t.reward + self.gamma * nq_target[best_next]
            };

            let td_error = (td_target - current_q[t.action]).abs();
            td_errors.push(td_error);

            let mut targets = vec![0.0; ACTION_DIM];
            targets[t.action] = td_target;
            let mask: Vec<bool> = (0..ACTION_DIM).map(|i| i == t.action).collect();

            // Scale gradient by IS weight to correct for non-uniform sampling bias
            total_loss += self
                .online_net
                .backward_step(&t.state, &targets, &mask, self.lr, is_weight);
        }

        // Feed TD errors back to the buffer so high-surprise transitions are sampled more
        self.replay.update_priorities(&batch.indices, &td_errors);

        // Feature 1: also train the regime-specific network on the same batch
        if self.active_regime != "unknown" {
            let regime = self.active_regime.clone();
            // Ensure the pair exists; create it lazily if not
            if !self.regime_nets.contains_key(&regime) {
                let sizes = [STATE_DIM, 128, 64, ACTION_DIM];
                let online = QNetwork::new(&sizes);
                let mut target = QNetwork::new(&sizes);
                target.copy_from(&online);
                self.regime_nets.insert(regime.clone(), (online, target));
            }

            // Re-sample a smaller batch for the regime net (reuse existing sample)
            let regime_batch = self.replay.sample(self.batch_size.min(32));
            let (regime_online, regime_target) = self.regime_nets.get_mut(&regime).unwrap();

            for (t, &is_weight) in regime_batch
                .transitions
                .iter()
                .zip(regime_batch.weights.iter())
            {
                if t.state.is_empty() {
                    continue;
                }
                let next_q_online = regime_online.forward(&t.next_state);
                let best_next = next_q_online
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let _current_q = regime_online.forward(&t.state);
                let td_target = if t.done {
                    t.reward
                } else {
                    let nq = regime_target.forward(&t.next_state);
                    t.reward + self.gamma * nq[best_next]
                };
                let mut targets = vec![0.0; ACTION_DIM];
                targets[t.action] = td_target;
                let mask: Vec<bool> = (0..ACTION_DIM).map(|i| i == t.action).collect();
                regime_online.backward_step(&t.state, &targets, &mask, self.lr, is_weight);
            }

            // Periodic target sync for the regime net
            if self.step_count > 0 && self.step_count % self.target_update_freq == 0 {
                let online_clone = {
                    let (on, _) = self.regime_nets.get(&regime).unwrap();
                    on.clone()
                };
                let (_, tgt) = self.regime_nets.get_mut(&regime).unwrap();
                tgt.copy_from(&online_clone);
            }
        }

        let avg_loss = total_loss / self.batch_size as f64;
        self.recent_losses.push(avg_loss);
        if self.recent_losses.len() > 200 {
            self.recent_losses.remove(0);
        }
        self.train_steps += 1;

        // Periodically hard-copy online → target
        if self.step_count > 0 && self.step_count % self.target_update_freq == 0 {
            let online = self.online_net.clone();
            self.target_net.copy_from(&online);
            self.target_updates += 1;
            info!(
                "🧠 NN target network updated (step={}, updates={}, ε={:.4})",
                self.step_count, self.target_updates, self.epsilon
            );
        }

        debug!("train_step loss={:.6} ε={:.4}", avg_loss, self.epsilon);
        Some(avg_loss)
    }

    // ── Reward shaping ──────────────────────────────────────────────────────

    /// Convert PnL percentage into a shaped scalar reward.
    ///
    /// v1.0.0 — mathematically redesigned from real trade distribution analysis:
    ///   observed wins: +99% at 0-19s; observed losses: -90% at various ages.
    ///   Old function EV at 30% win rate ≈ -192.  New EV > 0 when exits are fast.
    ///
    /// **Profit zone** — convex (super-linear) scaling via log₂:
    ///   R = pnl × (1 + log₂(1 + pnl/25))
    ///   +25% → ×2.0, +50% → ×2.58, +99% → ×3.31  (replaces flat ×1.6 cap).
    ///   Timing bonus: +75 for < 1 min, +30 for 1-3 min, +10 for 3-10 min,
    ///   −2/step thereafter (capital-lock cost), capped at −40.
    ///
    /// **Loss zones** — sub-linear scaling keeps noise from dominating signal:
    ///   −5% to 0%  : ×1.0  (noise — don't overfit)
    ///   −30% to −5%: ×1.8  (avoidable dip-holding)
    ///   −60% to −30%: ×2.5 (failure to cut losses)
    ///   < −60% (rug): ×1.5 flat −15 if hold_steps=0 (mercy — unavoidable),
    ///                  ×2.5 flat −70 otherwise (should have exited sooner).
    ///
    /// `hold_steps` is position age in MINUTES (call site: age_secs/60 as u32).
    pub fn shape_reward(pnl_pct: f64, hold_steps: u32) -> f64 {
        if pnl_pct >= 0.0 {
            // Super-linear profit: log₂(1 + pnl/25) adds ~0.26× at +5%,
            // ~1.0× at +25%, ~2.31× at +99%.  Rewards big wins far more.
            let log_boost = ((1.0 + pnl_pct / 25.0).ln() / std::f64::consts::LN_2).max(0.0);
            let base_reward = pnl_pct * (1.0 + log_boost);

            let timing_bonus: f64 = if hold_steps == 0 {
                75.0 // < 1 min fast snipe — maximum efficiency signal
            } else if hold_steps <= 3 {
                30.0 // quick clean exit
            } else if hold_steps <= 10 {
                10.0 // acceptable hold
            } else {
                // Capital-lock cost past 10 min, capped at −40
                -(((hold_steps as f64 - 10.0) * 2.0).min(40.0))
            };

            base_reward + timing_bonus
        } else if pnl_pct >= -5.0 {
            // Tiny loss: noise territory — don't let it drown profit signal
            pnl_pct * 1.0
        } else if pnl_pct >= -30.0 {
            // Moderate loss: avoidable, penalise dip-holding
            pnl_pct * 1.8
        } else if pnl_pct >= -60.0 {
            // Heavy loss: failure to cut — strong negative gradient
            pnl_pct * 2.5
        } else {
            // Rug territory (< −60%).
            // hold_steps=0 → exited in < 1 min → unavoidable; mercy reduces flat.
            // hold_steps>0 → held through a recognisable dump → full punishment.
            if hold_steps == 0 {
                pnl_pct * 1.5 - 15.0
            } else {
                pnl_pct * 2.5 - 70.0
            }
        }
    }

    // ── Regime handling (Feature 1) ─────────────────────────────────────────

    /// Called when the market regime label changes (no-arg version for backward compat).
    /// Delegates to `notify_regime_shift_labeled("unknown")`.
    pub fn notify_regime_shift(&mut self) {
        self.notify_regime_shift_labeled("unknown");
    }

    /// Set the active regime and spike epsilon so the agent re-explores under
    /// the new regime policy rather than applying a stale policy.
    pub fn notify_regime_shift_labeled(&mut self, regime: &str) {
        self.active_regime = regime.to_string();
        let new_epsilon = (self.epsilon + 0.25).min(0.40).max(self.epsilon);
        if new_epsilon > self.epsilon {
            info!(
                "🧠 Regime shift → '{}' — spiking ε: {:.4} → {:.4}",
                regime, self.epsilon, new_epsilon
            );
            self.epsilon = new_epsilon;
        }
        // Lazily create the regime pair if it doesn't exist yet
        if regime != "unknown" && !self.regime_nets.contains_key(regime) {
            let sizes = [STATE_DIM, 128, 64, ACTION_DIM];
            let online = QNetwork::new(&sizes);
            let mut target = QNetwork::new(&sizes);
            target.copy_from(&online);
            self.regime_nets
                .insert(regime.to_string(), (online, target));
            info!("🧠 Created network pair for regime '{}'", regime);
        }
    }

    /// Poll the regime-shift signal file written by the sniper strategy loop.
    /// Returns true if a shift was detected and ε was spiked.
    /// The caller should delete the file after reading.
    pub fn poll_regime_shift_file(path: &str) -> bool {
        if std::path::Path::new(path).exists() {
            let _ = std::fs::remove_file(path);
            return true;
        }
        false
    }

    // ── Adversarial simulation (Feature 2) ─────────────────────────────────

    /// Inject `count` synthetic adversarial transitions into the replay buffer.
    ///
    /// Rewards are calibrated to match `shape_reward` output so synthetic
    /// scenarios don't contradict real-trade signal magnitudes:
    /// 1. **Rug-pull** (held through): −90% pnl, slow exit → reward ≈ −295.
    /// 2. **Pump-and-dump** (fast peak exit): +99% pnl, hold_steps=0 → reward ≈ +403.
    /// 3. **Honeypot** (capital locked for hours): −70% pnl, slow exit → reward ≈ −245.
    pub fn inject_adversarial_scenarios(&mut self, count: usize) {
        let mut rng = rand::thread_rng();
        for i in 0..count {
            match i % 3 {
                // ── Rug-pull ──────────────────────────────────────────────
                0 => {
                    let state = TradeState {
                        pool_age_secs: rng.gen_range(60.0..600.0),
                        initial_liquidity_sol: rng.gen_range(1.0..10.0),
                        price_change_pct: rng.gen_range(1.0..5.0), // briefly pumped
                        volume_5min_sol: rng.gen_range(5.0..20.0),
                        buy_sell_ratio: rng.gen_range(3.0..8.0),
                        lp_burned: false,
                        mint_renounced: false,
                        current_pnl_pct: rng.gen_range(0.3..1.5),
                        position_age_secs: rng.gen_range(30.0..300.0),
                        daily_pnl_sol: rng.gen_range(-0.5..0.5),
                        consecutive_wins: rng.gen_range(0..3),
                        consecutive_losses: 0,
                        sol_balance_sol: rng.gen_range(1.0..5.0),
                        regime: 1, // appeared bullish
                        volatility: rng.gen_range(0.5..1.0),
                        spread_pct: rng.gen_range(0.01..0.05),
                        time_of_day_norm: rng.gen_range(0.0..1.0),
                        open_positions: rng.gen_range(1..3),
                        deployer_rug_rate: 0.8, // high rug risk — adversarial scenario
                        ..Default::default()
                    };
                    let next_state = TradeState {
                        price_change_pct: -0.99, // crashed
                        current_pnl_pct: -0.95,
                        ..state.clone()
                    };
                    // Held through rug: -90% pnl, hold_steps=2 → shape_reward(-90,2) ≈ -295
                    self.replay.push(Transition {
                        state: state.to_vec(),
                        action: TradeAction::Hold.index(),
                        reward: -295.0,
                        next_state: next_state.to_vec(),
                        done: true,
                    });
                }
                // ── Pump-and-dump ─────────────────────────────────────────
                1 => {
                    let state = TradeState {
                        pool_age_secs: rng.gen_range(30.0..180.0),
                        initial_liquidity_sol: rng.gen_range(0.5..5.0),
                        price_change_pct: rng.gen_range(3.0..10.0), // fast rise
                        volume_5min_sol: rng.gen_range(20.0..80.0),
                        buy_sell_ratio: rng.gen_range(5.0..15.0),
                        lp_burned: false,
                        mint_renounced: false,
                        current_pnl_pct: rng.gen_range(0.5..2.0),
                        position_age_secs: rng.gen_range(10.0..120.0),
                        daily_pnl_sol: rng.gen_range(0.0..2.0),
                        consecutive_wins: rng.gen_range(1..5),
                        consecutive_losses: 0,
                        sol_balance_sol: rng.gen_range(2.0..8.0),
                        regime: 1,
                        volatility: rng.gen_range(0.7..1.0),
                        spread_pct: rng.gen_range(0.02..0.08),
                        time_of_day_norm: rng.gen_range(0.0..1.0),
                        open_positions: rng.gen_range(1..4),
                        peak_pnl_pct: rng.gen_range(0.8..2.5), // strong pump signal
                        volume_velocity: rng.gen_range(0.3..1.0), // volume accelerating
                        price_velocity: rng.gen_range(0.3..1.0), // price accelerating up
                        deployer_rug_rate: rng.gen_range(0.0..0.3), // lower rug risk
                        ..Default::default()
                    };
                    let next_state = TradeState {
                        price_change_pct: rng.gen_range(-0.8..-0.3), // crash after dump
                        current_pnl_pct: 0.0,                        // sold at peak
                        ..state.clone()
                    };
                    // Fast peak exit: +99% pnl, hold_steps=0 → shape_reward(99,0) ≈ +403
                    self.replay.push(Transition {
                        state: state.to_vec(),
                        action: TradeAction::SellAll.index(),
                        reward: 403.0,
                        next_state: next_state.to_vec(),
                        done: true,
                    });
                }
                // ── Honeypot ──────────────────────────────────────────────
                _ => {
                    let state = TradeState {
                        pool_age_secs: rng.gen_range(120.0..600.0),
                        initial_liquidity_sol: rng.gen_range(1.0..8.0),
                        price_change_pct: rng.gen_range(0.5..3.0),
                        volume_5min_sol: rng.gen_range(2.0..15.0),
                        buy_sell_ratio: rng.gen_range(10.0..50.0), // absurdly high (no sells)
                        lp_burned: false,
                        mint_renounced: false,
                        current_pnl_pct: rng.gen_range(0.1..1.0),
                        position_age_secs: rng.gen_range(600.0..3_600.0), // stuck
                        daily_pnl_sol: rng.gen_range(-1.0..0.0),
                        consecutive_wins: 0,
                        consecutive_losses: rng.gen_range(1..5),
                        sol_balance_sol: rng.gen_range(0.5..3.0),
                        regime: 0, // sideways / uncertain
                        volatility: rng.gen_range(0.1..0.4),
                        spread_pct: rng.gen_range(0.05..0.3),
                        time_of_day_norm: rng.gen_range(0.0..1.0),
                        open_positions: rng.gen_range(1..5),
                        volume_velocity: rng.gen_range(-0.5..0.0), // volume dying
                        deployer_rug_rate: 0.9,                    // honeypot = rug
                        ..Default::default()
                    };
                    let next_state = TradeState {
                        price_change_pct: state.price_change_pct * 0.9,
                        current_pnl_pct: -1.0, // effectively a total loss
                        ..state.clone()
                    };
                    // Capital locked for hours: -70% pnl, hold_steps=10+ → shape_reward(-70,10) ≈ -245
                    self.replay.push(Transition {
                        state: state.to_vec(),
                        action: TradeAction::SellAll.index(),
                        reward: -245.0,
                        next_state: next_state.to_vec(),
                        done: true,
                    });
                }
            }
        }
    }

    // ── Action rebalancing ─────────────────────────────────────────────────

    /// Inject balanced synthetic transitions so all actions are represented in the
    /// replay buffer. Without this, SellAll dominates (every real trade is a sell)
    /// and the agent collapses to a single-action policy.
    ///
    /// Injects one Hold and one SellPartial experience per call alongside the
    /// existing adversarial scenarios.
    pub fn inject_action_balance(&mut self) {
        let mut rng = rand::thread_rng();

        // ── Hold transition (patience rewarded on stable pool) ─────────────
        let hold_state = TradeState {
            pool_age_secs: rng.gen_range(30.0..180.0),
            initial_liquidity_sol: rng.gen_range(5.0..20.0),
            price_change_pct: rng.gen_range(0.1..0.8),
            volume_5min_sol: rng.gen_range(10.0..40.0),
            buy_sell_ratio: rng.gen_range(2.0..5.0),
            lp_burned: true,
            mint_renounced: true,
            current_pnl_pct: rng.gen_range(0.05..0.3),
            position_age_secs: rng.gen_range(5.0..30.0),
            daily_pnl_sol: rng.gen_range(0.0..0.5),
            consecutive_wins: rng.gen_range(0..3),
            consecutive_losses: 0,
            sol_balance_sol: rng.gen_range(2.0..8.0),
            regime: 1,
            volatility: rng.gen_range(0.3..0.6),
            peak_pnl_pct: rng.gen_range(0.1..0.5),
            pool_score_norm: rng.gen_range(0.6..1.0),
            ..Default::default()
        };
        let next_hold = TradeState {
            current_pnl_pct: hold_state.current_pnl_pct * 1.2,
            ..hold_state.clone()
        };
        self.replay.push(Transition {
            state: hold_state.to_vec(),
            action: TradeAction::Hold.index(),
            reward: 15.0, // small positive: good call to hold a pumping pool
            next_state: next_hold.to_vec(),
            done: false,
        });

        // ── SellPartial transition (partial exit at moderate profit) ───────
        let partial_state = TradeState {
            pool_age_secs: rng.gen_range(10.0..60.0),
            initial_liquidity_sol: rng.gen_range(5.0..15.0),
            price_change_pct: rng.gen_range(0.3..0.8),
            volume_5min_sol: rng.gen_range(20.0..60.0),
            buy_sell_ratio: rng.gen_range(1.5..4.0),
            current_pnl_pct: rng.gen_range(0.25..0.75),
            position_age_secs: rng.gen_range(5.0..60.0),
            peak_pnl_pct: rng.gen_range(0.3..0.9),
            pool_score_norm: rng.gen_range(0.5..0.9),
            consecutive_wins: rng.gen_range(0..3),
            sol_balance_sol: rng.gen_range(1.0..5.0),
            ..Default::default()
        };
        let next_partial = TradeState {
            current_pnl_pct: partial_state.current_pnl_pct * 0.7,
            ..partial_state.clone()
        };
        self.replay.push(Transition {
            state: partial_state.to_vec(),
            action: TradeAction::SellPartial.index(),
            // Reward = shape_reward of the partial pct captured
            reward: DQNAgent::shape_reward(partial_state.current_pnl_pct * 100.0, 0),
            next_state: next_partial.to_vec(),
            done: false,
        });
    }

    // ── Tournament evolution ────────────────────────────────────────────────

    /// Evolve the tournament variant pool after a tournament completes.
    ///
    /// The winning variant's hyperparameters are kept. The two losers are
    /// replaced with mutations of the winner: ±20% on lr, ±0.005 on epsilon_decay,
    /// ±0.005 on gamma. This turns the fixed 3-variant pool into a continuous
    /// hill-climb across the hyperparameter landscape.
    ///
    /// Returns the new (epsilon_decay, lr, gamma) triples for each variant.
    pub fn evolve_tournament_variants(&mut self, winner_idx: usize) -> Vec<(f64, f64, f64)> {
        if self.tournament_hyperparams.is_empty() {
            return vec![];
        }
        let winner_idx = winner_idx.min(self.tournament_hyperparams.len() - 1);
        let (wd, wl, wg) = self.tournament_hyperparams[winner_idx];
        let mut rng = rand::thread_rng();

        let mut new_params = vec![(wd, wl, wg)]; // keep winner
        for _ in 1..self.tournament_hyperparams.len() {
            let lr_m: f64 = rng.gen_range(0.8..1.2);
            let ed_delta: f64 = rng.gen_range(-0.0005..0.0005);
            let gm_delta: f64 = rng.gen_range(-0.005..0.005);
            let new = (
                (wd + ed_delta).clamp(0.998, 0.9999),
                (wl * lr_m).clamp(1e-4, 5e-3),
                (wg + gm_delta).clamp(0.95, 0.999),
            );
            new_params.push(new);
        }
        self.tournament_hyperparams = new_params.clone();
        info!(
            "🧠 Tournament evolved: winner hyperparams ({:.4},{:.4},{:.4}) → {} mutants",
            wd,
            wl,
            wg,
            self.tournament_hyperparams.len() - 1
        );
        new_params
    }

    // ── Explainability (Feature 3) ──────────────────────────────────────────

    /// Compute Q-values for `state` using the global online network and return
    /// a human-readable explanation of the chosen action.
    pub fn explain_decision(&self, state: &TradeState) -> TradeDecisionExplanation {
        let sv = state.to_vec();
        let q_raw = self.online_net.forward(&sv);

        // Pair each Q-value with its action label
        let action_labels = [
            "Hold",
            "BuyStandard",
            "BuyAggressive",
            "SellPartial",
            "SellAll",
        ];
        let q_values: Vec<(String, f64)> = action_labels
            .iter()
            .zip(q_raw.iter())
            .map(|(&label, &qv)| (label.to_string(), qv))
            .collect();

        // Find the best action index
        let (best_idx, best_q) = q_raw
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, &v)| (i, v))
            .unwrap_or((0, 0.0));

        // Find the second-best Q to make the reason string meaningful
        let second_best = q_raw
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != best_idx)
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, &v)| (i, v));

        let top_reason = if let Some((second_idx, second_q)) = second_best {
            let pct_above = if second_q.abs() > 1e-9 {
                (best_q - second_q) / second_q.abs() * 100.0
            } else {
                100.0
            };
            let signal_hint = match best_idx {
                0 => "hold signal",
                1 | 2 => "entry signal",
                3 => "partial exit signal",
                4 => "momentum exit signal",
                _ => "signal",
            };
            format!(
                "Q({})={:+.2} is {:.1}% above Q({})={:+.2} — {}",
                action_labels[best_idx],
                best_q,
                pct_above,
                action_labels[second_idx],
                second_q,
                signal_hint,
            )
        } else {
            format!("Q({})={:+.2} dominates", action_labels[best_idx], best_q)
        };

        // Confidence: max_q / sum_abs_q
        let sum_abs: f64 = q_raw.iter().map(|v| v.abs()).sum();
        let confidence = if sum_abs > 1e-9 {
            best_q.abs() / sum_abs
        } else {
            0.0
        };

        TradeDecisionExplanation {
            action: action_labels[best_idx].to_string(),
            action_index: best_idx,
            q_values,
            top_reason,
            confidence,
        }
    }

    /// Compute and write an explanation JSON to `path`.
    pub fn write_explanation(&self, state: &TradeState, path: &str) {
        let explanation = self.explain_decision(state);
        let json = serde_json::to_string_pretty(&explanation).unwrap_or_default();
        let _ = std::fs::write(path, json);
    }

    // ── Persistence ─────────────────────────────────────────────────────────

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let ckpt = Checkpoint {
            online_net: self.online_net.clone(),
            target_net: self.target_net.clone(),
            epsilon: self.epsilon,
            step_count: self.step_count,
            train_steps: self.train_steps,
            total_reward: self.total_reward,
            target_updates: self.target_updates,
            regime_nets: self.regime_nets.clone(),
            active_regime: self.active_regime.clone(),
            state_dim: STATE_DIM,
            action_dim: ACTION_DIM,
        };
        let tmp = format!("{}.tmp", path);
        std::fs::write(&tmp, serde_json::to_string(&ckpt).unwrap())?;
        std::fs::rename(&tmp, path)
    }

    pub fn load(path: &str) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let ckpt: Checkpoint = serde_json::from_str(&raw)?;

        // Checkpoint versioning: silently reset if STATE_DIM or ACTION_DIM changed
        // rather than panicking on mismatched weight matrix shapes.
        let saved_state_dim = if ckpt.state_dim == 0 {
            18
        } else {
            ckpt.state_dim
        };
        let saved_action_dim = if ckpt.action_dim == 0 {
            5
        } else {
            ckpt.action_dim
        };
        if saved_state_dim != STATE_DIM || saved_action_dim != ACTION_DIM {
            info!(
                "NN checkpoint has state_dim={}/{} action_dim={}/{} — resetting agent",
                saved_state_dim, STATE_DIM, saved_action_dim, ACTION_DIM
            );
            return Ok(Self::new());
        }

        let mut agent = Self::new();
        agent.online_net = ckpt.online_net;
        agent.target_net = ckpt.target_net;
        agent.epsilon = ckpt.epsilon;
        agent.step_count = ckpt.step_count;
        agent.train_steps = ckpt.train_steps;
        agent.total_reward = ckpt.total_reward;
        agent.target_updates = ckpt.target_updates;
        agent.regime_nets = ckpt.regime_nets;
        agent.active_regime = ckpt.active_regime;
        Ok(agent)
    }

    // ── Stats ────────────────────────────────────────────────────────────────

    pub fn stats(&self) -> AgentStats {
        let avg_loss = if self.recent_losses.is_empty() {
            0.0
        } else {
            self.recent_losses.iter().sum::<f64>() / self.recent_losses.len() as f64
        };
        AgentStats {
            step_count: self.step_count,
            train_steps: self.train_steps,
            epsilon: self.epsilon,
            replay_size: self.replay.len(),
            total_reward: self.total_reward,
            avg_loss,
            target_updates: self.target_updates,
            ready_to_advise: self.ready_to_advise(),
            last_action: self.last_action.map(|a| a.label().to_string()),
            last_q_values: self.last_q_values.clone(),
        }
    }

    pub fn ready_to_advise(&self) -> bool {
        self.train_steps >= 500
            && self
                .last_q_values
                .iter()
                .any(|v| v.is_finite() && v.abs() > 1e-9)
    }
}

impl Default for DQNAgent {
    fn default() -> Self {
        Self::new()
    }
}
