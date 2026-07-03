//! Seed a per-mint pool-score feed for the signal oracle from the bot's own
//! decision history.
//!
//! The lean [`crate::signal::FileSignalSource`] originally answered every
//! `pool_score` query with a neutral `50.0` because there was no per-pool score
//! file. The sniper, however, already records a real 0–100 pool score for every
//! pool it evaluates in `scematica-pool-decisions.jsonl` (and a live snapshot in
//! `scematica-pool-radar.json`). This module distills those into a compact
//! `scematica-pool-scores.json` map that the oracle serves directly — turning a
//! stubbed endpoint into real intelligence, and giving a freshly-deployed relay
//! day-one data instead of an empty book.
//!
//! Aggregation rule: for each mint, keep the **highest** score ever observed
//! (a pool the bot only saw via dedup has score `0` and is *not* emitted — an
//! unscored mint should read as "unknown/neutral", not "terrible"). `samples`
//! counts how many times the mint was seen, so consumers can weight confidence.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// The file the seeder writes and [`crate::signal::FileSignalSource`] reads.
pub const POOL_SCORES_FILE: &str = "scematica-pool-scores.json";

/// One mint's distilled pool-quality signal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PoolScoreRecord {
    /// Best 0–100 pool score the bot ever assigned this mint.
    pub score: f64,
    /// Best pump.fun trending score seen alongside it (0–100).
    #[serde(default)]
    pub pumpfun_score: f64,
    /// Pool liquidity (SOL) from the observation that produced `score`.
    #[serde(default)]
    pub size_sol: f64,
    /// Pool age (seconds) from that same observation.
    #[serde(default)]
    pub age_secs: f64,
    /// How many times this mint was observed across the source data.
    #[serde(default)]
    pub samples: u32,
}

/// The on-disk shape: `{ "records": { "<mint>": PoolScoreRecord, ... } }`.
/// Mirrors the deployer-reputation file so both feeds parse the same way.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolScoreFile {
    #[serde(default)]
    pub records: HashMap<String, PoolScoreRecord>,
}

impl PoolScoreFile {
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Fold one observation into the map, keeping the best-scoring sighting and
    /// bumping the sample count. Ignores empty mints and non-positive scores
    /// (those carry no signal — see the module-level aggregation rule).
    fn observe(&mut self, mint: &str, score: f64, pumpfun: f64, size_sol: f64, age_secs: f64) {
        if mint.is_empty() {
            return;
        }
        // Count every real sighting, even a zero-score one, so `samples` reflects
        // how often the bot encountered the mint.
        let entry = self
            .records
            .entry(mint.to_string())
            .or_insert(PoolScoreRecord {
                score: 0.0,
                pumpfun_score: 0.0,
                size_sol: 0.0,
                age_secs: 0.0,
                samples: 0,
            });
        entry.samples = entry.samples.saturating_add(1);
        if score > entry.score {
            entry.score = score;
            entry.pumpfun_score = pumpfun;
            entry.size_sol = size_sol;
            entry.age_secs = age_secs;
        }
    }

    /// Drop mints that were seen but never scored above zero — they read as
    /// "unknown" (the oracle returns a neutral baseline for absent mints).
    fn prune_unscored(&mut self) {
        self.records.retain(|_, r| r.score > 0.0);
    }
}

// ── Source line shapes (only the fields we need; `serde(default)` for safety) ──

#[derive(Deserialize)]
struct DecisionLine {
    #[serde(default)]
    mint: String,
    #[serde(default)]
    pool_score: f64,
    #[serde(default)]
    pumpfun_score: f64,
    #[serde(default)]
    pool_size_sol: f64,
    #[serde(default)]
    pool_age_secs: f64,
}

#[derive(Deserialize)]
struct RadarEntry {
    #[serde(default)]
    mint: String,
    #[serde(default)]
    score: f64,
    #[serde(default)]
    size_sol: f64,
    #[serde(default)]
    age_secs: f64,
}

/// Build a pool-score map from `scematica-pool-decisions.jsonl` contents and,
/// optionally, `scematica-pool-radar.json` contents. Malformed lines/entries are
/// skipped rather than failing the whole build, so a partially-written live log
/// still seeds usefully.
pub fn seed_pool_scores(decisions_jsonl: &str, radar_json: Option<&str>) -> PoolScoreFile {
    let mut file = PoolScoreFile::default();

    for line in decisions_jsonl.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(d) = serde_json::from_str::<DecisionLine>(line) {
            file.observe(
                &d.mint,
                d.pool_score,
                d.pumpfun_score,
                d.pool_size_sol,
                d.pool_age_secs,
            );
        }
    }

    if let Some(radar) = radar_json {
        if let Ok(entries) = serde_json::from_str::<Vec<RadarEntry>>(radar) {
            for e in entries {
                // Radar has no pump.fun score; fold liquidity/age with its score.
                file.observe(&e.mint, e.score, 0.0, e.size_sol, e.age_secs);
            }
        }
    }

    file.prune_unscored();
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_best_score_and_counts_samples() {
        // Same mint seen three times: dedup(0), a real 40, then a better 72.
        let jsonl = r#"
{"mint":"MintA","pool_score":0.0,"pool_size_sol":0.0,"pool_age_secs":0.0}
{"mint":"MintA","pool_score":40.0,"pumpfun_score":10.0,"pool_size_sol":12.0,"pool_age_secs":3.0}
{"mint":"MintA","pool_score":72.0,"pumpfun_score":85.0,"pool_size_sol":30.0,"pool_age_secs":1.0}
"#;
        let file = seed_pool_scores(jsonl, None);
        let a = file.records.get("MintA").expect("MintA present");
        assert_eq!(a.score, 72.0, "keeps the highest score");
        assert_eq!(
            a.pumpfun_score, 85.0,
            "carries the best sighting's metadata"
        );
        assert_eq!(a.size_sol, 30.0);
        assert_eq!(
            a.samples, 3,
            "counts every sighting incl. the zero-score one"
        );
    }

    #[test]
    fn drops_mints_never_scored_above_zero() {
        // Only ever dedup-rejected → no signal → not emitted.
        let jsonl = r#"
{"mint":"DedupOnly","pool_score":0.0}
{"mint":"DedupOnly","pool_score":0.0}
{"mint":"Real","pool_score":55.0}
"#;
        let file = seed_pool_scores(jsonl, None);
        assert!(
            !file.records.contains_key("DedupOnly"),
            "unscored mint pruned"
        );
        assert!(file.records.contains_key("Real"));
        assert_eq!(file.len(), 1);
    }

    #[test]
    fn merges_radar_and_skips_garbage_lines() {
        let jsonl = "not json\n{\"mint\":\"MintB\",\"pool_score\":20.0}\n\n";
        let radar = r#"[{"mint":"MintB","score":48.0,"size_sol":9.0,"age_secs":2.0},
                        {"mint":"MintC","score":33.0}]"#;
        let file = seed_pool_scores(jsonl, Some(radar));
        // Radar's 48 beats the decision log's 20 for MintB.
        assert_eq!(file.records.get("MintB").unwrap().score, 48.0);
        assert_eq!(file.records.get("MintB").unwrap().size_sol, 9.0);
        // MintC only in radar.
        assert_eq!(file.records.get("MintC").unwrap().score, 33.0);
    }

    #[test]
    fn empty_input_yields_empty_map() {
        assert!(seed_pool_scores("", None).is_empty());
        assert!(seed_pool_scores("\n\n  \n", None).is_empty());
    }

    #[test]
    fn ignores_empty_mint() {
        let jsonl = r#"{"mint":"","pool_score":90.0}"#;
        assert!(seed_pool_scores(jsonl, None).is_empty());
    }
}
