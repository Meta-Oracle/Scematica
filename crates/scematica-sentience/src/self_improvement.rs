//! §25  Recursive Self-Improvement
//!
//! ```text
//! Q_t = f(Accuracy, Efficiency, Robustness, Safety, Generalization)
//! ΔArchitecture = g(Q_t, Error_t, Feedback_t)
//! ```
//!
//! Any proposed change must pass: Simulation → Evaluation → SafetyVerification
//! → Human/ExternalValidation → Deployment.
//!
//! The system must NOT equate self-modification with automatic authority.

use serde::{Deserialize, Serialize};
use crate::types::Bounded;

/// Quality assessment of the current architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureQuality {
    pub accuracy: Bounded,
    pub efficiency: Bounded,
    pub robustness: Bounded,
    pub safety: Bounded,
    pub generalization: Bounded,
}

impl ArchitectureQuality {
    pub fn new(accuracy: f64, efficiency: f64, robustness: f64, safety: f64, generalization: f64) -> Self {
        Self {
            accuracy: accuracy.into(),
            efficiency: efficiency.into(),
            robustness: robustness.into(),
            safety: safety.into(),
            generalization: generalization.into(),
        }
    }

    /// Scalar quality index — geometric mean for fair balance.
    pub fn quality_index(&self) -> f64 {
        (self.accuracy.value()
            * self.efficiency.value()
            * self.robustness.value()
            * self.safety.value()
            * self.generalization.value())
        .powf(0.2)
    }
}

/// Stages a proposed architectural change must pass before deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationStage {
    Simulation,
    Evaluation,
    SafetyVerification,
    ExternalValidation,
    Approved,
    Rejected { reason: String },
}

/// A proposed architectural modification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureProposal {
    pub id: String,
    pub description: String,
    pub stage: ValidationStage,
    pub quality_before: f64,
    pub quality_projected: f64,
}

impl ArchitectureProposal {
    /// Advance to next validation stage (linear pipeline).
    pub fn advance(&mut self) {
        self.stage = match &self.stage {
            ValidationStage::Simulation => ValidationStage::Evaluation,
            ValidationStage::Evaluation => ValidationStage::SafetyVerification,
            ValidationStage::SafetyVerification => ValidationStage::ExternalValidation,
            ValidationStage::ExternalValidation => ValidationStage::Approved,
            other => other.clone(),
        };
    }

    pub fn reject(&mut self, reason: impl Into<String>) {
        self.stage = ValidationStage::Rejected { reason: reason.into() };
    }

    pub fn is_deployable(&self) -> bool {
        self.stage == ValidationStage::Approved
    }
}
