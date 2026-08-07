//! §28  Core Axioms — enforced as runtime checks.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AxiomViolation {
    #[error("Axiom 7: Ethical constraint overridden by optimization")]
    EthicsOverridden,
    #[error("Axiom 8: Error produced no learning signal")]
    ErrorWithoutLearning,
    #[error("Axiom 14: Architectural change deployed without validation")]
    UnvalidatedDeployment,
    #[error("Axiom 6: Action '{0}' not evaluated for consequences")]
    ActionNotEvaluated(String),
}

pub fn require_ethical_gate(passed: bool) -> Result<(), AxiomViolation> {
    if !passed { Err(AxiomViolation::EthicsOverridden) } else { Ok(()) }
}

pub fn require_validation(approved: bool) -> Result<(), AxiomViolation> {
    if !approved { Err(AxiomViolation::UnvalidatedDeployment) } else { Ok(()) }
}

pub fn require_action_evaluated(id: &str, evaluated: bool) -> Result<(), AxiomViolation> {
    if !evaluated { Err(AxiomViolation::ActionNotEvaluated(id.to_string())) } else { Ok(()) }
}

/// Axiom 17: express "I do not know" when evidence is insufficient.
pub fn epistemic_label(uncertainty: f64) -> &'static str {
    if uncertainty > 0.7 { "I DO NOT KNOW" }
    else if uncertainty > 0.4 { "LOW CONFIDENCE" }
    else { "MODERATE CONFIDENCE" }
}
