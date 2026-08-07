//! Multi-agent tournament (#29).
//!
//! Runs 3 `DQNAgent` variants simultaneously in paper-trading mode.
//! Every `eval_freq` steps the winner by `total_reward` is promoted as the
//! primary agent.  Tournament state is serialised to `scematica-nn-tournament.json`.

use crate::{
    action::TradeAction,
    agent::{AgentStats, DQNAgent},
    state::TradeState,
};
use serde::{Deserialize, Serialize};
use tracing::info;

// ── Tournament serialisation helper ─────────────────────────────────────────

/// On-disk format for the tournament state.
#[derive(Serialize, Deserialize)]
struct TournamentSnapshot {
    primary_idx: usize,
    steps_since_eval: usize,
    eval_freq: usize,
    agent_names: Vec<String>,
    agent_total_rewards: Vec<f64>,
    agent_epsilons: Vec<f64>,
}

// ── Public type ───────────────────────────────────────────────────────────────

/// Runs 3 `DQNAgent` variants simultaneously in paper-trading mode.
///
/// Agents:
/// - "conservative": epsilon_decay=0.9999, lr=5e-4, gamma=0.95
/// - "balanced":     epsilon_decay=0.9995, lr=1e-3, gamma=0.99  (default)
/// - "aggressive":   epsilon_decay=0.999,  lr=2e-3, gamma=0.95
pub struct AgentTournament {
    /// (name, agent) triples.
    pub agents: Vec<(String, DQNAgent)>,
    /// Index of the current primary agent.
    pub primary_idx: usize,
    pub steps_since_eval: usize,
    pub eval_freq: usize,
}

impl AgentTournament {
    /// Create a fresh tournament with 3 agents using different hyper-parameters.
    pub fn new() -> Self {
        let configs: &[(&str, f64, f64, f64)] = &[
            ("conservative", 0.9999, 5e-4, 0.95),
            ("balanced",     0.9995, 1e-3, 0.99),
            ("aggressive",   0.999,  2e-3, 0.95),
        ];

        let agents = configs
            .iter()
            .map(|&(name, epsilon_decay, lr, gamma)| {
                (
                    name.to_string(),
                    DQNAgent::with_hyperparams(epsilon_decay, lr, gamma),
                )
            })
            .collect();

        Self {
            agents,
            primary_idx: 1, // "balanced" starts as primary
            steps_since_eval: 0,
            eval_freq: 1_000,
        }
    }

    /// Feed the same transition to all agents.
    pub fn observe_all(
        &mut self,
        state: TradeState,
        action: TradeAction,
        reward: f64,
        next_state: TradeState,
        done: bool,
    ) {
        for (_, agent) in self.agents.iter_mut() {
            agent.observe(state.clone(), action, reward, next_state.clone(), done);
        }
        self.steps_since_eval += 1;
        self.maybe_promote_winner();
    }

    /// Run one training step for every agent.
    pub fn train_all(&mut self) {
        for (_, agent) in self.agents.iter_mut() {
            agent.train_step();
        }
    }

    /// Return the action chosen by the current primary agent.
    pub fn primary_action(&mut self, state: &TradeState) -> TradeAction {
        self.agents[self.primary_idx].1.select_action(state)
    }

    /// Promote the agent with the highest `total_reward` to primary if it
    /// differs from the current primary.  Called automatically by `observe_all`.
    pub fn maybe_promote_winner(&mut self) {
        if self.steps_since_eval < self.eval_freq {
            return;
        }
        self.steps_since_eval = 0;

        let winner_idx = self
            .agents
            .iter()
            .enumerate()
            .max_by(|(_, (_, a)), (_, (_, b))| {
                a.stats()
                    .total_reward
                    .partial_cmp(&b.stats().total_reward)
                    .unwrap()
            })
            .map(|(i, _)| i)
            .unwrap_or(0);

        if winner_idx != self.primary_idx {
            let old_name = self.agents[self.primary_idx].0.clone();
            let new_name = self.agents[winner_idx].0.clone();
            let new_reward = self.agents[winner_idx].1.stats().total_reward;
            info!(
                "Tournament: promoting '{}' over '{}' (reward={:.2})",
                new_name, old_name, new_reward,
            );
            self.primary_idx = winner_idx;
        }
    }

    /// Persist tournament state to `path`.
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let snap = TournamentSnapshot {
            primary_idx: self.primary_idx,
            steps_since_eval: self.steps_since_eval,
            eval_freq: self.eval_freq,
            agent_names: self.agents.iter().map(|(n, _)| n.clone()).collect(),
            agent_total_rewards: self
                .agents
                .iter()
                .map(|(_, a)| a.stats().total_reward)
                .collect(),
            agent_epsilons: self
                .agents
                .iter()
                .map(|(_, a)| a.stats().epsilon)
                .collect(),
        };
        std::fs::write(path, serde_json::to_string_pretty(&snap).unwrap())
    }

    /// Restore a tournament from `path`.
    ///
    /// Because hyper-parameters are baked in at construction time, this
    /// rebuilds 3 fresh agents and restores only `primary_idx` / `eval_freq`
    /// from disk.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let snap: TournamentSnapshot = serde_json::from_str(&raw)?;
        let mut t = Self::new();
        t.primary_idx = snap.primary_idx.min(t.agents.len().saturating_sub(1));
        t.steps_since_eval = snap.steps_since_eval;
        t.eval_freq = snap.eval_freq;
        Ok(t)
    }

    /// Return per-agent stats snapshots.
    pub fn stats(&self) -> Vec<(String, AgentStats)> {
        self.agents
            .iter()
            .map(|(name, agent)| (name.clone(), agent.stats()))
            .collect()
    }
}

impl Default for AgentTournament {
    fn default() -> Self {
        Self::new()
    }
}
