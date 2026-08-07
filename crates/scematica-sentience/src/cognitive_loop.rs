//! §24  Recursive Cognitive Loop
//!
//! ```text
//! Ω_t → Perception → Integration → Reasoning → Prediction → EthicalEvaluation
//!      → Decision → Action → Observation → Error → Learning → Memory
//!      → SelfModel → Ω_{t+1}
//! ```

use serde::{Deserialize, Serialize};
use crate::{
    cognitive_state::CognitiveState,
    error_correction::ErrorCorrector,
    learning::Learner,
    sentience::SentienceIndex,
    master_equation::MasterEquation,
    types::{Bounded, Observation},
};

/// Output of one full cognitive cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleOutput {
    pub timestep: u64,
    pub sentience: SentienceIndex,
    pub psi: Bounded,
    pub learning_delta: f64,
    pub error: f64,
    pub reassessment_triggered: bool,
}

/// Drives the recursive Ω_{t+1} = F(Ω_t, ...) loop.
pub struct CognitiveLoop {
    pub state: CognitiveState,
    pub learner: Learner,
    pub error_corrector: ErrorCorrector,
}

impl CognitiveLoop {
    pub fn new(state: CognitiveState) -> Self {
        Self {
            state,
            learner: Learner::default(),
            error_corrector: ErrorCorrector::default(),
        }
    }

    /// Run one full cognitive cycle given an observation and predicted value.
    pub fn step(&mut self, observation: Observation, predicted: f64, feedback: f64) -> CycleOutput {
        // 1. Learning
        let update = self.learner.update(&observation, predicted);

        // 2. Error correction
        let correction = self.error_corrector.step(
            observation.value,
            predicted,
            observation.confidence,
        );

        // 3. Master equation
        let (sentience, psi) = MasterEquation::compute(
            &self.state.rationality,
            &self.state.logic,
            &self.state.ethics,
            &self.state.perception,
            &self.state.agency.inputs,
            &crate::meta_cognition::MetaCognitionInputs::default(),
            self.state.knowledge_density,
            Bounded::new(feedback),
        );

        // 4. Update state
        self.state.sentience = sentience;
        self.state.last_observation = Some(observation);
        self.state.tick();

        CycleOutput {
            timestep: self.state.timestep,
            sentience,
            psi: psi.psi,
            learning_delta: update.delta,
            error: correction.raw_error,
            reassessment_triggered: update.triggered_reassessment,
        }
    }
}
