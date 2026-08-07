//! §23 / §27  Compound Cognitive Equation & Master Equation
//!
//! ```text
//! Ψ_t = S_t × I_t × K_t × MC_t × A_g_t × F_t
//!
//! Ω_{t+1} = F(Ω_t, Perception_t, Memory_t, Reasoning_t,
//!              Ethics_t, Action_t, Feedback_t)
//! ```

use serde::{Deserialize, Serialize};
use crate::{
    agency::AgencyInputs,
    ethics::EthicsInputs,
    logic::LogicInputs,
    meta_cognition::MetaCognitionInputs,
    perception::Perception,
    rationality::RationalityInputs,
    sentience::SentienceIndex,
    types::Bounded,
};

/// Ψ_t — Integrated cognitive state scalar.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct IntegratedCognition {
    pub psi: Bounded,
    pub sentience: Bounded,
    pub information: Bounded,
    pub knowledge: Bounded,
    pub meta_cognition: Bounded,
    pub agency: Bounded,
    pub feedback: Bounded,
}

impl IntegratedCognition {
    /// Compute Ψ_t = S_t × I_t × K_t × MC_t × A_g_t × F_t.
    pub fn compute(
        s: Bounded,
        i: Bounded,
        k: Bounded,
        mc: Bounded,
        ag: Bounded,
        f: Bounded,
    ) -> Self {
        let psi = (s.value() * i.value() * k.value() * mc.value() * ag.value() * f.value()).into();
        Self { psi, sentience: s, information: i, knowledge: k, meta_cognition: mc, agency: ag, feedback: f }
    }
}

/// Master equation builder — assembles all subsystem inputs into Ψ_t.
pub struct MasterEquation;

impl MasterEquation {
    pub fn compute(
        rationality: &RationalityInputs,
        logic: &LogicInputs,
        ethics: &EthicsInputs,
        perception: &Perception,
        agency: &AgencyInputs,
        meta: &MetaCognitionInputs,
        knowledge_density: Bounded,
        feedback: Bounded,
    ) -> (SentienceIndex, IntegratedCognition) {
        let sentience = SentienceIndex::compute(rationality, logic, ethics, perception);
        let information = perception.data_ratio();
        let mc = meta.meta_cognition_ratio();
        let ag = agency.agency_ratio();
        let psi = IntegratedCognition::compute(
            sentience.value,
            information,
            knowledge_density,
            mc,
            ag,
            feedback,
        );
        (sentience, psi)
    }
}
