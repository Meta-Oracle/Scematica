//! §19  Curiosity / Exploration
//!
//! ```text
//! Curiosity(a) = H(K_t) - H(K_t | a)
//! subject to: Curiosity(a) ≤ Safety(a)
//! ```
//!
//! Curiosity is information gain — the system prefers actions that reduce meaningful
//! uncertainty.  Safety constraints are always enforced first.

use serde::{Deserialize, Serialize};

/// Information gain from taking action `a`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuriosityScore {
    pub action_id: String,
    /// Current knowledge entropy `H(K_t)`.
    pub prior_entropy: f64,
    /// Expected posterior entropy `H(K_t | a)`.
    pub posterior_entropy: f64,
    /// Safety budget — curiosity is capped to this value.
    pub safety_budget: f64,
}

impl CuriosityScore {
    /// Information gain, capped by safety budget.
    pub fn curiosity(&self) -> f64 {
        let raw = (self.prior_entropy - self.posterior_entropy).max(0.0);
        raw.min(self.safety_budget)
    }

    /// Whether the action is safe to pursue for curiosity.
    pub fn is_safe(&self) -> bool {
        self.safety_budget > 0.0
    }
}
