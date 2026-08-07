//! Shared primitives used across the cognitive architecture.

use serde::{Deserialize, Serialize};

/// A normalized scalar in `[0, 1]`.
/// Values outside this range are clamped on construction.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Bounded(f64);

impl Bounded {
    /// Clamp `v` into `[0, 1]`.
    pub fn new(v: f64) -> Self {
        Self(v.clamp(0.0, 1.0))
    }

    /// Raw `f64` value.
    pub fn value(self) -> f64 {
        self.0
    }

    /// Zero — represents complete absence / failure.
    pub const ZERO: Self = Self(0.0);
    /// One — represents maximum capacity.
    pub const ONE: Self = Self(1.0);
}

impl From<f64> for Bounded {
    fn from(v: f64) -> Self {
        Self::new(v)
    }
}

impl From<Bounded> for f64 {
    fn from(b: Bounded) -> Self {
        b.0
    }
}

impl std::fmt::Display for Bounded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.4}", self.0)
    }
}

/// Confidence value in `[0, 1]`.
/// `C(P) = 1` does NOT mean `P = True`; it means current evidence strongly supports P.
pub type Confidence = Bounded;

/// Provenance record attached to any proposition or inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub source: String,
    pub evidence: String,
    pub inference: String,
    pub confidence: Confidence,
    pub timestamp_ms: i64,
}

impl Provenance {
    pub fn new(
        source: impl Into<String>,
        evidence: impl Into<String>,
        inference: impl Into<String>,
        confidence: f64,
    ) -> Self {
        Self {
            source: source.into(),
            evidence: evidence.into(),
            inference: inference.into(),
            confidence: Confidence::new(confidence),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Timestep counter.
pub type Timestep = u64;

/// Generic outcome observation `O_t`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub value: f64,
    pub confidence: Confidence,
    pub provenance: Option<Provenance>,
    pub timestep: Timestep,
}

/// Learning rate `α ∈ (0, 1]`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LearningRate(f64);

impl LearningRate {
    pub fn new(v: f64) -> Self {
        Self(v.clamp(f64::EPSILON, 1.0))
    }
    pub fn value(self) -> f64 {
        self.0
    }
}

impl Default for LearningRate {
    fn default() -> Self {
        Self(0.1)
    }
}
