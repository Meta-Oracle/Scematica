use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Default path for the shared metrics file
pub const METRICS_FILE: &str = "scematica-metrics.json";

/// Default path for the append-only trade event log
pub const TRADES_FILE: &str = "scematica-trades.jsonl";

/// Default path for the strategy agent snapshot file
pub const STRATEGY_FILE: &str = "scematica-strategy.json";

/// Snapshot of the current live strategy parameters — written by the sniper,
/// read by the dashboard on each tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategySnapshot {
    pub take_profit_pct: f64,
    pub stop_loss_pct: f64,
    pub amount_multiplier: f64,
    pub market_regime: String,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl StrategySnapshot {
    pub fn write_to_file(&self, path: &str) {
        let tmp = format!("{}.tmp", path);
        if let Ok(json) = serde_json::to_string(self) {
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    pub fn load_from_file(path: &str) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }
}

/// A single trade event written by the sniper or arb engine.
/// Serialised as one JSON object per line (JSONL) for cheap append + tail.
///
/// The NN observer (`scematica-sniper/src/main.rs`) consumes this stream and
/// trains the Deep Q* agent — keys here are the source of truth for that
/// contract. `pnl_pct` and `position_age_secs` are populated on SELL events so
/// the agent gets normalised reward signal; defaulted to 0 on BUY/ARB so older
/// JSONL files keep deserialising.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeEvent {
    /// ISO-8601 timestamp
    pub timestamp: DateTime<Utc>,
    /// "BUY" | "SELL" | "ARB"
    pub kind: String,
    /// Base token mint address
    pub mint: String,
    /// Human-readable token symbol if known, otherwise empty
    pub symbol: String,
    /// Amount of quote token spent / received (UI units, e.g. SOL)
    pub amount: f64,
    /// Realised PnL in SOL (0.0 for buys, positive/negative for sells/arbs)
    pub pnl: f64,
    /// "✓" confirmed | "✗" failed
    pub status: String,
    /// Transaction signature (empty string if unavailable)
    pub signature: String,
    /// DEX name(s) involved, e.g. "Raydium" or "Raydium→Orca"
    pub dex: String,
    /// Number of hops (1 for sniper trades, 2+ for arb)
    pub hops: u8,
    /// Realised PnL as a percentage of entry size (SELL only; 0 otherwise).
    /// Used as the primary reward signal for the NN agent.
    #[serde(default)]
    pub pnl_pct: f64,
    /// How long the position was held in seconds (SELL only; 0 otherwise).
    #[serde(default)]
    pub position_age_secs: f64,
    /// Why the position was closed (SELL only). One of:
    /// "take_profit" | "stop_loss" | "trailing_stop" | "velocity_decay" |
    /// "peak_stagnation" | "dump_detected" | "no_pump_timeout" | "sell_mode" |
    /// "dump_mode" | "profit_lock" | "tiered_tp" | "fibonacci" | "timeout" | ""
    #[serde(default)]
    pub exit_reason: String,
}

impl TradeEvent {
    /// Append this event as a single JSON line to the trades file.
    /// Creates the file if it doesn't exist. Never truncates.
    pub fn append_to_file(&self, path: &str) {
        use std::io::Write;
        if let Ok(mut json) = serde_json::to_string(self) {
            json.push('\n');
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = file.write_all(json.as_bytes());
            }
        }
    }

    /// Read all trade events written after `byte_offset` in the file.
    /// Returns the new events and the updated byte offset for the next call.
    pub fn read_new_events(path: &str, byte_offset: u64) -> (Vec<TradeEvent>, u64) {
        use std::io::{BufRead, BufReader, Seek, SeekFrom};
        let mut file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return (vec![], byte_offset),
        };
        if file.seek(SeekFrom::Start(byte_offset)).is_err() {
            return (vec![], byte_offset);
        }
        let mut new_offset = byte_offset;
        let mut events = vec![];
        let reader = BufReader::new(&mut file);
        for line in reader.lines().map_while(Result::ok) {
            new_offset += line.len() as u64 + 1; // +1 for '\n'
            if let Ok(event) = serde_json::from_str::<TradeEvent>(&line) {
                events.push(event);
            }
        }
        (events, new_offset)
    }
}

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
