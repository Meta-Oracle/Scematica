//! Multi-agent tournament (#29).
//!
//! Runs 3 `DQNAgent` variants simultaneously in paper-trading mode.
//! Every `eval_freq` steps a challenger may be promoted over the primary agent, judged on
//! **recent mean reward** and required to clear a margin. Tournament state is serialised to
//! `scematica-nn-tournament.json`.
//!
//! It used to promote on `total_reward`, which is a lifetime sum that is never reset — so a
//! variant that was better in its first thousand steps kept winning forever, however badly it
//! was doing now. A comparison that cannot change its mind is not a tournament.

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
    /// How much better a challenger must be before it takes over, as a fraction of the
    /// incumbent's mean.
    ///
    /// Without a margin the tournament churns: three variants trained on the same stream
    /// produce means that cross constantly, and a primary that changes every evaluation is
    /// not a selection, it is noise with a promotion log.
    const PROMOTION_MARGIN: f64 = 0.10;

    /// Promote a challenger, if one has earned it.
    ///
    /// Judged on **recent** mean reward, not `total_reward`. The lifetime sum is never
    /// reset, so a variant that was better in its first thousand steps keeps winning
    /// forever — a comparison that cannot change its mind, which is the opposite of what a
    /// tournament is for.
    ///
    /// Three things can stop a promotion, and they are different:
    ///
    /// * the incumbent has no recent mean yet — nothing to compare against, so nothing to do
    /// * no challenger has one — they have not performed badly, they have not performed
    /// * the best challenger is ahead, but by less than the margin — that is noise
    pub fn maybe_promote_winner(&mut self) {
        if self.steps_since_eval < self.eval_freq {
            return;
        }
        self.steps_since_eval = 0;

        // `Option`, so a variant below the minimum sample is *absent* from the comparison
        // rather than entering it at zero — which would rank it below every losing variant
        // on the strength of having done nothing.
        let means: Vec<Option<f64>> =
            self.agents.iter().map(|(_, a)| a.recent_mean()).collect();

        let Some(incumbent) = means[self.primary_idx] else {
            // The incumbent has not been measured recently. Promoting against an unmeasured
            // baseline would be choosing on one number, not comparing two.
            return;
        };

        let mut best: Option<(usize, f64)> = None;
        for (i, m) in means.iter().enumerate() {
            if i == self.primary_idx {
                continue;
            }
            if let Some(v) = m {
                if best.is_none_or(|(_, b)| *v > b) {
                    best = Some((i, *v));
                }
            }
        }

        let Some((idx, challenger)) = best else {
            return;
        };

        // The margin is relative to the incumbent's magnitude, and `abs` because rewards go
        // negative: a challenger at -1.0 against an incumbent at -2.0 is a real improvement,
        // and a margin computed on the signed value would invert the test exactly when the
        // agent is losing money.
        let bar = incumbent + incumbent.abs() * Self::PROMOTION_MARGIN;
        if challenger <= bar {
            return;
        }

        let old_name = self.agents[self.primary_idx].0.clone();
        let new_name = self.agents[idx].0.clone();
        info!(
            "Tournament: promoting '{}' over '{}' — recent mean {:.4} vs {:.4} over {}              transition(s), clearing a {:.0}% margin",
            new_name,
            old_name,
            challenger,
            incumbent,
            self.agents[idx].1.recent_n(),
            Self::PROMOTION_MARGIN * 100.0,
        );
        self.primary_idx = idx;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{RECENT_MIN, RECENT_WINDOW};
    use crate::state::TradeState;

    /// Feed one agent `n` transitions all carrying `reward`.
    ///
    /// Goes through `observe` rather than poking the field, so the window is filled the way
    /// the running agent fills it.
    fn feed(agent: &mut DQNAgent, n: usize, reward: f64) {
        let s = TradeState::default();
        for _ in 0..n {
            agent.observe(s.clone(), TradeAction::Hold, reward, s.clone(), false);
        }
    }

    fn ready(t: &mut AgentTournament) {
        t.steps_since_eval = t.eval_freq;
    }

    #[test]
    fn a_variant_that_was_good_long_ago_stops_winning() {
        // The defect this replaced. `total_reward` is a lifetime sum that is never reset, so
        // an agent with a huge early lead kept the primary slot however badly it was doing
        // now — a comparison that cannot change its mind.
        let mut t = AgentTournament::new();
        t.primary_idx = 0;

        // Agent 0: an enormous historical lead, currently losing on every transition.
        //
        // The losing run must be at least `RECENT_WINDOW` long, or the window still holds
        // some of the good history and the recent mean is legitimately higher. The first
        // version of this test fed only `RECENT_MIN * 2` and failed for exactly that reason
        // — the window working, not the promotion rule failing.
        feed(&mut t.agents[0].1, 400, 5.0);
        feed(&mut t.agents[0].1, RECENT_WINDOW, -1.0);
        // Agent 1: no history, currently winning.
        feed(&mut t.agents[1].1, RECENT_MIN * 2, 1.0);

        assert!(
            t.agents[0].1.stats().total_reward > t.agents[1].1.stats().total_reward,
            "the old criterion would still pick agent 0"
        );

        ready(&mut t);
        t.maybe_promote_winner();
        assert_eq!(t.primary_idx, 1, "recent performance must decide");
    }

    #[test]
    fn an_unmeasured_challenger_does_not_enter_the_comparison() {
        // A variant below the minimum sample has not performed badly — it has not performed.
        // Entering it at zero would rank it below every losing variant on the strength of
        // having done nothing.
        let mut t = AgentTournament::new();
        t.primary_idx = 0;
        feed(&mut t.agents[0].1, RECENT_MIN * 2, -1.0);
        feed(&mut t.agents[1].1, RECENT_MIN / 2, 10.0); // below the floor

        ready(&mut t);
        t.maybe_promote_winner();
        assert_eq!(t.primary_idx, 0, "a variant with too little data must not be promoted");
    }

    #[test]
    fn an_unmeasured_incumbent_is_not_replaced_on_one_number() {
        // Promoting against a baseline nobody measured is choosing, not comparing.
        let mut t = AgentTournament::new();
        t.primary_idx = 0;
        feed(&mut t.agents[1].1, RECENT_MIN * 2, 5.0);

        ready(&mut t);
        t.maybe_promote_winner();
        assert_eq!(t.primary_idx, 0);
    }

    #[test]
    fn a_challenger_inside_the_margin_does_not_churn_the_primary() {
        // Three variants on one stream produce means that cross constantly. A primary that
        // changes every evaluation is not a selection, it is noise with a promotion log.
        let mut t = AgentTournament::new();
        t.primary_idx = 0;
        feed(&mut t.agents[0].1, RECENT_MIN * 2, 1.00);
        feed(&mut t.agents[1].1, RECENT_MIN * 2, 1.05); // +5%, under the 10% bar

        ready(&mut t);
        t.maybe_promote_winner();
        assert_eq!(t.primary_idx, 0, "a 5% edge is inside the margin");
    }

    #[test]
    fn a_challenger_clearing_the_margin_is_promoted() {
        let mut t = AgentTournament::new();
        t.primary_idx = 0;
        feed(&mut t.agents[0].1, RECENT_MIN * 2, 1.0);
        feed(&mut t.agents[1].1, RECENT_MIN * 2, 2.0);

        ready(&mut t);
        t.maybe_promote_winner();
        assert_eq!(t.primary_idx, 1);
    }

    #[test]
    fn the_margin_works_the_right_way_round_when_rewards_are_negative() {
        // The trap in a relative margin: computed on the signed value, the test inverts
        // exactly when the agent is losing money. -1.0 against -2.0 is a real improvement.
        let mut t = AgentTournament::new();
        t.primary_idx = 0;
        feed(&mut t.agents[0].1, RECENT_MIN * 2, -2.0);
        feed(&mut t.agents[1].1, RECENT_MIN * 2, -1.0);

        ready(&mut t);
        t.maybe_promote_winner();
        assert_eq!(t.primary_idx, 1, "losing less is better and must promote");
    }

    #[test]
    fn nothing_is_promoted_before_the_evaluation_interval() {
        let mut t = AgentTournament::new();
        t.primary_idx = 0;
        feed(&mut t.agents[0].1, RECENT_MIN * 2, -5.0);
        feed(&mut t.agents[1].1, RECENT_MIN * 2, 5.0);

        t.steps_since_eval = 0;
        t.maybe_promote_winner();
        assert_eq!(t.primary_idx, 0, "the interval is the interval");
    }
}
