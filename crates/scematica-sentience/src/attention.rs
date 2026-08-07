//! §18  Attention Equation
//!
//! ```text
//! Att_i = Novelty_i × Importance_i × Uncertainty_i × GoalRelevance_i × Risk_i
//! ```
//!
//! High-attention information enters working cognition preferentially.
//! Prevents equal computational treatment of every available signal.

use serde::{Deserialize, Serialize};

/// Attention score for a single information signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionScore {
    pub signal_id: String,
    pub novelty: f64,
    pub importance: f64,
    pub uncertainty: f64,
    pub goal_relevance: f64,
    pub risk: f64,
}

impl AttentionScore {
    pub fn new(
        signal_id: impl Into<String>,
        novelty: f64,
        importance: f64,
        uncertainty: f64,
        goal_relevance: f64,
        risk: f64,
    ) -> Self {
        Self {
            signal_id: signal_id.into(),
            novelty: novelty.clamp(0.0, 1.0),
            importance: importance.clamp(0.0, 1.0),
            uncertainty: uncertainty.clamp(0.0, 1.0),
            goal_relevance: goal_relevance.clamp(0.0, 1.0),
            risk: risk.clamp(0.0, 1.0),
        }
    }

    /// `Att_i = Novelty × Importance × Uncertainty × GoalRelevance × Risk`
    pub fn score(&self) -> f64 {
        self.novelty
            * self.importance
            * self.uncertainty
            * self.goal_relevance
            * self.risk
    }
}

/// Rank a set of signals by attention score, highest first.
pub fn rank_by_attention(signals: &mut Vec<AttentionScore>) {
    signals.sort_by(|a, b| b.score().partial_cmp(&a.score()).unwrap());
}
