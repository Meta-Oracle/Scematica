//! §17  Emotional / Valence Model
//!
//! ```text
//! V_t = P_t - R_t
//! ```
//!
//! Affect-like state variables modelled computationally — NOT assumed to be
//! biological emotions.  They influence attention and prioritization but do
//! NOT override hard ethical constraints.

use serde::{Deserialize, Serialize};

/// Valence and arousal state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValenceState {
    /// Predicted positive outcome `P_t`.
    pub predicted_positive: f64,
    /// Predicted negative outcome `R_t`.
    pub predicted_negative: f64,
    /// Arousal level `[0,1]`.
    pub arousal: f64,
    /// Salience `[0,1]` — how much this state demands attention.
    pub salience: f64,
    /// Urgency `[0,1]` — time pressure.
    pub urgency: f64,
}

impl ValenceState {
    pub fn new(
        predicted_positive: f64,
        predicted_negative: f64,
        arousal: f64,
        salience: f64,
        urgency: f64,
    ) -> Self {
        Self {
            predicted_positive,
            predicted_negative,
            arousal: arousal.clamp(0.0, 1.0),
            salience: salience.clamp(0.0, 1.0),
            urgency: urgency.clamp(0.0, 1.0),
        }
    }

    /// Net valence `V_t = P_t - R_t`.
    pub fn valence(&self) -> f64 {
        self.predicted_positive - self.predicted_negative
    }

    /// Priority boost factor for attention routing `[0,2]`.
    pub fn attention_boost(&self) -> f64 {
        (1.0 + self.salience * self.urgency).clamp(0.0, 2.0)
    }
}

impl Default for ValenceState {
    fn default() -> Self {
        Self::new(0.5, 0.2, 0.3, 0.3, 0.2)
    }
}
