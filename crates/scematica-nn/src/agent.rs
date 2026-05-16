use crate::{
    action::{TradeAction, ACTION_DIM},
    network::QNetwork,
    replay::{ReplayBuffer, Transition},
    state::{TradeState, STATE_DIM},
};
use rand::Rng;
use serde::{Deserialize, Serialize};
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
    /// True once epsilon < 0.5 and replay has enough samples to advise trades.
    pub ready_to_advise: bool,
    pub last_action: Option<String>,
    pub last_q_values: Vec<f64>,
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
}

/// Double Deep Q* agent.
///
/// Architecture: STATE_DIM → 128 → 64 → ACTION_DIM
/// Uses Double DQN: online net selects actions, target net evaluates them.
pub struct DQNAgent {
    online_net: QNetwork,
    target_net: QNetwork,
    replay: ReplayBuffer,
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
}

impl DQNAgent {
    pub fn new() -> Self {
        let sizes = [STATE_DIM, 128, 64, ACTION_DIM];
        let online_net = QNetwork::new(&sizes);
        let mut target_net = QNetwork::new(&sizes);
        target_net.copy_from(&online_net);
        Self {
            online_net,
            target_net,
            replay: ReplayBuffer::new(10_000),
            epsilon: 1.0,
            epsilon_min: 0.05,
            epsilon_decay: 0.9995,
            gamma: 0.99,
            lr: 1e-3,
            batch_size: 64,
            target_update_freq: 200,
            step_count: 0,
            train_steps: 0,
            total_reward: 0.0,
            recent_losses: Vec::new(),
            target_updates: 0,
            last_action: None,
            last_q_values: vec![0.0; ACTION_DIM],
        }
    }

    // ── Decision ────────────────────────────────────────────────────────────

    /// Epsilon-greedy action selection.
    pub fn select_action(&mut self, state: &TradeState) -> TradeAction {
        let sv = state.to_vec();
        let q = self.online_net.forward(&sv);
        self.last_q_values = q.clone();

        let action = if rand::thread_rng().gen::<f64>() < self.epsilon {
            TradeAction::from_index(rand::thread_rng().gen_range(0..ACTION_DIM))
        } else {
            let best = q.iter()
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
        let best = q.iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);
        (TradeAction::from_index(best), q)
    }

    // ── Learning ────────────────────────────────────────────────────────────

    /// Record a transition and decay epsilon.
    pub fn observe(
        &mut self,
        state: TradeState,
        action: TradeAction,
        reward: f64,
        next_state: TradeState,
        done: bool,
    ) {
        self.total_reward += reward;
        self.replay.push(Transition {
            state: state.to_vec(),
            action: action.index(),
            reward,
            next_state: next_state.to_vec(),
            done,
        });
        self.epsilon = (self.epsilon * self.epsilon_decay).max(self.epsilon_min);
        self.step_count += 1;
    }

    /// Sample a mini-batch and run one Double DQN gradient step.
    /// Returns average batch loss, or `None` if the buffer is too small.
    pub fn train_step(&mut self) -> Option<f64> {
        if self.replay.len() < self.batch_size {
            return None;
        }

        let batch = self.replay.sample(self.batch_size);
        let mut total_loss = 0.0;

        for t in &batch {
            // Double DQN: online net picks best next action, target net scores it
            let next_q_online = self.online_net.forward(&t.next_state);
            let best_next = next_q_online
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(0);

            let td_target = if t.done {
                t.reward
            } else {
                let nq_target = self.target_net.forward(&t.next_state);
                t.reward + self.gamma * nq_target[best_next]
            };

            // Build target vector: only the taken action gets the TD target;
            // other positions are masked out so they contribute zero gradient.
            let mut targets = vec![0.0; ACTION_DIM];
            targets[t.action] = td_target;
            let mask: Vec<bool> = (0..ACTION_DIM).map(|i| i == t.action).collect();

            total_loss += self.online_net.backward_step(&t.state, &targets, &mask, self.lr, 1.0);
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
    /// - Wins are rewarded linearly.
    /// - Losses are penalised 1.5× to discourage drawdown.
    /// - A small per-step hold penalty discourages idle positions.
    pub fn shape_reward(pnl_pct: f64, hold_steps: u32) -> f64 {
        let hold_penalty = hold_steps as f64 * 0.001;
        if pnl_pct >= 0.0 {
            pnl_pct - hold_penalty
        } else {
            pnl_pct * 1.5 - hold_penalty
        }
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
        };
        std::fs::write(path, serde_json::to_string(&ckpt).unwrap())
    }

    pub fn load(path: &str) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let ckpt: Checkpoint = serde_json::from_str(&raw)?;
        let mut agent = Self::new();
        agent.online_net    = ckpt.online_net;
        agent.target_net    = ckpt.target_net;
        agent.epsilon       = ckpt.epsilon;
        agent.step_count    = ckpt.step_count;
        agent.train_steps   = ckpt.train_steps;
        agent.total_reward  = ckpt.total_reward;
        agent.target_updates = ckpt.target_updates;
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
            ready_to_advise: self.epsilon < 0.5 && self.replay.len() >= self.batch_size,
            last_action: self.last_action.map(|a| a.label().to_string()),
            last_q_values: self.last_q_values.clone(),
        }
    }

    pub fn ready_to_advise(&self) -> bool {
        self.epsilon < 0.5 && self.replay.len() >= self.batch_size
    }
}

impl Default for DQNAgent {
    fn default() -> Self { Self::new() }
}
