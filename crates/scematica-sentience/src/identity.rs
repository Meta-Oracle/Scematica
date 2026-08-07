//! §16  Identity Continuity
//!
//! ```text
//! Identity_{t+1} = Identity_t + Memory_t + Experience_t + ValueState_t
//! subject to: Consistency(Identity_{t+1}, Identity_t)
//! ```
//!
//! The architecture preserves historical provenance — it does NOT rewrite its own history.

use serde::{Deserialize, Serialize};
use crate::types::{Bounded, Timestep};

/// A value held by the cognitive system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Value {
    pub name: String,
    pub strength: Bounded,
}

/// Immutable historical entry — history must not be rewritten.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub timestep: Timestep,
    pub description: String,
    pub identity_snapshot_hash: u64,
}

/// Identity state — continuity across time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityState {
    pub core_values: Vec<Value>,
    pub history: Vec<HistoryEntry>,
    /// Consistency score between current and previous identity `[0,1]`.
    pub continuity: Bounded,
    pub created_at: Timestep,
}

impl IdentityState {
    pub fn new(core_values: Vec<Value>) -> Self {
        Self {
            core_values,
            history: vec![],
            continuity: Bounded::ONE,
            created_at: 0,
        }
    }

    /// Record a state transition — append-only, never overwrite.
    pub fn record(&mut self, timestep: Timestep, description: impl Into<String>, hash: u64) {
        self.history.push(HistoryEntry {
            timestep,
            description: description.into(),
            identity_snapshot_hash: hash,
        });
    }

    /// Compute continuity as fraction of core values that remain stable.
    pub fn update_continuity(&mut self, prev_values: &[Value]) {
        if prev_values.is_empty() {
            self.continuity = Bounded::ONE;
            return;
        }
        let stable = prev_values
            .iter()
            .filter(|pv| {
                self.core_values
                    .iter()
                    .any(|cv| cv.name == pv.name && (cv.strength.value() - pv.strength.value()).abs() < 0.1)
            })
            .count();
        self.continuity = (stable as f64 / prev_values.len() as f64).into();
    }
}
