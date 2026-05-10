use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use scematica_core::metrics::MetricsSnapshot;
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
#[derive(Debug, Default)]
pub struct AppState {
    pub metrics: RwLock<Option<MetricsSnapshot>>,
    pub log_lines: RwLock<VecDeque<String>>,
    pub trades: RwLock<VecDeque<TradeEntry>>,
    pub wallet_address: RwLock<String>,
    pub sol_balance: RwLock<f64>,
    pub quote_balance: RwLock<f64>,
    pub active_mode: RwLock<BotMode>,
    pub should_quit: RwLock<bool>,
    pub selected_tab: RwLock<usize>,
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
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
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

    pub fn update_metrics(&self, snap: MetricsSnapshot) {
        *self.metrics.write() = Some(snap);
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
