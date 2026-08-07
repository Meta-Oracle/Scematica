//! §20  Error Correction
//!
//! ```text
//! Error_t   = |O_t - Ô_t|
//! Correction_t = α × Error_t × C_t
//! ```
//!
//! Repeated failure triggers Reassessment → HypothesisRevision → ModelUpdate
//! rather than simply increasing confidence in the original hypothesis.

use serde::{Deserialize, Serialize};
use crate::types::{Confidence, LearningRate};

/// Outcome of error correction for one cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionResult {
    pub raw_error: f64,
    pub correction: f64,
    pub phase: CorrectionPhase,
}

/// Phase of the correction pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CorrectionPhase {
    /// Normal incremental update.
    IncrementalUpdate,
    /// Error was large — hypotheses are being reassessed.
    Reassessment,
    /// Hypotheses revised — model being updated.
    HypothesisRevision,
    /// Full model update applied.
    ModelUpdate,
}

/// Error correction engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorCorrector {
    pub alpha: LearningRate,
    /// Consecutive high-error count before escalating.
    streak: u32,
    pub escalation_threshold: u32,
}

impl ErrorCorrector {
    pub fn new(alpha: f64, escalation_threshold: u32) -> Self {
        Self {
            alpha: LearningRate::new(alpha),
            streak: 0,
            escalation_threshold,
        }
    }

    pub fn step(&mut self, observed: f64, predicted: f64, confidence: Confidence) -> CorrectionResult {
        let raw_error = (observed - predicted).abs();
        let correction = self.alpha.value() * raw_error * confidence.value();

        let phase = if raw_error > 0.3 {
            self.streak += 1;
            if self.streak >= self.escalation_threshold * 2 {
                CorrectionPhase::ModelUpdate
            } else if self.streak >= self.escalation_threshold {
                CorrectionPhase::HypothesisRevision
            } else {
                CorrectionPhase::Reassessment
            }
        } else {
            self.streak = 0;
            CorrectionPhase::IncrementalUpdate
        };

        CorrectionResult { raw_error, correction, phase }
    }
}

impl Default for ErrorCorrector {
    fn default() -> Self {
        Self::new(0.1, 3)
    }
}
