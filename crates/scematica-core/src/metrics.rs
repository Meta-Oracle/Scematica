use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

/// Environment override for all runtime JSON/JSONL artifacts.
pub const DATA_DIR_ENV: &str = "SCEMATICA_DATA_DIR";

/// Default path for the shared metrics file
pub const METRICS_FILE: &str = "scematica-metrics.json";

/// Default path for the append-only trade event log
pub const TRADES_FILE: &str = "scematica-trades.jsonl";

/// Append-only pool evaluation ledger. Records accepted and rejected pools with
/// the exact signal snapshot that produced the decision.
pub const POOL_DECISIONS_FILE: &str = "scematica-pool-decisions.jsonl";

/// Append-only transaction execution telemetry. Records latency, retry, fee,
/// and error-shape data for buy/sell/arb execution quality analysis.
pub const TX_TELEMETRY_FILE: &str = "scematica-tx-telemetry.jsonl";

/// Default path for the strategy agent snapshot file
pub const STRATEGY_FILE: &str = "scematica-strategy.json";

/// Default path for the pool radar snapshot file.
pub const POOL_RADAR_FILE: &str = "scematica-pool-radar.json";

/// Default path for filter rejection/pass counters.
pub const FILTER_STATS_FILE: &str = "scematica-filter-stats.json";

/// Default path for the Deep Q* agent stats snapshot.
pub const NN_STATS_FILE: &str = "scematica-nn-stats.json";

/// Default path for the latest Deep Q* advice/explanation snapshot.
pub const NN_ADVICE_FILE: &str = "scematica-nn-advice.json";

/// Default path for the persisted Deep Q* checkpoint.
pub const NN_AGENT_FILE: &str = "scematica-nn-agent.json";

/// Default path for the sniper log.
pub const LOG_FILE: &str = "scematica-sniper.log";

/// Default path for the sniper process lock file.
pub const LOCK_FILE: &str = "scematica-sniper.lock";

/// Dashboard/API control files.
pub const SELL_MODE_FILE: &str = "scematica-sell-mode.json";
pub const DUMP_MODE_FILE: &str = "scematica-dump-mode.json";
pub const RATE_MODE_FILE: &str = "scematica-rate-mode.json";
pub const MOON_CHASE_FILE: &str = "scematica-moon-chase.json";
pub const BUILDER_MODE_FILE: &str = "scematica-builder-mode.json";
pub const HIGH_SPEED_FILE: &str = "scematica-highspeed-mode.json";
pub const POSITIONS_FILE: &str = "scematica-positions.json";
pub const TOURNAMENT_FILE: &str = "scematica-nn-tournament.json";
pub const DEPLOYER_REPUTATION_FILE: &str = "scematica-deployer-reputation.json";

/// Resolve the directory used for runtime artifacts.
///
/// Priority:
/// 1. `SCEMATICA_DATA_DIR`
/// 2. the process CWD when it looks like the workspace root
/// 3. the compile-time workspace root discovered from `scematica-core`
/// 4. the process CWD as a final fallback
pub fn artifact_dir() -> PathBuf {
    if let Ok(value) = std::env::var(DATA_DIR_ENV) {
        let value = value.trim();
        if !value.is_empty() {
            return PathBuf::from(shellexpand::tilde(value).to_string());
        }
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("config.toml").exists() || cwd.join(".git").exists() {
        return cwd;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for ancestor in manifest_dir.ancestors() {
        if ancestor.join("Cargo.toml").exists() && ancestor.join("config.toml").exists() {
            return ancestor.to_path_buf();
        }
    }

    cwd
}

/// Resolve a runtime artifact path. Absolute paths pass through unchanged.
pub fn artifact_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        artifact_dir().join(path)
    }
}

/// Lossy string form for APIs that still take `&str` file paths.
pub fn artifact_path_string(path: impl AsRef<Path>) -> String {
    artifact_path(path).to_string_lossy().into_owned()
}

/// Create an empty artifact if it is missing. Existing files are not truncated.
pub fn ensure_artifact_file(path: impl AsRef<Path>) {
    let path = artifact_path(path);
    ensure_parent_dir(&path);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);
}

fn ensure_parent_dir(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

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
        let path = artifact_path(path);
        ensure_parent_dir(&path);
        let tmp = tmp_path(&path);
        if let Ok(json) = serde_json::to_string(self) {
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    pub fn load_from_file(path: &str) -> Option<Self> {
        let path = artifact_path(path);
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

    // ── Pool metadata recorded at BUY time (0.0 for SELL/ARB) ─────────────────
    // These fields close the feedback loop: after each sell we can look back at
    // the paired BUY to see which pool signals predicted the outcome. Without
    // these, the pool scorer is calibrated on theory alone.
    #[serde(default)]
    pub pool_size_sol: f64,
    #[serde(default)]
    pub pool_age_secs: f64,
    #[serde(default)]
    pub velocity_sol_per_sec: f64,
    #[serde(default)]
    pub buy_pressure_ratio: f64,
    #[serde(default)]
    pub pool_score: f64,
    #[serde(default)]
    pub pumpfun_score: f64,
    /// Live inflow rate measured during the filter evaluation window (~600ms).
    #[serde(default)]
    pub inflow_rate_sol_per_sec: f64,
}

impl TradeEvent {
    /// Append this event as a single JSON line to the trades file.
    /// Creates the file if it doesn't exist. Never truncates.
    pub fn append_to_file(&self, path: &str) {
        use std::io::Write;
        let path = artifact_path(path);
        ensure_parent_dir(&path);
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
        let path = artifact_path(path);
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolDecisionEvent {
    pub timestamp: DateTime<Utc>,
    pub mint: String,
    pub pool: String,
    pub quote_mint: String,
    /// "accepted" | "rejected" | "ignored"
    pub decision: String,
    /// Gate or subsystem that made the decision.
    pub stage: String,
    /// Human-readable reason. Kept compact for dashboard/API rendering.
    pub reason: String,
    pub pool_size_sol: f64,
    pub pool_age_secs: f64,
    pub velocity_sol_per_sec: f64,
    pub buy_pressure_ratio: f64,
    pub pool_score: f64,
    pub pumpfun_score: f64,
    pub inflow_rate_sol_per_sec: f64,
    pub high_speed: bool,
    pub dex_boosted: bool,
    pub dex_boost_usd: f64,
    pub social_count: u8,
    /// Score floor active at the decision point. 0.0 when not applicable.
    pub effective_min_score: f64,
    pub dq_action: String,
    pub dq_confidence: f64,
    pub utc_hour: u8,
}

impl PoolDecisionEvent {
    pub fn append_to_file(&self, path: &str) {
        use std::io::Write;
        let path = artifact_path(path);
        ensure_parent_dir(&path);
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxTelemetryEvent {
    pub timestamp: DateTime<Utc>,
    /// "default" | "jito" | future executor name.
    pub executor: String,
    /// Heuristic label: "buy" | "sell" | "unknown".
    pub tx_kind: String,
    pub signature: String,
    pub confirmed: bool,
    pub error: String,
    pub attempts: u32,
    pub instruction_count: usize,
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
    pub compute_unit_price_hard_cap: u64,
    pub loaded_accounts_data_size_limit: u32,
    pub skip_preflight: bool,
    pub high_speed: bool,
    pub elapsed_ms: u64,
    pub blockhash_fetch_ms_total: u64,
    pub send_confirm_ms_total: u64,
    pub retry_delay_ms_total: u64,
    pub timeout_count: u32,
    pub rate_limit_count: u32,
    pub slippage_error_count: u32,
    pub blockhash_error_count: u32,
}

impl TxTelemetryEvent {
    pub fn append_to_file(&self, path: &str) {
        use std::io::Write;
        let path = artifact_path(path);
        ensure_parent_dir(&path);
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

    /// Record a confirmed *entry* (buy). Counts toward the attempt→confirm funnel.
    /// PnL realised on the matching exit must be recorded separately via
    /// [`record_pnl`](Self::record_pnl) so the confirm counter only ever counts
    /// entries (otherwise `trades_confirmed` could exceed `trades_attempted`).
    pub fn record_trade_confirmed(&self, pnl_lamports: i64) {
        self.trades_confirmed.fetch_add(1, Ordering::Relaxed);
        self.total_pnl_lamports
            .fetch_add(pnl_lamports, Ordering::Relaxed);
    }

    /// Accumulate realised PnL without touching any trade counter. Used by exit
    /// (sell) completion and arb settlement, which are not entry attempts.
    pub fn record_pnl(&self, pnl_lamports: i64) {
        self.total_pnl_lamports
            .fetch_add(pnl_lamports, Ordering::Relaxed);
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
        let uptime_secs = self
            .start_time
            .read()
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
        let path = artifact_path(path);
        ensure_parent_dir(&path);
        let tmp = tmp_path(&path);
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
        let path = artifact_path(path);
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }
}
