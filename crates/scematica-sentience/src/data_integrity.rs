//! §2  Information / Data Integrity
//!
//! ```text
//! I = f(C_data, T, S_rel, R_cor)
//! ```
//!
//! Raw information must never automatically become truth.
//! Completeness, temporal relevance, source reliability, and corroboration
//! each reduce integrity multiplicatively.

use serde::{Deserialize, Serialize};

use crate::types::Bounded;

/// Inputs that determine information integrity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataIntegrityInputs {
    /// Completeness of the information (`C_data ∈ [0,1]`).
    pub completeness: Bounded,
    /// Temporal relevance — how fresh / applicable the data is (`T ∈ [0,1]`).
    pub temporal_relevance: Bounded,
    /// Source reliability score (`S_rel ∈ [0,1]`).
    pub source_reliability: Bounded,
    /// Redundancy / corroboration across independent sources (`R_cor ∈ [0,1]`).
    pub corroboration: Bounded,
}

impl DataIntegrityInputs {
    pub fn new(
        completeness: f64,
        temporal_relevance: f64,
        source_reliability: f64,
        corroboration: f64,
    ) -> Self {
        Self {
            completeness: completeness.into(),
            temporal_relevance: temporal_relevance.into(),
            source_reliability: source_reliability.into(),
            corroboration: corroboration.into(),
        }
    }

    /// Compute integrity `I` as the geometric mean of the four dimensions.
    ///
    /// Geometric mean is used instead of a product so that a single weak component
    /// degrades — but does not catastrophically zero — the result.  A true zero in
    /// any component still propagates to zero.
    pub fn integrity(&self) -> Bounded {
        let product = self.completeness.value()
            * self.temporal_relevance.value()
            * self.source_reliability.value()
            * self.corroboration.value();
        if product <= 0.0 {
            return Bounded::ZERO;
        }
        product.powf(0.25).into()
    }
}

impl Default for DataIntegrityInputs {
    fn default() -> Self {
        Self::new(1.0, 1.0, 1.0, 1.0)
    }
}
