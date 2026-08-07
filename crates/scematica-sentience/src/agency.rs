//! §12  Agency Equation
//!
//! ```text
//! A_g = P × M_o × E_v × D_c × F_b
//! ```
//!
//! Agency emerges operationally from the ability to perceive, model, evaluate,
//! choose, act, observe consequences, and update.

use serde::{Deserialize, Serialize};
use crate::types::Bounded;

/// Inputs to the agency subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgencyInputs {
    /// Perception capability `P ∈ [0,1]`.
    pub perception: Bounded,
    /// World modelling capability `M_o ∈ [0,1]`.
    pub world_modelling: Bounded,
    /// Evaluation capability `E_v ∈ [0,1]`.
    pub evaluation: Bounded,
    /// Decision capability `D_c ∈ [0,1]`.
    pub decision_capability: Bounded,
    /// Feedback integration `F_b ∈ [0,1]`.
    pub feedback_integration: Bounded,
}

impl AgencyInputs {
    pub fn new(
        perception: f64,
        world_modelling: f64,
        evaluation: f64,
        decision_capability: f64,
        feedback_integration: f64,
    ) -> Self {
        Self {
            perception: perception.into(),
            world_modelling: world_modelling.into(),
            evaluation: evaluation.into(),
            decision_capability: decision_capability.into(),
            feedback_integration: feedback_integration.into(),
        }
    }

    /// `A_g = P × M_o × E_v × D_c × F_b`
    pub fn agency_ratio(&self) -> Bounded {
        (self.perception.value()
            * self.world_modelling.value()
            * self.evaluation.value()
            * self.decision_capability.value()
            * self.feedback_integration.value())
        .into()
    }
}

impl Default for AgencyInputs {
    fn default() -> Self {
        Self::new(0.9, 0.85, 0.85, 0.9, 0.85)
    }
}

/// Summary agency state stored inside `CognitiveState`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgencyState {
    pub agency_ratio: Bounded,
    pub inputs: AgencyInputs,
}

impl AgencyState {
    pub fn compute(inputs: AgencyInputs) -> Self {
        let ratio = inputs.agency_ratio();
        Self { agency_ratio: ratio, inputs }
    }
}

impl Default for AgencyState {
    fn default() -> Self {
        let inputs = AgencyInputs::default();
        Self::compute(inputs)
    }
}
