//! A concrete [`SignalSource`] (Primitive C) backed by the bot's on-disk
//! artifacts — the deployer-reputation ledger and the Deep Q* advice snapshot.
//! Serve it over x402 (see `scemadex-relay`) to monetize the intelligence.

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::de::DeserializeOwned;

use scemadex_sdk::{Address, Advice, PoolScore, Reputation, Result, ScemaDexError, SignalSource};

use crate::pool_scores::{PoolScoreFile, POOL_SCORES_FILE};

/// Score returned for a mint the bot has never scored. Neutral midpoint of the
/// 0–100 scale — "unknown", not "bad".
const NEUTRAL_POOL_SCORE: f64 = 50.0;

#[derive(serde::Deserialize)]
struct RepFile {
    #[serde(default)]
    records: HashMap<String, RepRecord>,
}

#[derive(serde::Deserialize)]
struct RepRecord {
    #[serde(default)]
    total_trades: u32,
    #[serde(default)]
    ema_rug_rate: f64,
}

#[derive(serde::Deserialize)]
struct AdviceFile {
    #[serde(default)]
    action: String,
    #[serde(default)]
    top_reason: String,
    #[serde(default)]
    confidence: f64,
}

/// Reads the bot's JSON artifacts to answer signal queries. Cheap (`std::fs`
/// per call); wrap in a cache if you serve high volume.
pub struct FileSignalSource {
    dir: PathBuf,
    reputation_file: String,
    advice_file: String,
    pool_scores_file: String,
}

impl FileSignalSource {
    /// Point at the directory where the bot writes its artifacts (its CWD).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            reputation_file: "scematica-deployer-reputation.json".to_string(),
            advice_file: "scematica-nn-advice.json".to_string(),
            pool_scores_file: POOL_SCORES_FILE.to_string(),
        }
    }

    fn read<T: DeserializeOwned>(&self, name: &str) -> Result<T> {
        let path = self.dir.join(name);
        let s = std::fs::read_to_string(&path)
            .map_err(|e| ScemaDexError::Oracle(format!("read {}: {e}", path.display())))?;
        serde_json::from_str(&s).map_err(|e| ScemaDexError::Oracle(format!("parse {name}: {e}")))
    }

    /// Read the seeded pool-score map, or an empty map if the file is absent —
    /// so a relay with no seed data degrades to neutral scores rather than error.
    fn pool_scores(&self) -> PoolScoreFile {
        self.read(&self.pool_scores_file).unwrap_or_default()
    }
}

#[async_trait]
impl SignalSource for FileSignalSource {
    async fn reputation(&self, deployer: &Address) -> Result<Reputation> {
        let file: RepFile = self.read(&self.reputation_file)?;
        Ok(match file.records.get(deployer.as_str()) {
            Some(r) => Reputation {
                score: (1.0 - r.ema_rug_rate).clamp(0.0, 1.0),
                samples: r.total_trades,
            },
            // Unknown deployer → neutral prior, zero samples.
            None => Reputation {
                score: 0.5,
                samples: 0,
            },
        })
    }

    async fn pool_score(&self, pool: &Address) -> Result<PoolScore> {
        // Served from the seeded `scematica-pool-scores.json` (distilled from the
        // sniper's own decision log by `pool_scores::seed_pool_scores`). A mint
        // the bot never scored — or a relay with no seed file — reads neutral.
        let scores = self.pool_scores();
        let score = scores
            .records
            .get(pool.as_str())
            .map(|r| r.score)
            .unwrap_or(NEUTRAL_POOL_SCORE);
        Ok(PoolScore { score })
    }

    async fn advice(&self, _token: &Address) -> Result<Advice> {
        // The advice snapshot is the agent's latest global decision, not
        // per-token; callers should treat it as market-regime guidance.
        let a: AdviceFile = self.read(&self.advice_file)?;
        Ok(Advice {
            action: a.action,
            conviction: a.confidence,
            reason: a.top_reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reads_reputation_from_ledger() {
        let dir = std::env::temp_dir().join(format!("scemadex-sig-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("scematica-deployer-reputation.json"),
            r#"{"records":{"Dep111":{"rug_count":1,"success_count":3,"total_trades":4,"ema_rug_rate":0.25}}}"#,
        )
        .unwrap();

        let src = FileSignalSource::new(&dir);
        // Address::new requires base58/32 bytes; use a real mint string and a
        // record keyed to it for the lookup path.
        std::fs::write(
            dir.join("scematica-deployer-reputation.json"),
            r#"{"records":{"So11111111111111111111111111111111111111112":{"total_trades":4,"ema_rug_rate":0.25}}}"#,
        )
        .unwrap();
        let addr = Address::new("So11111111111111111111111111111111111111112").unwrap();
        let rep = src.reputation(&addr).await.unwrap();
        assert_eq!(rep.samples, 4);
        assert!((rep.score - 0.75).abs() < 1e-9);

        // Unknown deployer → neutral.
        let other = Address::new("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();
        let rep2 = src.reputation(&other).await.unwrap();
        assert_eq!(rep2.samples, 0);
        assert!((rep2.score - 0.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn serves_seeded_pool_score_and_neutral_for_unknown() {
        let dir = std::env::temp_dir().join(format!("scemadex-ps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let known = "So11111111111111111111111111111111111111112";
        std::fs::write(
            dir.join(POOL_SCORES_FILE),
            format!(r#"{{"records":{{"{known}":{{"score":83.5,"samples":4}}}}}}"#),
        )
        .unwrap();

        let src = FileSignalSource::new(&dir);

        // Seeded mint → its real score.
        let addr = Address::new(known).unwrap();
        assert!((src.pool_score(&addr).await.unwrap().score - 83.5).abs() < 1e-9);

        // Unknown mint → neutral baseline, not an error.
        let unknown = Address::new("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();
        assert!((src.pool_score(&unknown).await.unwrap().score - NEUTRAL_POOL_SCORE).abs() < 1e-9);
    }

    #[tokio::test]
    async fn pool_score_is_neutral_when_no_seed_file() {
        // A relay with no seed data must degrade to neutral, not fail.
        let dir = std::env::temp_dir().join(format!("scemadex-ps-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = FileSignalSource::new(&dir);
        let addr = Address::new("So11111111111111111111111111111111111111112").unwrap();
        assert!((src.pool_score(&addr).await.unwrap().score - NEUTRAL_POOL_SCORE).abs() < 1e-9);
    }
}
