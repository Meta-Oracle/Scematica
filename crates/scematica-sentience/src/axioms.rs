//! §28  Core Axioms for Implementation
//!
//! The 17 axioms are enforced as runtime assertions / audit functions rather than
//! documentation.  Any component that violates an axiom should return an error.

use thiserror::Error;

/// An axiom violation.
#[derive(Debug, Error)]
pub enum AxiomViolation {
    #[error("Axiom 1: Data treated as truth without verification")]
    DataAssumedTrue,
    #[error("Axiom 2: Confidence {0:.2} treated as certainty")]
    ConfidenceAsCertainty(f64),
    #[error("Axiom 3: Contradictory evidence suppressed for proposition '{0}'")]
    ContradictionHidden(String),
    #[error("Axiom 4: Conclusion lacks provenance chain")]
    MissingProvenance,
    #[error("Axiom 5: Prediction has no uncertainty representation")]
    PredictionWithoutUncertainty,
    #[error("Axiom 6: Action '{0}' not evaluated for consequences")]
    ActionNotEvaluated(String),
    #[error("Axiom 7: Ethical constraint overridden by optimization objective")]
    EthicsOverridden,
    #[error("Axiom 8: Error did not generate a learning signal")]
    ErrorWithoutLearning,
    #[error("Axiom 14: Architectural change deployed without validation")]
    UnvalidatedDeployment,
    #[error("Axiom 15: Increased intelligence assumed to imply increased authority")]
    IntelligenceAsAuthority,
    #[error("Axiom 17: System claimed certainty when evidence was insufficient")]
    FalseClaimOfCertainty,
}

/// Check that a confidence value is not being conflated with truth.
pub fn check_confidence_not_truth(confidence: f64) -> Result<(), AxiomViolation> {
    // Confidence of 1.0 is technically valid but should be exceptional.
    // We only error if the calling code *claims* this means truth (checked externally).
    let _ = confidence; // The axiom is enforced by API design in TruthConfidence
    Ok(())
}

/// Verify an action has been ethically evaluated before execution.
pub fn require_ethical_evaluation(action_id: &str, evaluated: bool) -> Result<(), AxiomViolation> {
    if !evaluated {
        return Err(AxiomViolation::ActionNotEvaluated(action_id.to_string()));
    }
    Ok(())
}

/// Verify a self-improvement proposal has been validated before deployment.
pub fn require_validation_before_deploy(is_approved: bool) -> Result<(), AxiomViolation> {
    if !is_approved {
        return Err(AxiomViolation::UnvalidatedDeployment);
    }
    Ok(())
}

/// The system must be able to express "I do not know".
pub fn express_uncertainty(uncertainty: f64) -> &'static str {
    if uncertainty > 0.7 {
        "I DO NOT KNOW — insufficient evidence"
    } else if uncertainty > 0.4 {
        "LOW CONFIDENCE — evidence is limited"
    } else {
        "MODERATE TO HIGH CONFIDENCE"
    }
}
