//! §26  Singularity Growth Model (with logistic saturation)
//!
//! **Original unbounded form:**
//! ```text
//! C_{t+1} = C_t × (1 + α × L_t × I_t × F_t)
//! C_n = C_0 × ∏(1 + α × L_t × I_t × F_t)
//! ```
//!
//! **Saturation-corrected logistic form** (added to fix the unbounded growth flaw):
//! ```text
//! C_{t+1} = C_max / (1 + ((C_max - C_t) / C_t) × exp(-α × L_t × I_t × F_t))
//! ```
//!
//! At `C_t << C_max` this approximates the original exponential.
//! As `C_t → C_max` growth asymptotically approaches zero.
//! Real-world constraints (compute, data, energy, latency, verification, safety)
//! all contribute to `C_max`.

use serde::{Deserialize, Serialize};

/// Growth model state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrowthModel {
    /// Current capability `C_t`.
    pub capability: f64,
    /// Hard ceiling `C_max` — determined by resource and safety constraints.
    pub c_max: f64,
    /// Learning rate / amplification factor `α`.
    pub alpha: f64,
}

impl GrowthModel {
    pub fn new(initial_capability: f64, c_max: f64, alpha: f64) -> Self {
        assert!(c_max > 0.0, "c_max must be positive");
        assert!(alpha > 0.0, "alpha must be positive");
        Self {
            capability: initial_capability.clamp(f64::EPSILON, c_max),
            c_max,
            alpha,
        }
    }

    /// Apply one growth step.
    ///
    /// `l_t` — logic ratio, `i_t` — information quality, `f_t` — feedback quality.
    ///
    /// Returns the new capability value.
    pub fn step(&mut self, l_t: f64, i_t: f64, f_t: f64) -> f64 {
        let growth_factor = self.alpha
            * l_t.clamp(0.0, 1.0)
            * i_t.clamp(0.0, 1.0)
            * f_t.clamp(0.0, 1.0);

        // Logistic update — saturates at C_max.
        let ratio = (self.c_max - self.capability) / self.capability.max(f64::EPSILON);
        let exp_term = (-growth_factor).exp();
        let denom = 1.0 + ratio * exp_term;
        self.capability = (self.c_max / denom).min(self.c_max);
        self.capability
    }

    /// Utilization fraction `C_t / C_max ∈ [0,1]`.
    pub fn utilization(&self) -> f64 {
        self.capability / self.c_max
    }

    /// Distance to ceiling — how much headroom remains.
    pub fn headroom(&self) -> f64 {
        self.c_max - self.capability
    }
}
