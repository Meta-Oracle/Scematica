//! Flush-loop state: what has been anchored, and what comes next.
//!
//! The daemon exists to turn every batch from `Retrospective` into `Live`. That is not a
//! cosmetic upgrade — a commitment published after an outcome is knowable proves only that
//! the log was not edited afterwards, while one published before proves you called it in
//! advance. The second is the claim worth making, and it is purely a function of how fast
//! this loop runs.
//!
//! # Two failure modes this is built around
//!
//! **Double-anchoring.** Re-submitting a root the contract already holds reverts with
//! `AnchorExists`, which is merely wasted gas — but re-anchoring the *same decisions* under
//! a new root would inflate the record with duplicates and make any count meaningless.
//!
//! **Silent skipping.** Worse than duplication. A gap in the record is indistinguishable
//! from a deleted losing streak, which is the exact accusation this system exists to
//! refute. So progress is tracked by line count *and* validated by re-hashing the last
//! processed line: if the log was rotated or truncated underneath us, that check fails and
//! the daemon resyncs loudly instead of quietly resuming at the wrong offset.

use serde::{Deserialize, Serialize};

use crate::{decision_digest, DecisionRecord};
use mesh_core::commit::Digest;

/// Persisted progress. Small enough to write atomically on every flush.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Checkpoint {
    /// Parseable records consumed so far.
    pub processed: usize,
    /// Digest of the last consumed record, as a tripwire for rotation or truncation.
    #[serde(default)]
    pub last_digest: String,
    /// Roots anchored so far, newest last. Kept for operator inspection, not for logic.
    #[serde(default)]
    pub roots: Vec<String>,
}

/// Why a resync happened, so it can be reported rather than hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resync {
    /// Progress matched the log. Normal.
    None,
    /// The log is shorter than our progress — rotated or truncated.
    LogShrank,
    /// The record at our offset no longer hashes as expected — the log was rewritten.
    DigestMismatch,
}

/// What the next flush should cover.
#[derive(Debug, Clone)]
pub struct NextBatch<'a> {
    pub records: &'a [(DecisionRecord, u64)],
    pub start_index: usize,
    pub resync: Resync,
}

/// Select the records to anchor next, validating stored progress against the live log.
///
/// Returning the *resync reason* rather than silently correcting is deliberate: a rotated
/// log means some decisions may never have been anchored, and an operator needs to know
/// that a gap exists rather than discover a hole in the record later.
pub fn next_batch<'a>(all: &'a [(DecisionRecord, u64)], checkpoint: &Checkpoint) -> NextBatch<'a> {
    let mut start = checkpoint.processed;
    let mut resync = Resync::None;

    if start > all.len() {
        resync = Resync::LogShrank;
        start = 0;
    } else if start > 0 && !checkpoint.last_digest.is_empty() {
        let (record, unix) = &all[start - 1];
        if decision_digest(record, *unix).to_hex() != checkpoint.last_digest {
            resync = Resync::DigestMismatch;
            start = 0;
        }
    }

    NextBatch { records: &all[start..], start_index: start, resync }
}

/// Advance progress after a batch has actually been anchored.
///
/// Takes the anchored root so the checkpoint records what was committed. Call this only
/// after the transaction lands — advancing on a *planned* batch would skip records if the
/// send failed, producing exactly the gap this module is built to prevent.
pub fn advance(
    checkpoint: &mut Checkpoint,
    all: &[(DecisionRecord, u64)],
    consumed_upto: usize,
    root: &Digest,
) {
    checkpoint.processed = consumed_upto;
    checkpoint.last_digest = if consumed_upto == 0 {
        String::new()
    } else {
        let (record, unix) = &all[consumed_upto - 1];
        decision_digest(record, *unix).to_hex()
    };
    checkpoint.roots.push(root.to_hex());
}

/// Load a checkpoint, treating any problem as "start from the beginning".
///
/// A corrupt checkpoint must not stop attestation; re-anchoring already-covered decisions
/// costs gas and is caught by `AnchorExists`, whereas refusing to run leaves a growing gap.
pub fn load_checkpoint(path: &str) -> Checkpoint {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist via a temp file and rename, matching the sniper's own convention — a reader must
/// never observe a half-written checkpoint.
pub fn save_checkpoint(path: &str, checkpoint: &Checkpoint) -> std::io::Result<()> {
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(checkpoint)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(mint: &str) -> DecisionRecord {
        DecisionRecord {
            timestamp: "2026-08-10T00:00:00Z".into(),
            mint: mint.into(),
            decision: "accepted".into(),
            stage: "dq_advice".into(),
            reason: "ok".into(),
            pool_score: 70.0,
            dq_action: "Hold".into(),
            dq_confidence: 0.5,
        }
    }

    fn log(n: usize) -> Vec<(DecisionRecord, u64)> {
        (0..n).map(|i| (rec(&format!("M{i}")), 1_000 + i as u64)).collect()
    }

    #[test]
    fn a_fresh_checkpoint_takes_everything() {
        let all = log(5);
        let b = next_batch(&all, &Checkpoint::default());
        assert_eq!(b.records.len(), 5);
        assert_eq!(b.resync, Resync::None);
    }

    #[test]
    fn progress_is_respected_and_nothing_repeats() {
        let all = log(5);
        let mut cp = Checkpoint::default();
        advance(&mut cp, &all, 3, &Digest([1; 32]));

        let b = next_batch(&all, &cp);
        assert_eq!(b.start_index, 3);
        assert_eq!(b.records.len(), 2, "only the unanchored tail");
        assert_eq!(b.resync, Resync::None);
    }

    #[test]
    fn nothing_new_yields_an_empty_batch() {
        let all = log(3);
        let mut cp = Checkpoint::default();
        advance(&mut cp, &all, 3, &Digest([1; 32]));
        assert!(next_batch(&all, &cp).records.is_empty());
    }

    #[test]
    fn a_truncated_log_triggers_a_reported_resync() {
        // A silent resume at the old offset would skip records, and a gap in the record is
        // indistinguishable from a deleted losing streak.
        let all = log(10);
        let mut cp = Checkpoint::default();
        advance(&mut cp, &all, 8, &Digest([1; 32]));

        let shorter = log(4);
        let b = next_batch(&shorter, &cp);
        assert_eq!(b.resync, Resync::LogShrank);
        assert_eq!(b.start_index, 0, "resync must re-cover the whole log");
    }

    #[test]
    fn a_rewritten_log_is_detected_by_the_digest_tripwire() {
        let all = log(5);
        let mut cp = Checkpoint::default();
        advance(&mut cp, &all, 3, &Digest([1; 32]));

        // Same length, different content at our offset.
        let mut rewritten = log(5);
        rewritten[2].0.dq_action = "SellAll".into();

        let b = next_batch(&rewritten, &cp);
        assert_eq!(b.resync, Resync::DigestMismatch);
        assert_eq!(b.start_index, 0);
    }

    #[test]
    fn advancing_records_the_tripwire_digest() {
        let all = log(4);
        let mut cp = Checkpoint::default();
        advance(&mut cp, &all, 4, &Digest([9; 32]));

        let (r, u) = &all[3];
        assert_eq!(cp.last_digest, decision_digest(r, *u).to_hex());
        assert_eq!(cp.roots.len(), 1);
    }

    #[test]
    fn advancing_to_zero_clears_the_tripwire() {
        let all = log(2);
        let mut cp = Checkpoint::default();
        advance(&mut cp, &all, 0, &Digest([1; 32]));
        assert!(cp.last_digest.is_empty(), "no last record means no tripwire");
        // And that must not then be mistaken for a mismatch.
        assert_eq!(next_batch(&all, &cp).resync, Resync::None);
    }

    #[test]
    fn a_corrupt_checkpoint_file_starts_over_rather_than_stopping() {
        // Re-anchoring is cheap and caught by AnchorExists; refusing to run leaves a gap.
        let dir = std::env::temp_dir().join("mesh-attest-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("corrupt.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(load_checkpoint(path.to_str().unwrap()), Checkpoint::default());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn checkpoints_round_trip_through_disk() {
        let dir = std::env::temp_dir().join("mesh-attest-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cp.json");
        let p = path.to_str().unwrap();

        let all = log(3);
        let mut cp = Checkpoint::default();
        advance(&mut cp, &all, 3, &Digest([4; 32]));
        save_checkpoint(p, &cp).unwrap();

        assert_eq!(load_checkpoint(p), cp);
        let _ = std::fs::remove_file(&path);
    }
}
