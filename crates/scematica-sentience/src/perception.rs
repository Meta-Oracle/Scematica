//! §2  Data / Perception Equation
//!
//! ```text
//! D = A_aud × Vis × X × I
//! ```
//!
//! Each channel is normalized to `[0,1]`.  The multiplicative form ensures that a
//! completely absent channel cannot be compensated by boosting the others.

use serde::{Deserialize, Serialize};

use crate::types::Bounded;

/// Raw sensory / perceptual inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Perception {
    /// Audio perception quality (`A_aud`).
    pub audio: Bounded,
    /// Visual perception quality (`Vis`).
    pub visual: Bounded,
    /// General / environmental sensory input (`X`).
    pub sensory: Bounded,
    /// Information / data integrity (`I`).
    pub integrity: Bounded,
}

impl Perception {
    pub fn new(audio: f64, visual: f64, sensory: f64, integrity: f64) -> Self {
        Self {
            audio: audio.into(),
            visual: visual.into(),
            sensory: sensory.into(),
            integrity: integrity.into(),
        }
    }

    /// Compute the **Data Ratio** `D = A_aud × Vis × X × I`.
    pub fn data_ratio(&self) -> Bounded {
        (self.audio.value()
            * self.visual.value()
            * self.sensory.value()
            * self.integrity.value())
        .into()
    }
}

impl Default for Perception {
    /// A fully capable perception system.
    fn default() -> Self {
        Self::new(1.0, 1.0, 1.0, 1.0)
    }
}
