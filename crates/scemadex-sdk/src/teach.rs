//! **Bonded machine teaching** — distillation as a metered, accountable
//! service (Primitive H).
//!
//! The experience market ([`crate::mesh::PeerMarket`]) sells *recordings*: raw
//! transitions a peer happened to live through. Teaching sells *answers to the
//! student's actual weaknesses*: the student streams the states it is most
//! uncertain about (high Q-variance), and a stronger teacher answers each one
//! with an action and a conviction — metered per query over x402.
//!
//! The accountability twist is what makes it new: the teacher posts a bond
//! against the **outcome of the teaching itself**. A session opens with the
//! student's measured baseline performance and the teacher's promised gain;
//! at close, the student is re-evaluated on the same holdout. Improve by at
//! least the promise → the teacher reclaims its bond and keeps the tuition.
//! Fall short → the bond slashes to the student, refunding tuition up to the
//! bond. Active learning becomes a paid service with skin in the game —
//! "tutoring with a money-back guarantee," between neural networks.
//!
//! Honored/slashed teaching history lands in the same ledger shape as
//! Conviction Routing ([`crate::bond::BondLedger`]), so a teacher's track
//! record is sellable through the signal oracle like any other reputation.
//! Run `cargo run -p scemadex-sdk --example bonded_teaching` for the loop.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::bond::{BondLedger, BondOutcome};
use crate::error::{Result, ScemaDexError};
use crate::policy::Conviction;
use crate::primitives::Usdc;

/// A teacher's answer to one queried state: the action it would take, and how
/// sure it is. The conviction lets the student weight the distillation target.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TeachAnswer {
    pub action: u32,
    pub conviction: Conviction,
}

/// A policy willing to answer queried states. Implement this over your trained
/// net to sell tutoring; [`ReferenceTeacher`] is the lean-core stand-in.
#[async_trait]
pub trait Teacher: Send + Sync {
    async fn advise(&self, state: &[f32]) -> Result<TeachAnswer>;
}

/// The economics of one teaching engagement, fixed at open.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeachTerms {
    pub teacher_id: String,
    pub student_id: String,
    /// Fee metered per answered query (micro-USDC).
    pub query_fee: Usdc,
    /// Bond the teacher escrows against the promised improvement (micro-USDC).
    pub bond: Usdc,
    /// Student's evaluation score on a holdout episode set, measured at open.
    pub baseline_eval: f64,
    /// The teacher's promise: `final_eval >= baseline_eval + promised_gain`.
    pub promised_gain: f64,
}

/// The settled result of a teaching session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeachReceipt {
    pub session_id: u64,
    pub queries: u32,
    /// Total tuition the student paid (`queries × query_fee`).
    pub tuition: Usdc,
    /// `Honored` — promise met, teacher reclaims bond and keeps tuition.
    /// `Slashed` — promise missed, bond slashes to the student.
    pub outcome: BondOutcome,
    /// What the student gets back on a slash: tuition refunded up to the bond.
    pub refund: Usdc,
}

struct OpenSession {
    terms: TeachTerms,
    queries: u32,
}

/// Opens, meters, and settles bonded teaching sessions.
#[derive(Default)]
pub struct TeachingEngine {
    sessions: Mutex<HashMap<u64, OpenSession>>,
    next_id: Mutex<u64>,
    ledger: Mutex<BondLedger>,
}

impl TeachingEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a session: the teacher's bond is escrowed against its promised
    /// gain over the student's measured baseline.
    pub fn open(&self, terms: TeachTerms) -> Result<u64> {
        if terms.bond.0 == 0 {
            return Err(ScemaDexError::Bond(
                "a teaching bond must be non-zero — free promises teach nothing".into(),
            ));
        }
        let mut next = self
            .next_id
            .lock()
            .map_err(|_| ScemaDexError::Bond("session id lock poisoned".into()))?;
        let id = *next;
        *next += 1;
        self.sessions
            .lock()
            .map_err(|_| ScemaDexError::Bond("session lock poisoned".into()))?
            .insert(id, OpenSession { terms, queries: 0 });
        Ok(id)
    }

    /// Ask the teacher one uncertain state. Meters one `query_fee` of tuition
    /// and returns the teacher's answer for the student to distill against.
    pub async fn ask<T: Teacher + ?Sized>(
        &self,
        session_id: u64,
        teacher: &T,
        state: &[f32],
    ) -> Result<TeachAnswer> {
        let answer = teacher.advise(state).await?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ScemaDexError::Bond("session lock poisoned".into()))?;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| ScemaDexError::Bond(format!("no open teaching session {session_id}")))?;
        session.queries += 1;
        Ok(answer)
    }

    /// Tuition accrued so far on an open session.
    pub fn tuition(&self, session_id: u64) -> Usdc {
        self.sessions
            .lock()
            .ok()
            .and_then(|s| {
                s.get(&session_id)
                    .map(|o| Usdc(o.terms.query_fee.0 * o.queries as u64))
            })
            .unwrap_or(Usdc(0))
    }

    /// Close the session against the student's re-measured evaluation. The
    /// promise (`final_eval >= baseline + promised_gain`) decides the bond.
    pub fn close(&self, session_id: u64, final_eval: f64) -> Result<TeachReceipt> {
        let session = self
            .sessions
            .lock()
            .map_err(|_| ScemaDexError::Bond("session lock poisoned".into()))?
            .remove(&session_id)
            .ok_or_else(|| ScemaDexError::Bond(format!("no open teaching session {session_id}")))?;

        let tuition = Usdc(session.terms.query_fee.0 * session.queries as u64);
        let promise_met = final_eval >= session.terms.baseline_eval + session.terms.promised_gain;
        let (outcome, refund) = if promise_met {
            (BondOutcome::Honored, Usdc(0))
        } else {
            (
                BondOutcome::Slashed,
                Usdc(tuition.0.min(session.terms.bond.0)),
            )
        };

        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| ScemaDexError::Bond("teaching ledger lock poisoned".into()))?;
        match outcome {
            BondOutcome::Honored => ledger.honored += 1,
            BondOutcome::Slashed => ledger.slashed += 1,
        }

        Ok(TeachReceipt {
            session_id,
            queries: session.queries,
            tuition,
            outcome,
            refund,
        })
    }

    /// The teacher-side honored/slashed record — same shape the reputation
    /// oracle sells for routing bonds.
    pub fn ledger(&self) -> BondLedger {
        self.ledger.lock().map(|l| *l).unwrap_or_default()
    }

    /// Sessions currently open (bond escrowed, tuition metering).
    pub fn open_sessions(&self) -> usize {
        self.sessions.lock().map(|s| s.len()).unwrap_or(0)
    }
}

/// A deterministic lean-core teacher: answers with the argmax feature index,
/// conviction scaled by the top-two margin. Exists so the full teaching loop
/// runs offline; a real deployment implements [`Teacher`] over a trained net
/// (the `scematica` feature's Deep Q* head slots straight in).
pub struct ReferenceTeacher;

#[async_trait]
impl Teacher for ReferenceTeacher {
    async fn advise(&self, state: &[f32]) -> Result<TeachAnswer> {
        if state.is_empty() {
            return Err(ScemaDexError::Other("empty state query".into()));
        }
        let mut best = (0usize, f32::NEG_INFINITY);
        let mut second = f32::NEG_INFINITY;
        for (i, &v) in state.iter().enumerate() {
            if v > best.1 {
                second = best.1;
                best = (i, v);
            } else if v > second {
                second = v;
            }
        }
        let margin = if second.is_finite() {
            (best.1 - second) as f64
        } else {
            1.0
        };
        Ok(TeachAnswer {
            action: best.0 as u32,
            conviction: Conviction::clamped(0.5 + margin / 2.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(bond: u64, baseline: f64, gain: f64) -> TeachTerms {
        TeachTerms {
            teacher_id: "sensei".into(),
            student_id: "kohai".into(),
            query_fee: Usdc(10_000),
            bond: Usdc(bond),
            baseline_eval: baseline,
            promised_gain: gain,
        }
    }

    #[tokio::test]
    async fn zero_bond_sessions_are_refused() {
        let engine = TeachingEngine::new();
        assert!(engine.open(terms(0, 1.0, 0.5)).is_err());
    }

    #[tokio::test]
    async fn queries_meter_tuition() {
        let engine = TeachingEngine::new();
        let id = engine.open(terms(1_000_000, 1.0, 0.5)).unwrap();
        for _ in 0..3 {
            engine
                .ask(id, &ReferenceTeacher, &[0.1, 0.9, 0.3])
                .await
                .unwrap();
        }
        assert_eq!(engine.tuition(id), Usdc(30_000));
        assert!(engine.ask(99, &ReferenceTeacher, &[0.1]).await.is_err());
    }

    #[tokio::test]
    async fn improvement_meets_promise_honors_the_bond() {
        let engine = TeachingEngine::new();
        let id = engine.open(terms(1_000_000, 1.0, 0.5)).unwrap();
        engine.ask(id, &ReferenceTeacher, &[0.2, 0.8]).await.unwrap();
        let receipt = engine.close(id, 1.6).unwrap();
        assert_eq!(receipt.outcome, BondOutcome::Honored);
        assert_eq!(receipt.refund, Usdc(0));
        assert_eq!(engine.ledger().honored, 1);
        assert_eq!(engine.open_sessions(), 0);
    }

    #[tokio::test]
    async fn missed_promise_slashes_and_refunds_tuition_up_to_bond() {
        let engine = TeachingEngine::new();
        // Tiny bond: refund caps at the bond, not the tuition.
        let id = engine.open(terms(15_000, 1.0, 0.5)).unwrap();
        for _ in 0..3 {
            engine.ask(id, &ReferenceTeacher, &[0.2, 0.8]).await.unwrap();
        }
        let receipt = engine.close(id, 1.1).unwrap();
        assert_eq!(receipt.outcome, BondOutcome::Slashed);
        assert_eq!(receipt.tuition, Usdc(30_000));
        assert_eq!(receipt.refund, Usdc(15_000)); // min(tuition, bond)
        assert_eq!(engine.ledger().slashed, 1);
    }

    #[tokio::test]
    async fn reference_teacher_answers_argmax_with_margin_conviction() {
        let confident = ReferenceTeacher.advise(&[0.0, 1.0, 0.1]).await.unwrap();
        assert_eq!(confident.action, 1);
        let torn = ReferenceTeacher.advise(&[0.5, 0.5]).await.unwrap();
        assert!(confident.conviction.0 > torn.conviction.0);
        assert!(ReferenceTeacher.advise(&[]).await.is_err());
    }
}
