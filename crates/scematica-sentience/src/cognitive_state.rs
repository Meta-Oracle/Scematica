//! §6  Cognitive State
//!
//! ```text
//! Ω_t = (S_t, R_t, L_t, M_t, D_t, K_t, E_t, G_t, U_t, A_g_t)
//! ```
//!
//! Sentience cannot be represented solely by a static scalar.
//! This module defines the full state vector and its update rule.

use serde::{Deserialize, Serialize};

use crate::{
    agency::AgencyState,
    ethics::EthicsInputs,
    logic::LogicInputs,
    perception::Perception,
    rationality::RationalityInputs,
    sentience::SentienceIndex,
    types::{Bounded, Observation, Timestep},
};

/// Goal descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub description: String,
    /// Priority weight in `[0,1]`.
    pub priority: Bounded,
    pub active: bool,
}

/// Complete cognitive state at timestep `t`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveState {
    pub timestep: Timestep,
    /// Integrated sentience index `S_t`.
    pub sentience: SentienceIndex,
    /// Rationality ratio `R_t`.
    pub rationality: RationalityInputs,
    /// Logic ratio `L_t`.
    pub logic: LogicInputs,
    /// Moral-ethical ratio `M_t`.
    pub ethics: EthicsInputs,
    /// Perceptual / data ratio `D_t`.
    pub perception: Perception,
    /// Knowledge state (scalar summary; full graph in `knowledge_graph`).
    pub knowledge_density: Bounded,
    /// Episodic memory depth (fraction of capacity used).
    pub memory_depth: Bounded,
    /// Active goal set `G_t`.
    pub goals: Vec<Goal>,
    /// Overall uncertainty `U_t` — higher = more uncertain.
    pub uncertainty: Bounded,
    /// Agency state `A_g_t`.
    pub agency: AgencyState,
    /// Latest environmental observation `O_t`.
    pub last_observation: Option<Observation>,
}

impl CognitiveState {
    /// Build a baseline cognitive state at timestep 0.
    pub fn initial() -> Self {
        Self {
            timestep: 0,
            sentience: SentienceIndex::compute(
                &RationalityInputs::default(),
                &LogicInputs::default(),
                &EthicsInputs::default(),
                &Perception::default(),
            ),
            rationality: RationalityInputs::default(),
            logic: LogicInputs::default(),
            ethics: EthicsInputs::default(),
            perception: Perception::default(),
            knowledge_density: 0.5.into(),
            memory_depth: 0.0.into(),
            goals: vec![],
            uncertainty: 0.5.into(),
            agency: AgencyState::default(),
            last_observation: None,
        }
    }

    /// Advance the timestep counter.
    pub fn tick(&mut self) {
        self.timestep += 1;
    }
}
