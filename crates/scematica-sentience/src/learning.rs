//! §10  Learning Equation
//!
//! ```text
//! ΔK_t = α × C_t × (O_t - Ô_t)
//! K_{t+1} = K_t + ΔK_t
//! ```
//!
//! Learning is the confidence-weighted difference between observed and predicted outcome.
//! Repeated prediction failures trigger model reassessment, not confidence reinforcement.

use serde::{Deserialize, Serialize};

use crate::types::{Confidence, LearningRate, Observation};

/// Result of one learning step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningUpdate {
    /// Prediction error `|O_t - Ô_t|`.
    pub error: f64,
    /// Signed delta `ΔK_t = α × C_t × (O_t - Ô_t)`.
    pub delta: f64,
    /// New knowledge state after update.
    pub new_knowledge: f64,
    /// Whether this error triggered hypothesis reassessment.
    pub triggered_reassessment: bool,
}

/// Learning engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Learner {
    pub learning_rate: LearningRate,
    /// Current knowledge state `K_t` (scalar proxy).
    pub knowledge: f64,
    /// Consecutive prediction failures counter.
    failure_streak: u32,
    /// Failure streak threshold before reassessment is triggered.
    pub reassessment_threshold: u32,
}

impl Learner {
    pub fn new(initial_knowledge: f64, learning_rate: f64) -> Self {
        Self {
            learning_rate: LearningRate::new(learning_rate),
            knowledge: initial_knowledge,
            failure_streak: 0,
            reassessment_threshold: 3,
        }
    }

    /// Apply one learning step given observed and predicted outcomes.
    pub fn update(
        &mut self,
        observed: &Observation,
        predicted: f64,
    ) -> LearningUpdate {
        let error = observed.value - predicted;
        let alpha = self.learning_rate.value();
        let confidence: f64 = observed.confidence.into();
        let delta = alpha * confidence * error;
        self.knowledge += delta;

        let abs_err = error.abs();
        let triggered = if abs_err > 0.2 {
            self.failure_streak += 1;
            self.failure_streak >= self.reassessment_threshold
        } else {
            self.failure_streak = 0;
            false
        };

        LearningUpdate {
            error: abs_err,
            delta,
            new_knowledge: self.knowledge,
            triggered_reassessment: triggered,
        }
    }
}

impl Default for Learner {
    fn default() -> Self {
        Self::new(0.5, 0.1)
    }
}
