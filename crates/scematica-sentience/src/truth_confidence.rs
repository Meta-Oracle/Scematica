//! §22  Truth Confidence
//!
//! ```text
//! C(P) = f(Evidence, SourceReliability, Corroboration, Recency, Consistency, Contradiction)
//! ```
//!
//! **C(P) = 1 does NOT mean P = True.**
//! It means current evidence strongly supports P.

use serde::{Deserialize, Serialize};
use crate::types::Confidence;

/// Inputs to the truth confidence function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TruthConfidenceInputs {
    pub evidence_strength: f64,
    pub source_reliability: f64,
    pub corroboration: f64,
    pub recency: f64,
    pub consistency: f64,
    /// Contradiction penalty `[0,1]` — higher means more contradicting evidence exists.
    pub contradiction_penalty: f64,
}

impl TruthConfidenceInputs {
    /// `C(P)` weighted average with contradiction penalty applied.
    pub fn confidence(&self) -> Confidence {
        let positive = (self.evidence_strength
            + self.source_reliability
            + self.corroboration
            + self.recency
            + self.consistency)
            / 5.0;
        let penalised = positive * (1.0 - self.contradiction_penalty.clamp(0.0, 1.0));
        Confidence::new(penalised)
    }

    /// Epistemic status label.
    pub fn epistemic_label(&self) -> &'static str {
        let c = self.confidence().value();
        match c {
            v if v >= 0.9 => "high_confidence",
            v if v >= 0.7 => "moderate_confidence",
            v if v >= 0.5 => "uncertain",
            v if v >= 0.3 => "low_confidence",
            _ => "insufficient_evidence",
        }
    }
}
