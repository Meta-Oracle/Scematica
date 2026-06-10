//! **Experience royalties** — training data that pays dividends (Primitive G).
//!
//! The [`crate::mesh::PeerMarket`] sells experience as a one-shot good: pay
//! once, train, done. This module makes experience a **yield-bearing asset**.
//! Every [`ExperienceBatch`] has a stable content digest; when an agent trains
//! on a purchased batch, the [`LineageLedger`] records the provenance. When
//! that agent later earns inference fees, a royalty slice streams back *up the
//! lineage* to the sellers whose experience shaped it — pro-rata by how many
//! transitions each contributed.
//!
//! The incentive flip is the point: a seller's best strategy is no longer to
//! dump junk transitions for a quick fee, but to sell its **best** experience —
//! because a successful student is an annuity. "Data dividends" have been
//! discussed in AI-economics papers for years; this is the first place the
//! required pieces (provenance, micropayments, measurable downstream earnings)
//! coexist in one settlement loop.
//!
//! Attribution here is the pragmatic v1 — pro-rata by transition count. The
//! ledger is deliberately separate from the fee engine so a smarter attribution
//! (TD-error improvement measured at purchase time) can replace the weights
//! without touching settlement. Run `cargo run -p scemadex-sdk --example
//! experience_royalties` for the loop.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{Result, ScemaDexError};
use crate::mesh::ExperienceBatch;
use crate::primitives::Usdc;

/// One ancestor in a student's training lineage: who sold the batch, its
/// content digest, and its attribution weight (transition count in v1).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LineageEntry {
    pub batch_digest: String,
    pub seller_id: String,
    pub weight: u64,
}

/// How one earned fee was divided between the earning agent and its lineage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoyaltySplit {
    /// What the earning agent keeps (fee minus royalties, plus rounding dust).
    pub student_keeps: Usdc,
    /// `(seller_id, royalty)` payouts up the lineage.
    pub payouts: Vec<(String, Usdc)>,
}

/// Records which experience trained which agent, and streams royalties from
/// downstream fees back to the sellers.
#[derive(Default)]
pub struct LineageLedger {
    lineages: Mutex<HashMap<String, Vec<LineageEntry>>>,
    earned: Mutex<HashMap<String, u64>>,
}

impl LineageLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `student_id` trained on `batch`. Call this at the moment of
    /// consumption (right after [`crate::mesh::PeerMarket::buy_experience`]).
    pub fn record_training(&self, student_id: impl Into<String>, batch: &ExperienceBatch) -> Result<()> {
        let entry = LineageEntry {
            batch_digest: batch.digest(),
            seller_id: batch.peer_id.clone(),
            weight: batch.transitions as u64,
        };
        self.lineages
            .lock()
            .map_err(|_| ScemaDexError::Mesh("lineage lock poisoned".into()))?
            .entry(student_id.into())
            .or_default()
            .push(entry);
        Ok(())
    }

    /// The recorded ancestry of an agent's policy.
    pub fn lineage(&self, student_id: &str) -> Vec<LineageEntry> {
        self.lineages
            .lock()
            .ok()
            .and_then(|l| l.get(student_id).cloned())
            .unwrap_or_default()
    }

    /// A stable root over the lineage's batch digests, in consumption order —
    /// the checkpoint-stampable provenance of a policy. (FNV-fold in the lean
    /// core; an on-chain settler would use a real Merkle tree.)
    pub fn lineage_root(&self, student_id: &str) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for entry in self.lineage(student_id) {
            for b in entry.batch_digest.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        format!("{h:016x}")
    }

    /// Split a fee the student just earned between itself and its lineage.
    ///
    /// `royalty_bps` of the fee forms the royalty pool, divided pro-rata by
    /// entry weight. Batches the student sold to itself earn nothing —
    /// self-dealing can't farm royalties. Integer-division dust stays with the
    /// student, so value is conserved: `student_keeps + Σ payouts == fee`.
    pub fn distribute(
        &self,
        student_id: &str,
        fee: Usdc,
        royalty_bps: u32,
    ) -> Result<RoyaltySplit> {
        let lineage = self.lineage(student_id);
        let pool = fee.0.saturating_mul(royalty_bps.min(10_000) as u64) / 10_000;
        let total_weight: u64 = lineage
            .iter()
            .filter(|e| e.seller_id != student_id)
            .map(|e| e.weight)
            .sum();

        let mut payouts: Vec<(String, Usdc)> = Vec::new();
        let mut paid = 0u64;
        if total_weight > 0 {
            // Aggregate per seller so a seller of many batches gets one payout.
            let mut by_seller: HashMap<String, u64> = HashMap::new();
            for e in lineage.iter().filter(|e| e.seller_id != student_id) {
                *by_seller.entry(e.seller_id.clone()).or_default() += e.weight;
            }
            let mut sellers: Vec<_> = by_seller.into_iter().collect();
            sellers.sort(); // deterministic payout order
            for (seller, weight) in sellers {
                let royalty = pool
                    .saturating_mul(weight)
                    .checked_div(total_weight)
                    .unwrap_or(0);
                if royalty > 0 {
                    paid += royalty;
                    payouts.push((seller, Usdc(royalty)));
                }
            }
            let mut earned = self
                .earned
                .lock()
                .map_err(|_| ScemaDexError::Mesh("royalty ledger lock poisoned".into()))?;
            for (seller, royalty) in &payouts {
                *earned.entry(seller.clone()).or_default() += royalty.0;
            }
        }

        Ok(RoyaltySplit {
            student_keeps: Usdc(fee.0 - paid),
            payouts,
        })
    }

    /// Cumulative royalties a seller has earned across every student and fee.
    pub fn royalties_earned(&self, seller_id: &str) -> Usdc {
        Usdc(
            self.earned
                .lock()
                .ok()
                .and_then(|e| e.get(seller_id).copied())
                .unwrap_or(0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(seller: &str, transitions: u32) -> ExperienceBatch {
        ExperienceBatch {
            peer_id: seller.into(),
            transitions,
            price: Usdc(1_000),
            payload: vec![transitions as u8],
        }
    }

    #[test]
    fn batch_digest_is_stable_and_content_sensitive() {
        let a = batch("alpha", 100);
        assert_eq!(a.digest(), batch("alpha", 100).digest());
        assert_ne!(a.digest(), batch("alpha", 101).digest());
    }

    #[test]
    fn no_lineage_means_student_keeps_everything() {
        let ledger = LineageLedger::new();
        let split = ledger.distribute("solo", Usdc(1_000_000), 1_000).unwrap();
        assert_eq!(split.student_keeps, Usdc(1_000_000));
        assert!(split.payouts.is_empty());
    }

    #[test]
    fn royalties_split_prorata_by_transitions_and_conserve_value() {
        let ledger = LineageLedger::new();
        ledger.record_training("student", &batch("alpha", 7_500)).unwrap();
        ledger.record_training("student", &batch("beta", 2_500)).unwrap();

        // 10% royalty on a 1 USDC fee → 100_000 µUSDC pool.
        let split = ledger.distribute("student", Usdc(1_000_000), 1_000).unwrap();
        assert_eq!(split.payouts.len(), 2);
        assert_eq!(split.payouts[0], ("alpha".into(), Usdc(75_000)));
        assert_eq!(split.payouts[1], ("beta".into(), Usdc(25_000)));
        let total: u64 = split.payouts.iter().map(|(_, p)| p.0).sum();
        assert_eq!(split.student_keeps.0 + total, 1_000_000);

        // Royalties accrue across fees — the annuity.
        ledger.distribute("student", Usdc(1_000_000), 1_000).unwrap();
        assert_eq!(ledger.royalties_earned("alpha"), Usdc(150_000));
    }

    #[test]
    fn self_sold_batches_earn_no_royalties() {
        let ledger = LineageLedger::new();
        ledger.record_training("student", &batch("student", 10_000)).unwrap();
        let split = ledger.distribute("student", Usdc(1_000_000), 1_000).unwrap();
        assert!(split.payouts.is_empty());
        assert_eq!(split.student_keeps, Usdc(1_000_000));
    }

    #[test]
    fn same_seller_batches_aggregate_into_one_payout() {
        let ledger = LineageLedger::new();
        ledger.record_training("student", &batch("alpha", 1_000)).unwrap();
        ledger.record_training("student", &batch("alpha", 3_000)).unwrap();
        let split = ledger.distribute("student", Usdc(1_000_000), 1_000).unwrap();
        assert_eq!(split.payouts, vec![("alpha".into(), Usdc(100_000))]);
    }

    #[test]
    fn lineage_root_reflects_training_history() {
        let ledger = LineageLedger::new();
        let untrained_root = ledger.lineage_root("student");
        ledger.record_training("student", &batch("alpha", 100)).unwrap();
        assert_ne!(ledger.lineage_root("student"), untrained_root);
    }
}
