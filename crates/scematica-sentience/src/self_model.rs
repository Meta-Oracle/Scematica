//! §15  Self-Model
//!
//! ```text
//! Self_t = (K_t, M_t, G_t, C_t, U_t, L_t)
//! SelfAwareness = Knowledge(Self) + Uncertainty(Self) + Capability(Self) + Limitation(Self)
//! ```
//!
//! This is a **computational self-model**, not proof of phenomenal consciousness.
//! The system must explicitly represent its own limitations.

use serde::{Deserialize, Serialize};
use crate::types::Bounded;

/// Known capability descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub proficiency: Bounded,
}

/// Known limitation descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limitation {
    pub name: String,
    /// How severe / impactful is this limitation `[0,1]`.
    pub severity: Bounded,
    pub description: String,
}

/// The system's model of itself at time `t`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfModel {
    /// Knowledge density summary `K_t`.
    pub knowledge: Bounded,
    /// Memory utilization `M_t`.
    pub memory: Bounded,
    /// Active goals count (normalized).
    pub goals_active: Bounded,
    /// Known capabilities `C_t`.
    pub capabilities: Vec<Capability>,
    /// Epistemic uncertainty `U_t`.
    pub uncertainty: Bounded,
    /// Known limitations `L_t` — must be explicit.
    pub limitations: Vec<Limitation>,
}

impl SelfModel {
    /// Scalar self-awareness index.
    /// `SelfAwareness = mean(Knowledge, 1-Uncertainty, avg_capability, 1-avg_limitation_severity)`
    pub fn self_awareness_index(&self) -> Bounded {
        let knowledge = self.knowledge.value();
        let uncertainty_inv = 1.0 - self.uncertainty.value();
        let capability_avg = if self.capabilities.is_empty() {
            0.5
        } else {
            self.capabilities.iter().map(|c| c.proficiency.value()).sum::<f64>()
                / self.capabilities.len() as f64
        };
        let limitation_avg = if self.limitations.is_empty() {
            0.0
        } else {
            self.limitations.iter().map(|l| l.severity.value()).sum::<f64>()
                / self.limitations.len() as f64
        };
        ((knowledge + uncertainty_inv + capability_avg + (1.0 - limitation_avg)) / 4.0).into()
    }
}

impl Default for SelfModel {
    fn default() -> Self {
        Self {
            knowledge: 0.5.into(),
            memory: 0.1.into(),
            goals_active: 0.0.into(),
            capabilities: vec![],
            uncertainty: 0.5.into(),
            limitations: vec![Limitation {
                name: "bounded_knowledge".into(),
                severity: 0.5.into(),
                description: "Knowledge is incomplete and may contain errors.".into(),
            }],
        }
    }
}
