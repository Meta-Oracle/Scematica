//! §14  Meta-Cognition
//!
//! ```text
//! MC = R_c × E_c × U_c × S_c
//! ```
//!
//! The system must reason about its own reasoning — continuously asking:
//! - "Why do I believe this?"
//! - "What evidence supports/contradicts this?"
//! - "How confident should I be?"
//! - "What would change my conclusion?"
//!
//! This creates recursive evaluation without assuming subjective consciousness.

use serde::{Deserialize, Serialize};
use crate::types::Bounded;

/// Meta-cognitive input dimensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaCognitionInputs {
    /// Reasoning confidence — how confident the system is in its own reasoning process.
    pub reasoning_confidence: Bounded,
    /// Error detection — ability to catch its own mistakes.
    pub error_detection: Bounded,
    /// Uncertainty calibration — alignment between expressed and actual uncertainty.
    pub uncertainty_calibration: Bounded,
    /// Self-consistency — coherence of beliefs across time.
    pub self_consistency: Bounded,
}

impl MetaCognitionInputs {
    pub fn new(
        reasoning_confidence: f64,
        error_detection: f64,
        uncertainty_calibration: f64,
        self_consistency: f64,
    ) -> Self {
        Self {
            reasoning_confidence: reasoning_confidence.into(),
            error_detection: error_detection.into(),
            uncertainty_calibration: uncertainty_calibration.into(),
            self_consistency: self_consistency.into(),
        }
    }

    /// `MC = R_c × E_c × U_c × S_c`
    pub fn meta_cognition_ratio(&self) -> Bounded {
        (self.reasoning_confidence.value()
            * self.error_detection.value()
            * self.uncertainty_calibration.value()
            * self.self_consistency.value())
        .into()
    }
}

impl Default for MetaCognitionInputs {
    fn default() -> Self {
        Self::new(0.8, 0.75, 0.85, 0.9)
    }
}

/// An introspective query the system poses to itself.
#[derive(Debug, Clone)]
pub enum MetaQuery {
    WhyDoIBelieveThis { proposition: String },
    WhatEvidenceSupports { proposition: String },
    WhatEvidenceContradicts { proposition: String },
    HowConfidentShouldIBe { proposition: String },
    WhatWouldChangeMyConclusion { proposition: String },
}
