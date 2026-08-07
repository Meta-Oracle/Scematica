use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

const LEDGER_FILE: &str = "scematica-deployer-reputation.json";

/// Per-deployer outcome record.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeployerRecord {
    pub rug_count: u32,
    pub success_count: u32,
    pub total_trades: u32,
    /// Exponentially weighted rug rate (faster adaptation to recent behaviour)
    pub ema_rug_rate: f64,
}

impl DeployerRecord {
    /// Returns a score in [0.0, 1.0]: 1.0 = completely trustworthy, 0.0 = confirmed rugger.
    pub fn score(&self) -> f64 {
        if self.total_trades == 0 {
            return 0.5; // unknown deployer — neutral
        }
        // Blend raw rug rate with EMA for responsiveness
        let raw_rate = self.rug_count as f64 / self.total_trades as f64;
        let blended = 0.4 * raw_rate + 0.6 * self.ema_rug_rate;
        1.0 - blended
    }

    pub fn record_rug(&mut self) {
        self.rug_count += 1;
        self.total_trades += 1;
        self.ema_rug_rate = 0.3 * 1.0 + 0.7 * self.ema_rug_rate;
    }

    pub fn record_success(&mut self) {
        self.success_count += 1;
        self.total_trades += 1;
        self.ema_rug_rate = 0.3 * 0.0 + 0.7 * self.ema_rug_rate;
    }
}

/// Persistent ledger mapping deployer pubkey → outcome history.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DeployerLedger {
    records: HashMap<String, DeployerRecord>,
}

impl DeployerLedger {
    pub fn load() -> Self {
        let Ok(data) = std::fs::read_to_string(LEDGER_FILE) else {
            return Self::default();
        };
        serde_json::from_str(&data).unwrap_or_default()
    }

    pub fn save(&self) {
        let tmp = format!("{}.tmp", LEDGER_FILE);
        if let Ok(s) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp, &s).is_ok() {
                let _ = std::fs::rename(&tmp, LEDGER_FILE);
            }
        }
    }

    pub fn score(&self, deployer: &str) -> f64 {
        self.records.get(deployer).map(|r| r.score()).unwrap_or(0.5)
    }

    pub fn record_rug(&mut self, deployer: &str) {
        let rec = self.records.entry(deployer.to_string()).or_default();
        rec.record_rug();
        info!("📒 Deployer ledger: {} → rug (score now {:.2})", &deployer[..8.min(deployer.len())], rec.score());
        self.save();
    }

    pub fn record_success(&mut self, deployer: &str) {
        let rec = self.records.entry(deployer.to_string()).or_default();
        rec.record_success();
        debug!("📒 Deployer ledger: {} → success (score now {:.2})", &deployer[..8.min(deployer.len())], rec.score());
        self.save();
    }

    pub fn known_count(&self) -> usize { self.records.len() }

    /// Return the number of rugs attributed to this deployer.
    ///
    /// The ledger does not store per-event timestamps, so this returns the
    /// *lifetime* rug count.  The caller (CrossPoolCorrelationFilter) uses it
    /// as a conservative "24-hour" proxy — if a deployer has rugged many times
    /// historically, they are likely to continue doing so.
    pub fn recent_rug_count(&self, deployer: &str) -> u32 {
        self.records
            .get(deployer)
            .map(|r| r.rug_count)
            .unwrap_or(0)
    }
}

