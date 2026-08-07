//! §3  Rationality Equation
//!
//! ```text
//! R = (E × Co_r × U) / (B + ε)
//! ```
//!
//! Rewards evidence-based conclusions, consistency, and uncertainty awareness.
//! Penalises bias, circular reasoning, overconfidence, and hallucinated certainty.
//!
//! Note: `Co_r` here is **reasoning consistency** (distinct from `Co` in the Logic
//! equation).  `ε > 0` prevents division by zero when bias is absent.

use serde::{Deserialize, Serialize};

use crate::types::Bounded;

const EPSILON: f64 = 1e-6;

/// Inputs to the rationality subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RationalityInputs {
    /// Evidence utilization — how well available evidence is used (`E ∈ [0,1]`).
    pub evidence_utilization: Bounded,
    /// Internal consistency of the reasoning chain (`Co_r ∈ [0,1]`).
    pub consistency: Bounded,
    /// Uncertainty awareness — explicit representation of unknowns (`U ∈ [0,1]`).
    pub uncertainty_awareness: Bounded,
    /// Bias influence — how much systematic bias affects outputs (`B ∈ [0,1]`).
    /// Higher values → lower rationality.
    pub bias: Bounded,
}

impl RationalityInputs {
    pub fn new(
        evidence_utilization: f64,
        consistency: f64,
        uncertainty_awareness: f64,
        bias: f64,
    ) -> Self {
        Self {
            evidence_utilization: evidence_utilization.into(),
            consistency: consistency.into(),
            uncertainty_awareness: uncertainty_awareness.into(),
            bias: bias.into(),
        }
    }

    /// Compute `R = (E × Co_r × U) / (B + ε)`, clamped to `[0,1]`.
    pub fn rationality(&self) -> Bounded {
        let numerator = self.evidence_utilization.value()
            * self.consistency.value()
            * self.uncertainty_awareness.value();
        let denominator = self.bias.value() + EPSILON;
        (numerator / denominator).into()
    }
}

impl Default for RationalityInputs {
    /// High rationality baseline: excellent evidence use, consistency, and uncertainty
    /// awareness; minimal bias.
    fn default() -> Self {
        Self::new(0.9, 0.9, 0.8, 0.05)
    }
}
