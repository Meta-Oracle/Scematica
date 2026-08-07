//! §13  Decision Function
//!
//! ```text
//! U(a_i) = B_i - H_i - R_i + L_i + G_i
//! a* = argmax U(a_i)  subject to Ethics(a_i)=1, Safety(a_i)=1, Constraints(a_i)=1
//! ```

use serde::{Deserialize, Serialize};

/// A candidate action with its utility components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateAction {
    pub id: String,
    pub description: String,
    /// Expected benefit `B_i`.
    pub benefit: f64,
    /// Expected harm `H_i`.
    pub harm: f64,
    /// Risk `R_i`.
    pub risk: f64,
    /// Learning value — how much this action reduces uncertainty `L_i`.
    pub learning_value: f64,
    /// Goal alignment score `G_i ∈ [0,1]`.
    pub goal_alignment: f64,
    // Hard gates — all must be true for the action to be selectable.
    pub ethics_gate: bool,
    pub safety_gate: bool,
    pub system_constraints_gate: bool,
}

impl CandidateAction {
    /// Net utility `U(a_i) = B_i - H_i - R_i + L_i + G_i`.
    /// Returns `None` if any hard gate fails.
    pub fn utility(&self) -> Option<f64> {
        if !self.ethics_gate || !self.safety_gate || !self.system_constraints_gate {
            return None; // P(a) = 0
        }
        Some(self.benefit - self.harm - self.risk + self.learning_value + self.goal_alignment)
    }

    /// Whether this action clears all hard constraints.
    pub fn is_permitted(&self) -> bool {
        self.ethics_gate && self.safety_gate && self.system_constraints_gate
    }
}

/// Select the optimal permitted action.
pub fn select_action(candidates: &[CandidateAction]) -> Option<&CandidateAction> {
    candidates
        .iter()
        .filter(|a| a.is_permitted())
        .filter_map(|a| a.utility().map(|u| (a, u)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(a, _)| a)
}
