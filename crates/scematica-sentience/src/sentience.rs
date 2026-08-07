//! §1 / §29  Primary Sentience Equation
//!
//! ```text
//! S = R × L × M × D
//! D = A_aud × Vis × X × I
//! ∴ S = R × L × M × (A_aud × Vis × X × I)
//! ```
//!
//! Multiplicative structure is **intentional**: a system cannot compensate for a
//! fundamental cognitive deficiency by increasing another dimension.
//!
//! `S` is a **functional cognitive coherence index**, not a claim of consciousness.

use serde::{Deserialize, Serialize};

use crate::{
    ethics::EthicsInputs,
    logic::LogicInputs,
    perception::Perception,
    rationality::RationalityInputs,
    types::Bounded,
};

/// The primary sentience index `S_t ∈ [0,1]`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SentienceIndex {
    pub value: Bounded,
    /// Component breakdown for introspection.
    pub rationality: Bounded,
    pub logic: Bounded,
    pub moral: Bounded,
    pub data: Bounded,
}

impl SentienceIndex {
    /// Compute `S = R × L × M × D`.
    pub fn compute(
        r: &RationalityInputs,
        l: &LogicInputs,
        m: &EthicsInputs,
        d: &Perception,
    ) -> Self {
        let rationality = r.rationality();
        let logic = l.logic_ratio();
        let moral = m.moral_ratio();
        let data = d.data_ratio();
        let value = (rationality.value() * logic.value() * moral.value() * data.value()).into();
        Self { value, rationality, logic, moral, data }
    }

    /// Bottleneck component — the dimension most limiting sentience.
    pub fn bottleneck(&self) -> &'static str {
        let components = [
            (self.rationality.value(), "rationality"),
            (self.logic.value(), "logic"),
            (self.moral.value(), "moral"),
            (self.data.value(), "data"),
        ];
        components
            .iter()
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
            .map(|(_, name)| *name)
            .unwrap_or("unknown")
    }
}
