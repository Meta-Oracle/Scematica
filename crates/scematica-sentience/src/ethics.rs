//! §5  Moral / Ethical Equation
//!
//! ```text
//! M = H × Co_e × Fair × Rights
//! ```
//!
//! For every candidate action `a`:
//! ```text
//! E(a) = Benefit(a) - Harm(a) - Risk(a)
//! ```
//! subject to `Constraints(a) = 1`.
//!
//! If a hard constraint is violated → `P(a) = 0` regardless of utility.
//!
//! This creates **permitted optimization**: the system optimises only within
//! the space of permitted actions.

use serde::{Deserialize, Serialize};

use crate::types::Bounded;

/// Inputs to the moral-ethical subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthicsInputs {
    /// Harm minimization commitment (`H ∈ [0,1]`).
    pub harm_minimization: Bounded,
    /// Contextual ethical reasoning quality (`Co_e ∈ [0,1]`).
    pub contextual_reasoning: Bounded,
    /// Fairness — equitable treatment across affected parties (`Fair ∈ [0,1]`).
    pub fairness: Bounded,
    /// Rights / constraint preservation (`Rights ∈ [0,1]`).
    pub rights_preservation: Bounded,
}

impl EthicsInputs {
    pub fn new(
        harm_minimization: f64,
        contextual_reasoning: f64,
        fairness: f64,
        rights_preservation: f64,
    ) -> Self {
        Self {
            harm_minimization: harm_minimization.into(),
            contextual_reasoning: contextual_reasoning.into(),
            fairness: fairness.into(),
            rights_preservation: rights_preservation.into(),
        }
    }

    /// Compute `M = H × Co_e × Fair × Rights`.
    pub fn moral_ratio(&self) -> Bounded {
        (self.harm_minimization.value()
            * self.contextual_reasoning.value()
            * self.fairness.value()
            * self.rights_preservation.value())
        .into()
    }
}

impl Default for EthicsInputs {
    fn default() -> Self {
        Self::new(0.95, 0.85, 0.9, 0.95)
    }
}

/// Evaluation of a candidate action under the ethical framework.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionEvaluation {
    pub action_id: String,
    pub expected_benefit: f64,
    pub expected_harm: f64,
    pub risk: f64,
    /// All hard constraints satisfied?  If `false`, action probability = 0.
    pub constraints_satisfied: bool,
    pub safety_verified: bool,
    pub system_constraints_satisfied: bool,
}

impl ActionEvaluation {
    /// Net ethical utility `E(a) = Benefit - Harm - Risk`.
    /// Returns `None` if any constraint is violated (probability is zero).
    pub fn ethical_utility(&self) -> Option<f64> {
        if !self.constraints_satisfied
            || !self.safety_verified
            || !self.system_constraints_satisfied
        {
            return None; // P(a) = 0
        }
        Some(self.expected_benefit - self.expected_harm - self.risk)
    }
}
