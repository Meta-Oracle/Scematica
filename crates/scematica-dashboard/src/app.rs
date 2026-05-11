use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use scematica_core::metrics::{BotMetrics, MetricsSnapshot, METRICS_FILE};
use scematica_core::rpc::RpcConnection;
use std::collections::VecDeque;
use std::sync::Arc;

/// Maximum number of log lines to keep in memory
const MAX_LOG_LINES: usize = 200;
/// Maximum number of trade history entries
const MAX_TRADES: usize = 100;

#[derive(Debug, Clone)]
pub struct TradeEntry {
    pub timestamp: DateTime<Utc>,
    pub kind: String,       // "BUY" | "SELL" | "ARB"
    pub mint: String,
    pub amount: f64,
    pub pnl: f64,
    pub status: String,     // "✓" | "✗"
    pub signature: String,
}

/// Shared application state for the dashboard
#[derive(Debug)]
pub struct AppState {
    pub metrics: Arc<BotMetrics>,
    pub rpc: Arc<RpcConnection>,
    pub log_lines: RwLock<VecDeque<String>>,
    pub trades: RwLock<VecDeque<TradeEntry>>,
    pub wallet_address: RwLock<String>,
    pub sol_balance: RwLock<f64>,
    pub quote_balance: RwLock<f64>,
    pub active_mode: RwLock<BotMode>,
    pub should_quit: RwLock<bool>,
    pub selected_tab: RwLock<usize>,
    /// Latest snapshot read from the metrics file (written by sniper/arb processes)
    pub live_snapshot: RwLock<Option<MetricsSnapshot>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BotMode {
    #[default]
    Idle,
    Sniper,
    Arb,
    Both,
}

impl std::fmt::Display for BotMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BotMode::Idle => write!(f, "IDLE"),
            BotMode::Sniper => write!(f, "SNIPER"),
            BotMode::Arb => write!(f, "ARB"),
            BotMode::Both => write!(f, "SNIPER+ARB"),
        }
    }
}

impl AppState {
    pub fn new(metrics: Arc<BotMetrics>, rpc: Arc<RpcConnection>) -> Arc<Self> {
        Arc::new(Self {
            metrics,
            rpc,
            log_lines: RwLock::new(VecDeque::new()),
            trades: RwLock::new(VecDeque::new()),
            wallet_address: RwLock::new(String::new()),
            sol_balance: RwLock::new(0.0),
            quote_balance: RwLock::new(0.0),
            active_mode: RwLock::new(BotMode::default()),
            should_quit: RwLock::new(false),
            selected_tab: RwLock::new(0),
            live_snapshot: RwLock::new(None),
        })
    }

    /// Poll the metrics file and update live_snapshot. Called on each Tick.
    pub fn poll_metrics_file(&self) {
        if let Some(snap) = MetricsSnapshot::load_from_file(METRICS_FILE) {
            *self.live_snapshot.write() = Some(snap);
        }
    }

    /// Returns the live snapshot if available, otherwise falls back to the
    /// in-process BotMetrics (useful when running dashboard alongside a bot).
    pub fn effective_snapshot(&self) -> MetricsSnapshot {
        self.live_snapshot
            .read()
            .clone()
            .unwrap_or_else(|| self.metrics.snapshot())
    }

    pub fn push_log(&self, line: impl Into<String>) {
        let mut logs = self.log_lines.write();
        logs.push_back(line.into());
        while logs.len() > MAX_LOG_LINES {
            logs.pop_front();
        }
    }

    pub fn push_trade(&self, trade: TradeEntry) {
        let mut trades = self.trades.write();
        trades.push_front(trade);
        while trades.len() > MAX_TRADES {
            trades.pop_back();
        }
    }

    pub fn quit(&self) {
        *self.should_quit.write() = true;
    }

    pub fn is_quitting(&self) -> bool {
        *self.should_quit.read()
    }

    pub fn next_tab(&self) {
        let mut tab = self.selected_tab.write();
        *tab = (*tab + 1) % 4;
    }

    pub fn prev_tab(&self) {
        let mut tab = self.selected_tab.write();
        *tab = tab.checked_sub(1).unwrap_or(3);
    }
}
