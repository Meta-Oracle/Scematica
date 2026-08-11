//! `mesh-attest` — turn a trading agent's decisions into a tamper-evident public record.
//!
//! Every trading bot's performance claim is unverifiable. The category runs on screenshots
//! and cherry-picked windows, which is why nobody trusts any of it — including the ones
//! that genuinely work. There is no way to prove you did not delete the losing months.
//!
//! This closes that. Each decision the Deep Q\* agent makes is committed *before its
//! outcome is known*, batched into a Merkle root, and anchored on BOT Chain with a bond
//! behind it. Afterwards you can prove what you decided and when; you cannot revise it.
//!
//! # Why it reads a log instead of calling the agent
//!
//! The sniper already writes every evaluation to `scematica-pool-decisions.jsonl`,
//! including the DQ\* action and confidence. Reading that file keeps this crate entirely
//! out of the Solana workspace — no `solana-sdk` in the dependency tree, no lockfile
//! conflict, and no possibility of breaking a bot that is currently making money.
//!
//! # The property that gives this value
//!
//! **Promptness is the whole guarantee.** A commitment published after the outcome is
//! known proves nothing — you could have waited to see which way it went. So lag is
//! measured per record and reported, and a batch whose decisions are older than the
//! freshness bound is marked [`Freshness::Retrospective`]. That is not a failure to hide;
//! it is a fact the record must carry, because a retrospective anchor is a weaker claim
//! and presenting it as a live one would be the exact dishonesty this exists to prevent.
//!
//! Unlike [`mesh_core`], this crate is allowed dependencies — it is not on the
//! deterministic inference path that others must reimplement.

pub mod anchor;
pub mod daemon;

pub use anchor::{plan_anchor, AnchorPlan, MESH_MAINNET};

use mesh_core::commit::Digest;
use mesh_core::fixed::Fx;
use mesh_runtime::batch::ClaimBatch;
use serde::{Deserialize, Serialize};
use tiny_keccak::{Hasher, Keccak};

/// One evaluation, as the sniper recorded it.
///
/// Field names mirror `scematica-pool-decisions.jsonl`. Unknown fields are ignored so the
/// bot can add columns without breaking attestation.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DecisionRecord {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub mint: String,
    #[serde(default)]
    pub decision: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub pool_score: f64,
    #[serde(default)]
    pub dq_action: String,
    #[serde(default)]
    pub dq_confidence: f64,
}

/// How trustworthy the timing of a batch is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Every decision was anchored before its outcome could be known.
    Live,
    /// At least one decision is older than the bound. A weaker claim, and labelled so.
    Retrospective,
}

/// A batch ready to anchor.
#[derive(Debug, Clone)]
pub struct Attestation {
    pub root: Digest,
    pub digests: Vec<Digest>,
    pub count: usize,
    /// Unix seconds of the oldest and newest decision covered.
    pub window: (u64, u64),
    pub freshness: Freshness,
    /// Seconds between the oldest decision and the moment of attestation.
    pub max_lag_secs: u64,
}

/// Default freshness bound.
///
/// The sniper's positions resolve in minutes, so a decision anchored within five minutes
/// is committed before its result is known. Beyond that the claim weakens.
pub const DEFAULT_MAX_LAG_SECS: u64 = 300;

fn tagged(parts: &[&[u8]]) -> Digest {
    let mut k = Keccak::v256();
    for p in parts {
        k.update(p);
    }
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    Digest(out)
}

/// Length-prefix a string before hashing.
///
/// Without this, `("ab", "c")` and `("a", "bc")` produce identical preimages — a
/// canonicalisation bug that would let two different decisions share a digest, and so let
/// one be substituted for the other after the fact.
fn field(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
    out
}

/// Canonical digest of one decision.
///
/// Floats are quantised through [`Fx`] rather than hashed as IEEE bits: `f64` has multiple
/// encodings for the same value (notably `-0.0` and NaN payloads), so hashing the raw bits
/// would make two identical decisions hash differently depending on how the number was
/// produced. Everything here must be reproducible by a verifier holding only the log.
pub fn decision_digest(r: &DecisionRecord, unix: u64) -> Digest {
    tagged(&[
        b"scema-bot-mesh/decision/v1",
        &unix.to_be_bytes(),
        &field(r.mint.as_bytes()),
        &field(r.decision.as_bytes()),
        &field(r.stage.as_bytes()),
        &field(r.reason.as_bytes()),
        &field(r.dq_action.as_bytes()),
        &Fx::from_f64(r.pool_score).to_bits().to_be_bytes(),
        &Fx::from_f64(r.dq_confidence).to_bits().to_be_bytes(),
    ])
}

/// Parse an RFC3339 timestamp to unix seconds.
///
/// Hand-rolled to avoid a date dependency, and strict: an unparseable timestamp yields
/// `None` and the record is skipped rather than defaulted to zero. A record silently
/// timestamped 1970 would read as maximally stale and quietly poison the freshness verdict.
pub fn parse_unix(ts: &str) -> Option<u64> {
    let bytes = ts.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let num = |a: usize, b: usize| -> Option<u64> { ts.get(a..b)?.parse::<u64>().ok() };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, s) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }

    // Days since epoch via the civil-from-days algorithm (Howard Hinnant's), which is
    // exact for the proleptic Gregorian calendar and avoids a leap-year special case.
    let y_adj = if mo <= 2 { y - 1 } else { y };
    let era = y_adj / 400;
    let yoe = y_adj - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = (era as i64) * 146_097 + doe as i64 - 719_468;
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86_400 + h * 3600 + mi * 60 + s)
}

/// Parse a JSONL decision log, skipping unusable lines.
///
/// Malformed lines are skipped rather than aborting: the log is appended to by a live
/// process, so a torn final line is normal and must not stop the day's attestation.
pub fn parse_log(contents: &str) -> Vec<(DecisionRecord, u64)> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let record: DecisionRecord = serde_json::from_str(line).ok()?;
            let unix = parse_unix(&record.timestamp)?;
            Some((record, unix))
        })
        .collect()
}

/// Build an attestation over decisions, as of `now`.
///
/// Returns `None` for an empty set — anchoring nothing costs gas and asserts nothing.
pub fn attest(records: &[(DecisionRecord, u64)], now: u64, max_lag_secs: u64) -> Option<Attestation> {
    if records.is_empty() {
        return None;
    }

    let mut batch = ClaimBatch::new();
    let mut digests = Vec::with_capacity(records.len());
    let mut oldest = u64::MAX;
    let mut newest = 0u64;

    for (record, unix) in records {
        let digest = decision_digest(record, *unix);
        batch.push(digest);
        digests.push(digest);
        oldest = oldest.min(*unix);
        newest = newest.max(*unix);
    }

    // Saturating: a clock skew that puts a decision in the future must not underflow into
    // an enormous lag and mislabel a live batch as retrospective.
    let max_lag = now.saturating_sub(oldest);

    Some(Attestation {
        root: batch.root()?,
        digests,
        count: records.len(),
        window: (oldest, newest),
        freshness: if max_lag <= max_lag_secs { Freshness::Live } else { Freshness::Retrospective },
        max_lag_secs: max_lag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(mint: &str, ts: &str, action: &str, conf: f64) -> DecisionRecord {
        DecisionRecord {
            timestamp: ts.into(),
            mint: mint.into(),
            decision: "accepted".into(),
            stage: "dq_advice".into(),
            reason: "ok".into(),
            pool_score: 71.5,
            dq_action: action.into(),
            dq_confidence: conf,
        }
    }

    const T0: &str = "2026-08-10T00:00:00Z";

    #[test]
    fn timestamps_parse_to_unix() {
        // 2026-08-10T00:00:00Z, cross-checked against the civil-from-days algorithm.
        let a = parse_unix(T0).unwrap();
        let b = parse_unix("2026-08-10T00:01:00Z").unwrap();
        assert_eq!(b - a, 60);
        assert_eq!(parse_unix("2026-08-11T00:00:00Z").unwrap() - a, 86_400);
    }

    #[test]
    fn a_bad_timestamp_is_skipped_not_defaulted() {
        // Defaulting to 0 would read as maximally stale and poison the freshness verdict.
        assert!(parse_unix("not a date").is_none());
        assert!(parse_unix("2026-13-01T00:00:00Z").is_none(), "month 13");
        assert!(parse_unix("").is_none());
    }

    #[test]
    fn changing_any_field_changes_the_digest() {
        let base = rec("MintA", T0, "BuyStandard", 0.8);
        let d0 = decision_digest(&base, 1);

        let mut other = base.clone();
        other.dq_action = "Hold".into();
        assert_ne!(decision_digest(&other, 1), d0, "action must be committed");

        let mut other = base.clone();
        other.dq_confidence = 0.81;
        assert_ne!(decision_digest(&other, 1), d0, "confidence must be committed");

        let mut other = base.clone();
        other.decision = "rejected".into();
        assert_ne!(decision_digest(&other, 1), d0);

        assert_ne!(decision_digest(&base, 2), d0, "time must be committed");
    }

    #[test]
    fn field_boundaries_are_unambiguous() {
        // Without length-prefixing, ("ab","c") and ("a","bc") share a preimage — and one
        // decision could be substituted for another after the fact.
        let mut a = rec("ab", T0, "Hold", 0.5);
        a.decision = "c".into();
        let mut b = rec("a", T0, "Hold", 0.5);
        b.decision = "bc".into();
        assert_ne!(decision_digest(&a, 1), decision_digest(&b, 1));
    }

    #[test]
    fn digests_are_stable_across_runs() {
        let r = rec("MintA", T0, "BuyAggressive", 0.42);
        let d = decision_digest(&r, 99);
        for _ in 0..32 {
            assert_eq!(decision_digest(&r, 99), d);
        }
    }

    #[test]
    fn a_prompt_batch_is_live_and_a_late_one_is_not() {
        let t = parse_unix(T0).unwrap();
        let records = vec![(rec("M", T0, "Hold", 0.1), t)];

        let live = attest(&records, t + 60, DEFAULT_MAX_LAG_SECS).unwrap();
        assert_eq!(live.freshness, Freshness::Live);

        // Anchoring after the outcome is knowable proves nothing, and the record says so.
        let late = attest(&records, t + 86_400, DEFAULT_MAX_LAG_SECS).unwrap();
        assert_eq!(late.freshness, Freshness::Retrospective);
        assert_eq!(late.max_lag_secs, 86_400);
    }

    #[test]
    fn clock_skew_cannot_mislabel_a_live_batch() {
        let t = parse_unix(T0).unwrap();
        let records = vec![(rec("M", T0, "Hold", 0.1), t)];
        // `now` before the decision: saturating subtraction, not an underflow to u64::MAX.
        let a = attest(&records, t - 10, DEFAULT_MAX_LAG_SECS).unwrap();
        assert_eq!(a.max_lag_secs, 0);
        assert_eq!(a.freshness, Freshness::Live);
    }

    #[test]
    fn every_decision_is_provable_against_the_root() {
        let t = parse_unix(T0).unwrap();
        let records: Vec<_> = (0..5).map(|i| (rec(&format!("Mint{i}"), T0, "Hold", 0.5), t + i)).collect();
        let a = attest(&records, t + 10, DEFAULT_MAX_LAG_SECS).unwrap();

        let mut batch = ClaimBatch::new();
        for d in &a.digests {
            batch.push(*d);
        }
        assert_eq!(batch.root().unwrap(), a.root);
        for (i, d) in a.digests.iter().enumerate() {
            let proof = batch.proof(i).unwrap();
            assert!(mesh_runtime::verify_proof(d, i as u32, &proof, &a.root));
        }
    }

    #[test]
    fn a_torn_log_line_does_not_abort_the_batch() {
        // The file is appended to by a live process; a partial final line is normal.
        let log = format!(
            "{}\n{}\n{{\"mint\":\"trunc",
            serde_json::to_string(&rec("A", T0, "Hold", 0.5)).unwrap(),
            serde_json::to_string(&rec("B", T0, "Hold", 0.5)).unwrap(),
        );
        assert_eq!(parse_log(&log).len(), 2);
    }

    #[test]
    fn unknown_columns_do_not_break_parsing() {
        // The bot must be able to add fields without breaking attestation.
        let line = r#"{"timestamp":"2026-08-10T00:00:00Z","mint":"X","decision":"accepted","brand_new_field":42}"#;
        assert_eq!(parse_log(line).len(), 1);
    }

    #[test]
    fn nothing_to_attest_yields_no_anchor() {
        assert!(attest(&[], 1, DEFAULT_MAX_LAG_SECS).is_none());
    }

    #[test]
    fn the_window_spans_oldest_to_newest() {
        let t = parse_unix(T0).unwrap();
        let records = vec![
            (rec("A", T0, "Hold", 0.5), t + 100),
            (rec("B", T0, "Hold", 0.5), t),
            (rec("C", T0, "Hold", 0.5), t + 50),
        ];
        let a = attest(&records, t + 200, DEFAULT_MAX_LAG_SECS).unwrap();
        assert_eq!(a.window, (t, t + 100));
    }
}
