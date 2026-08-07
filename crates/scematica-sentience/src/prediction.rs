//! §11  Prediction Engine
//!
//! ```text
//! Ω̂_{t+1} = P(Ω_{t+1} | Ω_t, O_t)
//! F = {f_1, f_2, ..., f_n}  each with P(f_i | K_t, Ω_t)
//! ```
//!
//! Uncertainty must be maintained — never collapse into a single assumed future.

use serde::{Deserialize, Serialize};
use crate::types::Confidence;

/// A single predicted future scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FutureScenario {
    pub id: String,
    pub description: String,
    pub probability: f64,
    pub predicted_value: f64,
    pub confidence: Confidence,
}

/// A distribution over future scenarios — the prediction is the whole distribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionDistribution {
    pub scenarios: Vec<FutureScenario>,
}

impl PredictionDistribution {
    pub fn new(scenarios: Vec<FutureScenario>) -> Self {
        let mut d = Self { scenarios };
        d.normalize();
        d
    }

    fn normalize(&mut self) {
        let total: f64 = self.scenarios.iter().map(|s| s.probability).sum();
        if total > 0.0 {
            for s in &mut self.scenarios {
                s.probability /= total;
            }
        }
    }

    /// Expected value E[V] = Σ P(f_i) × V(f_i).
    pub fn expected_value(&self) -> f64 {
        self.scenarios.iter().map(|s| s.probability * s.predicted_value).sum()
    }

    /// Shannon entropy — higher means more uncertainty.
    pub fn entropy(&self) -> f64 {
        self.scenarios
            .iter()
            .filter(|s| s.probability > 0.0)
            .map(|s| -s.probability * s.probability.ln())
            .sum()
    }

    pub fn mode(&self) -> Option<&FutureScenario> {
        self.scenarios.iter().max_by(|a, b| a.probability.partial_cmp(&b.probability).unwrap())
    }
}
