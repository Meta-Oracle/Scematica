//! §4  Logic Equation
//!
//! ```text
//! L = Val × Co × Q × Fq
//! ```
//!
//! Notation (conflicts resolved from original document):
//! - `Val`  — Validity (renamed from `V` which clashed with Visual)
//! - `Co`   — Consistency (renamed from `C` which clashed with Completeness)
//! - `Q`    — Causal coherence
//! - `Fq`   — Formal reasoning quality (renamed from `F` which clashed with Feedback)
//!
//! The system must distinguish:
//! - Correlation ≠ Causation
//! - Possibility ≠ Probability
//! - Probability ≠ Certainty
//! - Prediction ≠ Observation

use serde::{Deserialize, Serialize};

use crate::types::Bounded;

/// Inputs to the logic subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicInputs {
    /// Structural validity of arguments (`Val ∈ [0,1]`).
    pub validity: Bounded,
    /// Internal consistency — freedom from contradictions (`Co ∈ [0,1]`).
    pub consistency: Bounded,
    /// Causal coherence — correct attribution of cause vs. correlation (`Q ∈ [0,1]`).
    pub causal_coherence: Bounded,
    /// Formal reasoning quality — correct application of logic rules (`Fq ∈ [0,1]`).
    pub formal_quality: Bounded,
}

impl LogicInputs {
    pub fn new(
        validity: f64,
        consistency: f64,
        causal_coherence: f64,
        formal_quality: f64,
    ) -> Self {
        Self {
            validity: validity.into(),
            consistency: consistency.into(),
            causal_coherence: causal_coherence.into(),
            formal_quality: formal_quality.into(),
        }
    }

    /// Compute `L = Val × Co × Q × Fq`.
    pub fn logic_ratio(&self) -> Bounded {
        (self.validity.value()
            * self.consistency.value()
            * self.causal_coherence.value()
            * self.formal_quality.value())
        .into()
    }
}

impl Default for LogicInputs {
    fn default() -> Self {
        Self::new(0.9, 0.9, 0.85, 0.85)
    }
}
