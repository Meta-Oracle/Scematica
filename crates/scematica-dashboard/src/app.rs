use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use scematica_core::metrics::{BotMetrics, MetricsSnapshot, StrategySnapshot, TradeEvent, METRICS_FILE, STRATEGY_FILE, TRADES_FILE};
use scematica_core::rpc::RpcConnection;
use serde::{Deserialize, Serialize};
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
/// Number of PnL data points to keep for the sparkline
const SPARKLINE_CAPACITY: usize = 60;
/// Max age of radar pool entries (5 minutes)
const RADAR_MAX_AGE_SECS: i64 = 300;

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

/// Live position snapshot — written by the sniper's live-position registry
/// every ~250 ms. Mirrors `scematica_sniper::sniper::LivePositionSnapshot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivePosition {
    pub mint: String,
    pub entry_lamports: u64,
    pub current_value_lamports: u64,
    pub peak_value_lamports: u64,
    pub entry_unix_secs: i64,
    pub dynamic_tp_pct: f64,
    pub escalations: u32,
    pub last_check_unix_secs: i64,
    #[serde(default)]
    pub current_sl_lamports: u64,
    /// SL level as % from entry (negative = loss floor, 0 = breakeven)
    #[serde(default)]
    pub current_sl_pct: f64,
    /// Consecutive declining price-check ticks
    #[serde(default)]
    pub decline_streak: u32,
}

impl LivePosition {
    pub fn age_secs(&self) -> i64 {
        chrono::Utc::now().timestamp().saturating_sub(self.entry_unix_secs).max(0)
    }
    pub fn pnl_pct(&self) -> f64 {
        if self.entry_lamports == 0 { 0.0 } else {
            (self.current_value_lamports as f64 - self.entry_lamports as f64)
                / self.entry_lamports as f64 * 100.0
        }
    }
    pub fn peak_pnl_pct(&self) -> f64 {
        if self.entry_lamports == 0 { 0.0 } else {
            (self.peak_value_lamports as f64 - self.entry_lamports as f64)
                / self.entry_lamports as f64 * 100.0
        }
    }
    pub fn entry_sol(&self) -> f64  { self.entry_lamports as f64 / 1e9 }
    pub fn value_sol(&self) -> f64  { self.current_value_lamports as f64 / 1e9 }
    pub fn sl_sol(&self) -> f64     { self.current_sl_lamports as f64 / 1e9 }
    /// 0.0–1.0: how far current value sits between SL (0) and TP (1)
    pub fn progress_to_tp(&self) -> f64 {
        if self.entry_lamports == 0 { return 0.5; }
        let sl = self.current_sl_lamports as f64;
        let tp = self.entry_lamports as f64 * (1.0 + self.dynamic_tp_pct / 100.0);
        let cur = self.current_value_lamports as f64;
        if tp <= sl { return 0.5; }
        ((cur - sl) / (tp - sl)).clamp(0.0, 1.0)
    }
}

/// Pool radar entry — written by the sniper when a pool is evaluated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadarPool {
    pub mint: String,
    /// Seconds since pool opened
    pub age_secs: f64,
    /// Pool size in SOL at evaluation time
    pub size_sol: f64,
    /// true if the pool passed all filter checks and reached the buy call
    pub passed_filters: bool,
    /// Pool score 0..100 (from NN agent or heuristic)
    pub score: f64,
    /// Unix timestamp (seconds) when the sniper evaluated this pool
    pub timestamp: i64,
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
    /// Auto dump mode: when true the sniper immediately force-sells all positions (min_out=0)
    pub dump_mode_active: RwLock<bool>,
    /// Mirror of scematica-highspeed-mode.json — operator toggles via [h] in Logs tab.
    pub high_speed_active: RwLock<bool>,
    /// Current rate mode — controls buy size, TP%, and SL% presets
    pub rate_mode: RwLock<RateMode>,
    /// Current builder mode — controls wallet growth target + progressive scaling
    pub builder_mode: RwLock<BuilderMode>,
    /// Moon Chase: when ON, the momentum-hold escalator runs aggressive params
    /// (more escalations, higher factor, wider pullback). Mirrored to the sniper
    /// via `scematica-moon-chase.json`.
    pub moon_chase: RwLock<bool>,
    /// Normalized PnL samples for the sparkline (scaled to u64 for ratatui Sparkline)
    pub pnl_sparkline: RwLock<VecDeque<u64>>,
    /// Best confirmed trade PnL this session (SOL)
    pub best_trade_pnl: RwLock<f64>,
    /// Worst confirmed trade PnL this session (SOL)
    pub worst_trade_pnl: RwLock<f64>,
    /// Current win/loss streak (positive = wins, negative = losses)
    pub trade_streak: RwLock<i32>,
    /// Raw filter rejection stats read from scematica-filter-stats.json
    pub filter_stats: RwLock<Option<serde_json::Value>>,
    /// Latest NN agent stats from scematica-nn-stats.json
    pub nn_stats: RwLock<Option<serde_json::Value>>,
    /// Log tab filter: substring/keyword the user typed after pressing '/'
    pub log_filter: RwLock<String>,
    /// True while the user is typing in the log filter bar
    pub log_filter_active: RwLock<bool>,
    /// Per-token price samples for the mini price chart (mint → deque of price ratios)
    pub price_history: RwLock<std::collections::HashMap<String, VecDeque<f64>>>,
    /// Pool radar entries — populated by sniper writing to scematica-pool-radar.json
    pub radar_pools: RwLock<Vec<RadarPool>>,
    /// Live open positions — sniper updates `scematica-positions.json` every 1 s
    pub live_positions: RwLock<Vec<LivePosition>>,
    /// Set to true the first time `scematica-positions.json` is successfully parsed.
    /// Once true, `open_position_count()` trusts the file count unconditionally —
    /// even when it is 0 (all positions closed). Without this flag, an empty file
    /// causes the counter to fall back to `open_position_mints()` which reads from
    /// the trade-history deque and may report phantom open positions for buys whose
    /// matching sells were evicted from the deque or not yet polled.
    pub live_positions_file_seen: std::sync::atomic::AtomicBool,
    /// Rolling last 5 alert messages displayed in the overview panel
    pub alert_history: RwLock<VecDeque<(chrono::DateTime<chrono::Utc>, String, String)>>,
}

/// Wallet-growth ladder. Each mode applies a live compounding algorithm that
/// continuously recomputes position size, TP, and SL from the current wallet
/// balance vs. target every 5 s, without restarting.
///
///   Off          — config.toml values (no algorithm override)
///   Growth       — 0.2 SOL  Mild geometric: size 1.0–2.0×, base TP/SL
///   Builder      — 1.0 SOL  Geometric compounding: size 1.5–3.5×, TP scales with distance
///   SuperBuilder — 3.0 SOL  Parabolic compounding: size 2.0–8.0×, auto moon-chase early
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuilderMode {
    #[default]
    Off,
    Growth,
    Builder,
    SuperBuilder,
}

impl BuilderMode {
    pub fn target_sol(self) -> f64 {
        match self {
            BuilderMode::Off          => 0.0,
            BuilderMode::Growth       => 0.2,
            BuilderMode::Builder      => 1.0,
            BuilderMode::SuperBuilder => 3.0,
        }
    }
    pub fn progressive(self) -> bool {
        // progressive_scaling in buy() is disabled for Builder modes — sizing
        // is fully controlled by live_params.amount_multiplier from the watcher.
        false
    }
    pub fn label(self) -> &'static str {
        match self {
            BuilderMode::Off          => "Off",
            BuilderMode::Growth       => "Growth",
            BuilderMode::Builder      => "Builder",
            BuilderMode::SuperBuilder => "SuperBuilder",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            BuilderMode::Off          => "off",
            BuilderMode::Growth       => "growth",
            BuilderMode::Builder      => "builder",
            BuilderMode::SuperBuilder => "super_builder",
        }
    }
    /// Short algorithm description shown in the Control tab.
    pub fn algo_description(self) -> &'static str {
        match self {
            BuilderMode::Off          => "config.toml values (no algorithm override)",
            BuilderMode::Growth       => "size 1.0–2.0×  TP base  SL base",
            BuilderMode::Builder      => "size 1.5–3.5× (p^0.65)  TP 1.5×→1.0× base  SL widens early",
            BuilderMode::SuperBuilder => "size 2.0–8.0× (p^0.35)  TP 2.0×→1.0× base  moon-chase p<25%",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BotMode {
    #[default]
    Idle,
    Sniper,
    Arb,
    Both,
}

/// Rate mode controls buy size, take-profit, and stop-loss presets.
///
/// Modes ordered from least to most aggressive:
///   Bearish → Micro → Safe → Balanced → Aggressive → Degen → Bullish
///
/// `Micro` is sized for wallets with only $1–2 of SOL — 0.1× the base
/// quote_amount (≈0.001 SOL/trade) so a small bag can actually place a buy
/// after fees and ATA rent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RateMode {
    Bearish,
    Micro,
    Safe,
    #[default]
    Balanced,
    Aggressive,
    Degen,
    Bullish,
    /// **Moon** — the 8th tier, built for chasing parabolic outliers. Entry TP
    /// is 1200 % but the real prize is the escalator: with Moon Chase ON, a
    /// Moon trade can ride through 7+ escalations (1200 % → 2100 % → 3675 % →
    /// 6431 % → …) before the pullback exit fires. The 5 % of trades that
    /// reach this territory pay for the much higher rate of small losses.
    Moon,
}

impl RateMode {
    pub fn multiplier(self) -> f64 {
        match self {
            RateMode::Bearish    => 0.3,
            RateMode::Micro      => 0.1,
            RateMode::Safe       => 0.5,
            RateMode::Balanced   => 1.0,
            RateMode::Aggressive => 2.0,
            RateMode::Degen      => 4.0,
            RateMode::Bullish    => 6.0,
            RateMode::Moon       => 8.0,
        }
    }
    pub fn tp_pct(self) -> f64 {
        // v0.9.0: base TP targets bumped to take advantage of momentum-hold
        // escalation. The values below are entry triggers — once hit, the
        // sell monitor escalates them if velocity is still strong, so a
        // Balanced trade can ride 150 % → 225 % → 337 % → 506 % before
        // capping out (4 escalations × 1.5×).
        //
        // Moon (v0.9.1) sits at 1200 % — high enough that the escalator only
        // engages on legitimate parabolic moves, so we don't waste capital
        // chasing small bumps with the most expensive position size.
        match self {
            RateMode::Bearish    =>   45.0,
            RateMode::Micro      =>   60.0,
            RateMode::Safe       =>   75.0,
            RateMode::Balanced   =>  150.0,
            RateMode::Aggressive =>  300.0,
            RateMode::Degen      =>  450.0,
            RateMode::Bullish    =>  750.0,
            RateMode::Moon       => 1200.0,
        }
    }
    pub fn sl_pct(self) -> f64 {
        match self {
            RateMode::Bearish    =>   8.0,
            RateMode::Micro      =>  10.0,
            RateMode::Safe       =>  10.0,
            RateMode::Balanced   =>  15.0,
            RateMode::Aggressive =>  25.0,
            RateMode::Degen      =>  40.0,
            RateMode::Bullish    =>  50.0,
            RateMode::Moon       =>  60.0,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            RateMode::Bearish    => "Bearish",
            RateMode::Micro      => "Micro",
            RateMode::Safe       => "Safe",
            RateMode::Balanced   => "Balanced",
            RateMode::Aggressive => "Aggressive",
            RateMode::Degen      => "Degen",
            RateMode::Bullish    => "Bullish",
            RateMode::Moon       => "Moon",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            RateMode::Bearish    => "bearish",
            RateMode::Micro      => "micro",
            RateMode::Safe       => "safe",
            RateMode::Balanced   => "balanced",
            RateMode::Aggressive => "aggressive",
            RateMode::Degen      => "degen",
            RateMode::Bullish    => "bullish",
            RateMode::Moon       => "moon",
        }
    }
    /// SOL per trade at base quote_amount=0.01
    pub fn buy_sol(self) -> f64 {
        0.01 * self.multiplier()
    }
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
            dump_mode_active: RwLock::new(false),
            high_speed_active: RwLock::new(false),
            rate_mode: RwLock::new(RateMode::default()),
            builder_mode: RwLock::new(BuilderMode::default()),
            moon_chase: RwLock::new(false),
            pnl_sparkline: RwLock::new(VecDeque::with_capacity(SPARKLINE_CAPACITY)),
            best_trade_pnl: RwLock::new(0.0),
            worst_trade_pnl: RwLock::new(0.0),
            trade_streak: RwLock::new(0),
            filter_stats: RwLock::new(None),
            nn_stats: RwLock::new(None),
            log_filter: RwLock::new(String::new()),
            log_filter_active: RwLock::new(false),
            price_history: RwLock::new(std::collections::HashMap::new()),
            radar_pools: RwLock::new(Vec::new()),
            live_positions: RwLock::new(Vec::new()),
            live_positions_file_seen: std::sync::atomic::AtomicBool::new(false),
            alert_history: RwLock::new(VecDeque::with_capacity(5)),
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

        // File-shrink detection: if scematica-trades.jsonl was truncated or rotated
        // externally (e.g. user reset positions), the recorded offset is now past
        // EOF. Treat that as "reset" — clear the in-memory deque and re-read from 0.
        if let Ok(meta) = std::fs::metadata(TRADES_FILE) {
            if meta.len() < offset {
                self.trades.write().clear();
                *self.trade_file_offset.write() = 0;
                self.push_log("[RESET] Trade history file shrank → cleared in-memory positions");
            }
        }

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
            // Update session stats for confirmed SELL events
            if event.kind == "SELL" && event.status == "✓" {
                let pnl = event.pnl;
                // Sparkline: encode as scaled u64 (baseline 1000, +/- pnl*500)
                let spark_val = (1000.0 + pnl * 500.0).max(0.0) as u64;
                {
                    let mut sl = self.pnl_sparkline.write();
                    sl.push_back(spark_val);
                    while sl.len() > SPARKLINE_CAPACITY { sl.pop_front(); }
                }
                // Best / worst
                {
                    let mut best = self.best_trade_pnl.write();
                    if pnl > *best { *best = pnl; }
                }
                {
                    let mut worst = self.worst_trade_pnl.write();
                    if pnl < *worst { *worst = pnl; }
                }
                // Win/loss streak
                {
                    let mut streak = self.trade_streak.write();
                    if pnl >= 0.0 {
                        *streak = if *streak >= 0 { *streak + 1 } else { 1 };
                    } else {
                        *streak = if *streak <= 0 { *streak - 1 } else { -1 };
                    }
                }
            }
            // Alert history: record confirmed BUY/SELL events (last 5 visible in overview)
            if event.status == "✓" {
                let title = format!("{} {}", event.status, event.kind.clone());
                let body = if event.kind == "SELL" {
                    format!("{}… PnL: {:.4} SOL", &event.mint[..8.min(event.mint.len())], event.pnl)
                } else {
                    format!("{}… {:.4} SOL", &event.mint[..8.min(event.mint.len())], event.amount)
                };
                let mut ah = self.alert_history.write();
                if ah.len() >= 5 { ah.pop_back(); }
                ah.push_front((event.timestamp, title, body));
            }
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

        // First call: seek to file end so we don't replay the entire prior session's
        // log into the dashboard buffer on startup. Stored offset == 0 + a non-empty
        // file ⇒ first run ⇒ jump past existing content and only show new lines.
        if offset == 0 {
            if let Ok(meta) = std::fs::metadata(LOG_FILE) {
                let end = meta.len();
                if end > 0 {
                    offset = end;
                    *self.sniper_log_offset.write() = end;
                }
            }
        }

        let Ok(mut file) = std::fs::File::open(LOG_FILE) else { return };

        // If the file shrank (rotation, manual truncate), reset to the new length so
        // we don't perma-block on a stale offset past EOF.
        if let Ok(meta) = file.metadata() {
            if meta.len() < offset {
                offset = meta.len();
                *self.sniper_log_offset.write() = offset;
            }
        }

        if file.seek(SeekFrom::Start(offset)).is_err() { return }
        let mut new_offset = offset;
        let reader = BufReader::new(&mut file);
        for line in reader.lines().map_while(Result::ok) {
            // BufReader::lines() strips trailing \n AND \r\n; advance by len + 1
            // (we approximate, since we don't know which terminator was used). Any
            // 1-byte/line drift is bounded by the file-shrink reset above.
            new_offset += line.len() as u64 + 1;
            self.push_log(format!("[SNIPER] {}", line));
        }
        if new_offset != offset {
            *self.sniper_log_offset.write() = new_offset;
        }
    }

    /// Read filter rejection stats from disk and cache them for the UI.
    pub fn poll_filter_stats_file(&self) {
        const STATS_FILE: &str = "scematica-filter-stats.json";
        let Ok(data) = std::fs::read_to_string(STATS_FILE) else { return };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else { return };
        *self.filter_stats.write() = Some(v);
    }

    /// Read NN agent stats from scematica-nn-stats.json.
    pub fn poll_nn_stats_file(&self) {
        let Ok(data) = std::fs::read_to_string("scematica-nn-stats.json") else { return };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else { return };
        *self.nn_stats.write() = Some(v);
    }

    /// Read live open positions from scematica-positions.json (sniper writes every 1 s).
    pub fn poll_live_positions_file(&self) {
        const POS_FILE: &str = "scematica-positions.json";
        let Ok(data) = std::fs::read_to_string(POS_FILE) else { return };
        let Ok(positions) = serde_json::from_str::<Vec<LivePosition>>(&data) else { return };
        *self.live_positions.write() = positions;
        // Mark that the file has been seen at least once. After this point,
        // open_position_count() trusts the file count (even when 0) instead of
        // falling back to the trade-history deque — preventing phantom positions.
        self.live_positions_file_seen.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Read pool radar entries from scematica-pool-radar.json.
    /// Drops any entries older than 5 minutes.
    pub fn poll_radar_file(&self) {
        const RADAR_FILE: &str = "scematica-pool-radar.json";
        let Ok(data) = std::fs::read_to_string(RADAR_FILE) else { return };
        let Ok(mut pools) = serde_json::from_str::<Vec<RadarPool>>(&data) else { return };
        let now = chrono::Utc::now().timestamp();
        pools.retain(|p| now - p.timestamp <= RADAR_MAX_AGE_SECS);
        *self.radar_pools.write() = pools;
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
        // Track price history per token for the mini chart (use amount as proxy sample)
        if trade.amount > 0.0 {
            let mut ph = self.price_history.write();
            let entry = ph.entry(trade.mint.clone()).or_insert_with(VecDeque::new);
            entry.push_back(trade.amount);
            if entry.len() > 60 { entry.pop_front(); }
        }
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
        *tab = (*tab + 1) % 6;
    }

    pub fn prev_tab(&self) {
        let mut tab = self.selected_tab.write();
        *tab = tab.checked_sub(1).unwrap_or(5);
    }

    pub fn push_chat_line(&self, line: ChatLine) {
        let mut history = self.chat_history.write();
        history.push_back(line);
        while history.len() > 200 {
            history.pop_front();
        }
    }

    /// Count currently open positions.
    ///
    /// Once `scematica-positions.json` has been parsed at least once, the file
    /// is the authoritative source — even when it contains 0 entries (all positions
    /// closed). This prevents the fallback from returning phantom open positions
    /// for buys whose matching sell was evicted from the in-memory trade deque.
    ///
    /// Before the file is seen (sniper not yet started), derives from trade history
    /// so the counter is non-zero during a session where the sniper ran previously.
    pub fn open_position_count(&self) -> usize {
        if self.live_positions_file_seen.load(std::sync::atomic::Ordering::Relaxed) {
            self.live_positions.read().len()
        } else {
            // Sniper hasn't written the positions file yet — derive from trade history
            let live = self.live_positions.read().len();
            if live > 0 { live } else { self.open_position_mints().len() }
        }
    }

    /// Derive currently open positions from trade history.
    /// Returns mints that have more confirmed BUYs than confirmed SELLs in the deque.
    pub fn open_position_mints(&self) -> Vec<String> {
        let trades = self.trades.read();
        let mut counts: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
        for t in trades.iter() {
            if t.status != "✓" { continue; }
            match t.kind.as_str() {
                "BUY"  => *counts.entry(t.mint.clone()).or_insert(0) += 1,
                "SELL" => *counts.entry(t.mint.clone()).or_insert(0) -= 1,
                _ => {}
            }
        }
        counts.into_iter().filter(|(_, v)| *v > 0).map(|(k, _)| k).collect()
    }

    /// Export the trades deque to a CSV file. Returns the path written.
    pub fn export_trades_csv(&self) -> anyhow::Result<String> {
        use std::io::Write;
        let path = format!(
            "trades-export-{}.csv",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        );
        let mut f = std::fs::File::create(&path)?;
        writeln!(f, "timestamp,kind,status,mint,amount,pnl,signature")?;
        let trades = self.trades.read();
        for t in trades.iter() {
            writeln!(
                f,
                "{},{},{},{},{:.6},{:.6},{}",
                t.timestamp.format("%Y-%m-%dT%H:%M:%SZ"),
                t.kind, t.status, t.mint, t.amount, t.pnl, t.signature
            )?;
        }
        Ok(path)
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
