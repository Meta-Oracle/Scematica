use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Default path for the shared metrics file
pub const METRICS_FILE: &str = "scematica-metrics.json";

/// Global bot metrics, updated atomically during operation
#[derive(Debug, Default)]
pub struct BotMetrics {
    pub trades_attempted: AtomicU64,
    pub trades_confirmed: AtomicU64,
    pub trades_failed: AtomicU64,
    pub arb_opportunities_found: AtomicU64,
    pub arb_executed: AtomicU64,
    pub total_pnl_lamports: AtomicI64,
    pub pools_tracked: AtomicU64,
    pub start_time: RwLock<Option<DateTime<Utc>>>,
}

impl BotMetrics {
    pub fn new() -> Arc<Self> {
        let m = Arc::new(Self::default());
        *m.start_time.write() = Some(Utc::now());
        m
    }

    pub fn record_trade_attempt(&self) {
        self.trades_attempted.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_trade_confirmed(&self, pnl_lamports: i64) {
        self.trades_confirmed.fetch_add(1, Ordering::Relaxed);
        self.total_pnl_lamports.fetch_add(pnl_lamports, Ordering::Relaxed);
    }

    pub fn record_trade_failed(&self) {
        self.trades_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_arb_found(&self) {
        self.arb_opportunities_found.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_arb_executed(&self) {
        self.arb_executed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_pools_tracked(&self, count: u64) {
        self.pools_tracked.store(count, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        let uptime_secs = self.start_time.read()
            .map(|t| (Utc::now() - t).num_seconds() as u64)
            .unwrap_or(0);

        MetricsSnapshot {
            trades_attempted: self.trades_attempted.load(Ordering::Relaxed),
            trades_confirmed: self.trades_confirmed.load(Ordering::Relaxed),
            trades_failed: self.trades_failed.load(Ordering::Relaxed),
            arb_opportunities_found: self.arb_opportunities_found.load(Ordering::Relaxed),
            arb_executed: self.arb_executed.load(Ordering::Relaxed),
            total_pnl_lamports: self.total_pnl_lamports.load(Ordering::Relaxed),
            pools_tracked: self.pools_tracked.load(Ordering::Relaxed),
            uptime_secs,
        }
    }

    /// Write the current snapshot to a JSON file for the dashboard to read.
    /// Writes atomically via a temp file to avoid partial reads.
    pub fn flush_to_file(&self, path: &str) {
        let snap = self.snapshot();
        let tmp = format!("{}.tmp", path);
        if let Ok(json) = serde_json::to_string(&snap) {
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub trades_attempted: u64,
    pub trades_confirmed: u64,
    pub trades_failed: u64,
    pub arb_opportunities_found: u64,
    pub arb_executed: u64,
    pub total_pnl_lamports: i64,
    pub pools_tracked: u64,
    pub uptime_secs: u64,
}

impl MetricsSnapshot {
    pub fn win_rate(&self) -> f64 {
        if self.trades_attempted == 0 {
            return 0.0;
        }
        self.trades_confirmed as f64 / self.trades_attempted as f64 * 100.0
    }

    pub fn total_pnl_sol(&self) -> f64 {
        self.total_pnl_lamports as f64 / 1_000_000_000.0
    }

    /// Load a snapshot from the metrics file written by a running bot process.
    /// Returns None if the file doesn't exist or can't be parsed.
    pub fn load_from_file(path: &str) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }
}
