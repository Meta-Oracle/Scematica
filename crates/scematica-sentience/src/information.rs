//! §7  Information Integration
//!
//! ```text
//! I_t  = Σ w_i x_i
//! I_t* = Σ w_i x_i + γ Σ_{i≠j} W_ij (x_i x_j)
//! ```
//!
//! The second-order term captures that information nodes generate additional meaning
//! through their relationships — not just their individual values.

use serde::{Deserialize, Serialize};

/// A single information node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfoNode {
    pub id: String,
    pub value: f64,
    /// Weight of this node in the integration sum.
    pub weight: f64,
}

/// A directed relationship between two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRelation {
    pub from_id: String,
    pub to_id: String,
    /// Relationship strength `W_ij`.
    pub strength: f64,
}

/// Information integration engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InformationIntegrator {
    pub nodes: Vec<InfoNode>,
    pub relations: Vec<NodeRelation>,
    /// Integration coefficient `γ ∈ [0,1]`.
    pub gamma: f64,
}

impl InformationIntegrator {
    pub fn new(gamma: f64) -> Self {
        Self {
            nodes: vec![],
            relations: vec![],
            gamma: gamma.clamp(0.0, 1.0),
        }
    }

    /// First-order integrated information `I_t = Σ w_i x_i`.
    pub fn first_order(&self) -> f64 {
        self.nodes.iter().map(|n| n.weight * n.value).sum()
    }

    /// Second-order integrated information `I_t*`.
    pub fn second_order(&self) -> f64 {
        let first = self.first_order();

        // Build a lookup for node values.
        let lookup: std::collections::HashMap<&str, f64> =
            self.nodes.iter().map(|n| (n.id.as_str(), n.value)).collect();

        let interaction: f64 = self
            .relations
            .iter()
            .filter_map(|r| {
                let xi = lookup.get(r.from_id.as_str())?;
                let xj = lookup.get(r.to_id.as_str())?;
                Some(r.strength * xi * xj)
            })
            .sum();

        first + self.gamma * interaction
    }
}
