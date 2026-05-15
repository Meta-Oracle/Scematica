use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use scematica_core::metrics::{BotMetrics, MetricsSnapshot, StrategySnapshot, TradeEvent, METRICS_FILE, STRATEGY_FILE, TRADES_FILE};
use scematica_core::rpc::RpcConnection;
use std::collections::VecDeque;
use std::sync::Arc;
use crate::onboarding::OnboardingManager;
use crate::chat::{ChatLine, ChatUpdate};
use crate::process::BotCommand;
use scematica_ai::tool_dispatcher::LiveData;
use scematica_ai::chat_types::PendingToolCall;

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
    /// SCEMATICA token balance (AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump)
    pub scematica_balance: RwLock<f64>,
    pub active_mode: RwLock<BotMode>,
    pub is_ai_loading: RwLock<bool>,
    pub onboarding: RwLock<OnboardingManager>,
    pub should_quit: RwLock<bool>,
    pub selected_tab: RwLock<usize>,
    /// Latest snapshot read from the metrics file (written by sniper/arb processes)
    pub live_snapshot: RwLock<Option<MetricsSnapshot>>,
    /// Byte offset into the trade event log — tracks how far we've read
    pub trade_file_offset: RwLock<u64>,
    /// Byte offset into scematica-sniper.log — for external-process log tailing
    pub sniper_log_offset: RwLock<u64>,
    /// Latest strategy agent params — displayed in the Config tab
    pub strategy_tp_pct: RwLock<f64>,
    pub strategy_sl_pct: RwLock<f64>,
    pub strategy_multiplier: RwLock<f64>,
    pub strategy_regime: RwLock<String>,
    /// Chat tab state
    pub chat_history: RwLock<VecDeque<ChatLine>>,
    pub chat_input: RwLock<String>,
    pub chat_pending: RwLock<Option<PendingToolCall>>,
    pub chat_tx: RwLock<Option<tokio::sync::mpsc::Sender<ChatUpdate>>>,
    /// Shared live data for the AI tool dispatcher
    pub live_data: Arc<RwLock<LiveData>>,
    /// Channel to the process manager task — send BotCommand::Start/Stop
    pub bot_cmd_tx: RwLock<Option<tokio::sync::mpsc::Sender<BotCommand>>>,
    /// Emergency sell mode: when true the sniper skips buys and force-sells everything
    pub sell_mode_active: RwLock<bool>,
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
            scematica_balance: RwLock::new(0.0),
            active_mode: RwLock::new(BotMode::default()),
            is_ai_loading: RwLock::new(false),
            onboarding: RwLock::new(OnboardingManager::new()),
            should_quit: RwLock::new(false),
            selected_tab: RwLock::new(0),
            live_snapshot: RwLock::new(None),
            trade_file_offset: RwLock::new(0),
            sniper_log_offset: RwLock::new(0),
            strategy_tp_pct: RwLock::new(50.0),
            strategy_sl_pct: RwLock::new(20.0),
            strategy_multiplier: RwLock::new(1.0),
            strategy_regime: RwLock::new("neutral".into()),
            chat_history: RwLock::new(VecDeque::new()),
            chat_input: RwLock::new(String::new()),
            chat_pending: RwLock::new(None),
            chat_tx: RwLock::new(None),
            live_data: Arc::new(RwLock::new(LiveData::default())),
            bot_cmd_tx: RwLock::new(None),
            sell_mode_active: RwLock::new(false),
        })
    }

    /// Poll the metrics file and update live_snapshot. Called on each Tick.
    pub fn poll_metrics_file(&self) {
        if let Some(snap) = MetricsSnapshot::load_from_file(METRICS_FILE) {
            *self.live_snapshot.write() = Some(snap);
        }
    }

    /// Tail the trade event log file and push any new events into the trades deque.
    pub fn poll_trade_file(&self) {
        let offset = *self.trade_file_offset.read();
        let (events, new_offset) = TradeEvent::read_new_events(TRADES_FILE, offset);
        if events.is_empty() {
            return;
        }
        *self.trade_file_offset.write() = new_offset;
        for event in events {
            let log_line = format!(
                "[TRADE] {} {} | amount: {:.4} | pnl: {:.4} | {}",
                event.kind,
                &event.mint[..8.min(event.mint.len())],
                event.amount,
                event.pnl,
                event.status,
            );
            let entry = TradeEntry {
                timestamp: event.timestamp,
                kind: event.kind,
                mint: event.mint,
                amount: event.amount,
                pnl: event.pnl,
                status: event.status,
                signature: event.signature,
            };
            self.push_trade(entry);
            self.push_log(log_line);
        }
    }

    /// Poll the strategy snapshot file and update live strategy params.
    pub fn poll_strategy_file(&self) {
        if let Some(snap) = StrategySnapshot::load_from_file(STRATEGY_FILE) {
            *self.strategy_tp_pct.write() = snap.take_profit_pct;
            *self.strategy_sl_pct.write() = snap.stop_loss_pct;
            *self.strategy_multiplier.write() = snap.amount_multiplier;
            *self.strategy_regime.write() = snap.market_regime;
        }
    }
    /// Tail scematica-sniper.log and push new lines to the log panel.
    /// Used when the sniper runs as a separate process (not dashboard-managed).
    pub fn poll_log_file(&self) {
        use std::io::{BufRead, BufReader, Seek, SeekFrom};
        const LOG_FILE: &str = "scematica-sniper.log";
        let mut offset = *self.sniper_log_offset.read();
        let Ok(mut file) = std::fs::File::open(LOG_FILE) else { return };
        if file.seek(SeekFrom::Start(offset)).is_err() { return }
        let mut new_offset = offset;
        let reader = BufReader::new(&mut file);
        for line in reader.lines().map_while(Result::ok) {
            new_offset += line.len() as u64 + 1;
            self.push_log(format!("[SNIPER] {}", line));
        }
        if new_offset != offset {
            *self.sniper_log_offset.write() = new_offset;
        }
    }

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
        *tab = (*tab + 1) % 5;
    }

    pub fn prev_tab(&self) {
        let mut tab = self.selected_tab.write();
        *tab = tab.checked_sub(1).unwrap_or(4);
    }

    pub fn push_chat_line(&self, line: ChatLine) {
        let mut history = self.chat_history.write();
        history.push_back(line);
        while history.len() > 200 {
            history.pop_front();
        }
    }

    /// Sync live wallet/metrics data into the shared LiveData arc for the AI tool dispatcher.
    pub fn sync_live_data(&self) {
        let snap = self.effective_snapshot();
        let mut ld = self.live_data.write();
        ld.sol_balance = *self.sol_balance.read();
        ld.scema_balance = *self.scematica_balance.read();
        ld.wallet_address = self.wallet_address.read().clone();
        ld.trades_attempted = snap.trades_attempted;
        ld.trades_confirmed = snap.trades_confirmed;
        ld.arbs_found = snap.arb_opportunities_found;
        ld.arbs_executed = snap.arb_executed;
        ld.total_pnl_lamports = (snap.total_pnl_sol() * 1_000_000_000.0) as i64;
        ld.uptime_secs = snap.uptime_secs;
    }
}
