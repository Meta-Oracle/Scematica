//! §21  Contradiction Engine
//!
//! ```text
//! Conflict(P, ¬P) = Evidence(P) - Evidence(¬P)
//! ```
//!
//! The system retains {P, ¬P, Confidence(P), Confidence(¬P)} until evidence resolves
//! the contradiction.  It must NOT silently select one.

use serde::{Deserialize, Serialize};
use crate::types::Confidence;

/// State of a tracked contradiction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContradictionStatus {
    /// Both propositions are live; conflict is unresolved.
    Unresolved,
    /// Evidence weight favours P.
    FavoursPrimary,
    /// Evidence weight favours ¬P.
    FavoursNegation,
    /// Resolved — sufficient evidence has settled the question.
    Resolved { accepted: bool },
}

/// A tracked contradiction between P and ¬P.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contradiction {
    pub id: String,
    pub proposition: String,
    pub confidence_p: Confidence,
    pub confidence_neg_p: Confidence,
    pub status: ContradictionStatus,
}

impl Contradiction {
    pub fn new(
        id: impl Into<String>,
        proposition: impl Into<String>,
        conf_p: f64,
        conf_neg_p: f64,
    ) -> Self {
        let cp = Confidence::new(conf_p);
        let cn = Confidence::new(conf_neg_p);
        let status = Self::derive_status(cp, cn);
        Self {
            id: id.into(),
            proposition: proposition.into(),
            confidence_p: cp,
            confidence_neg_p: cn,
            status,
        }
    }

    fn derive_status(cp: Confidence, cn: Confidence) -> ContradictionStatus {
        let diff = cp.value() - cn.value();
        match diff {
            d if d.abs() < 0.1 => ContradictionStatus::Unresolved,
            d if d >= 0.1 && d < 0.7 => ContradictionStatus::FavoursPrimary,
            d if d <= -0.1 && d > -0.7 => ContradictionStatus::FavoursNegation,
            d if d >= 0.7 => ContradictionStatus::Resolved { accepted: true },
            _ => ContradictionStatus::Resolved { accepted: false },
        }
    }

    /// Update evidence and recompute status.
    pub fn update_evidence(&mut self, new_conf_p: f64, new_conf_neg_p: f64) {
        // Blend: new evidence shifts but doesn't replace old confidence.
        let blended_p = (self.confidence_p.value() + new_conf_p) / 2.0;
        let blended_n = (self.confidence_neg_p.value() + new_conf_neg_p) / 2.0;
        self.confidence_p = Confidence::new(blended_p);
        self.confidence_neg_p = Confidence::new(blended_n);
        self.status = Self::derive_status(self.confidence_p, self.confidence_neg_p);
    }

    /// Net conflict weight: positive favours P, negative favours ¬P.
    pub fn conflict_weight(&self) -> f64 {
        self.confidence_p.value() - self.confidence_neg_p.value()
    }
}
