//! Provenance tracking for propositions — §4 axiom that every reasoning chain
//! maintains `P_i = (source, evidence, inference, confidence)`.

pub use crate::types::Provenance;

/// A chain of reasoning steps, each with full provenance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ProvenanceChain {
    pub steps: Vec<crate::types::Provenance>,
}

impl ProvenanceChain {
    pub fn new() -> Self { Self::default() }

    pub fn push(&mut self, step: crate::types::Provenance) {
        self.steps.push(step);
    }

    /// Overall chain confidence = product of individual step confidences.
    pub fn chain_confidence(&self) -> f64 {
        self.steps
            .iter()
            .map(|s| s.confidence.value())
            .product()
    }
}
