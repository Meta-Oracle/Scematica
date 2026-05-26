use crate::chat::{ChatLine, ChatUpdate};
use crate::onboarding::OnboardingManager;
use crate::process::BotCommand;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use scematica_ai::chat_types::PendingToolCall;
use scematica_ai::tool_dispatcher::LiveData;
use scematica_core::metrics::{
    BotMetrics, MetricsSnapshot, StrategySnapshot, TradeEvent, METRICS_FILE, STRATEGY_FILE,
    TRADES_FILE,
};
use scematica_core::rpc::RpcConnection;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

/// Maximum number of log lines to keep in memory
const MAX_LOG_LINES: usize = 200;
/// Maximum number of trade history entries
const MAX_TRADES: usize = 100;
/// Number of PnL data points to keep for the sparkline
const SPARKLINE_CAPACITY: usize = 60;
/// Max age of radar pool entries (5 minutes)
const RADAR_MAX_AGE_SECS: i64 = 300;
/// Number of top-level dashboard tabs rendered by the TUI.
pub const DASHBOARD_TAB_COUNT: usize = 6;

#[derive(Debug, Clone)]
pub struct TradeEntry {
    pub timestamp: DateTime<Utc>,
    pub kind: String,
    pub mint: String,
    pub amount: f64,
    pub pnl: f64,
    pub status: String,
    pub signature: String,
    pub exit_reason: String,
    pub pnl_pct: f64,
    pub position_age_secs: f64,
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
        chrono::Utc::now()
            .timestamp()
            .saturating_sub(self.entry_unix_secs)
            .max(0)
    }
    pub fn pnl_pct(&self) -> f64 {
        if self.entry_lamports == 0 {
            0.0
        } else {
            (self.current_value_lamports as f64 - self.entry_lamports as f64)
                / self.entry_lamports as f64
                * 100.0
        }
    }
    pub fn peak_pnl_pct(&self) -> f64 {
        if self.entry_lamports == 0 {
            0.0
        } else {
            (self.peak_value_lamports as f64 - self.entry_lamports as f64)
                / self.entry_lamports as f64
                * 100.0
        }
    }
    pub fn entry_sol(&self) -> f64 {
        self.entry_lamports as f64 / 1e9
    }
    pub fn value_sol(&self) -> f64 {
        self.current_value_lamports as f64 / 1e9
    }
    pub fn sl_sol(&self) -> f64 {
        self.current_sl_lamports as f64 / 1e9
    }
    /// 0.0–1.0: how far current value sits between SL (0) and TP (1)
    pub fn progress_to_tp(&self) -> f64 {
        if self.entry_lamports == 0 {
            return 0.5;
        }
        let sl = self.current_sl_lamports as f64;
        let tp = self.entry_lamports as f64 * (1.0 + self.dynamic_tp_pct / 100.0);
        let cur = self.current_value_lamports as f64;
        if tp <= sl {
            return 0.5;
        }
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
    /// Latest SOL/USD price from CoinGecko — refreshed every 60 s. 0.0 = not yet loaded.
    pub sol_price_usd: RwLock<f64>,
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
    /// Vertical scroll offset for the Config tab (Up/Down arrows)
    pub config_scroll: RwLock<u16>,
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
    /// NN tournament variant stats from scematica-nn-tournament.json
    pub tournament_stats: RwLock<Option<serde_json::Value>>,
    /// Deployer reputation ledger from scematica-deployer-reputation.json
    pub deployer_reputation: RwLock<Option<serde_json::Value>>,
    /// Unix milliseconds of last confirmed trade (for border flash)
    pub last_trade_flash_ts: std::sync::atomic::AtomicI64,
    /// True if the last trade was a win (for flash color)
    pub last_trade_was_win: std::sync::atomic::AtomicBool,
    /// Session cumulative PnL in SOL (all confirmed SELLs)
    pub cumulative_pnl_sol: RwLock<f64>,
    /// Running peak of cumulative_pnl_sol (for drawdown computation)
    pub session_peak_pnl_sol: RwLock<f64>,
    /// Time-series equity curve: (unix_ts, cumulative_pnl_sol) — up to 500 points
    pub pnl_equity_curve: RwLock<VecDeque<(i64, f64)>>,
    /// Big runner callout: (pnl_pct, unix_secs_when_set) — displayed for 10s
    pub big_runner: RwLock<Option<(f64, i64)>>,
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
            BuilderMode::Off => 0.0,
            BuilderMode::Growth => 0.2,
            BuilderMode::Builder => 1.0,
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
            BuilderMode::Off => "Off",
            BuilderMode::Growth => "Growth",
            BuilderMode::Builder => "Builder",
            BuilderMode::SuperBuilder => "SuperBuilder",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            BuilderMode::Off => "off",
            BuilderMode::Growth => "growth",
            BuilderMode::Builder => "builder",
            BuilderMode::SuperBuilder => "super_builder",
        }
    }
    /// Short algorithm description shown in the Control tab.
    pub fn algo_description(self) -> &'static str {
        match self {
            BuilderMode::Off => "config.toml values (no algorithm override)",
            BuilderMode::Growth => "size 1.0-2.0x  TP base  SL base",
            BuilderMode::Builder => "size 1.5-3.5x (p^0.65)  TP 1.5x->1.0x base  SL widens early",
            BuilderMode::SuperBuilder => {
                "size 2.0-8.0x (p^0.35)  TP 2.0x->1.0x base  moon-chase p<25%"
            }
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
            RateMode::Bearish => 0.3,
            RateMode::Micro => 0.1,
            RateMode::Safe => 0.5,
            RateMode::Balanced => 1.0,
            RateMode::Aggressive => 2.0,
            RateMode::Degen => 4.0,
            RateMode::Bullish => 6.0,
            RateMode::Moon => 8.0,
        }
    }
    pub fn tp_pct(self) -> f64 {
        match self {
            RateMode::Bearish => 45.0,
            RateMode::Micro => 60.0,
            RateMode::Safe => 75.0,
            RateMode::Balanced => 150.0,
            RateMode::Aggressive => 300.0,
            RateMode::Degen => 450.0,
            RateMode::Bullish => 750.0,
            // Moon targets 1000× — escalation ladder rides from 10000% → 20000% → 40000% → ...
            RateMode::Moon => 100000.0,
        }
    }
    /// Wallet % to use per trade — 0.0 means use fixed quote_amount instead
    pub fn wallet_pct(self) -> f64 {
        match self {
            RateMode::Micro => 0.3,
            RateMode::Bearish => 0.5,
            RateMode::Safe => 0.8,
            RateMode::Balanced => 1.5,
            RateMode::Aggressive => 3.0,
            RateMode::Degen => 6.0,
            RateMode::Bullish => 8.0,
            RateMode::Moon => 12.0,
        }
    }
    pub fn sl_pct(self) -> f64 {
        match self {
            RateMode::Bearish => 8.0,
            RateMode::Micro => 10.0,
            RateMode::Safe => 10.0,
            RateMode::Balanced => 15.0,
            RateMode::Aggressive => 25.0,
            RateMode::Degen => 40.0,
            RateMode::Bullish => 50.0,
            RateMode::Moon => 60.0,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            RateMode::Bearish => "Bearish",
            RateMode::Micro => "Micro",
            RateMode::Safe => "Safe",
            RateMode::Balanced => "Balanced",
            RateMode::Aggressive => "Aggressive",
            RateMode::Degen => "Degen",
            RateMode::Bullish => "Bullish",
            RateMode::Moon => "Moon",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            RateMode::Bearish => "bearish",
            RateMode::Micro => "micro",
            RateMode::Safe => "safe",
            RateMode::Balanced => "balanced",
            RateMode::Aggressive => "aggressive",
            RateMode::Degen => "degen",
            RateMode::Bullish => "bullish",
            RateMode::Moon => "moon",
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

/// Reformat a raw tracing-subscriber log line into a compact TUI-friendly string.
///
/// Tracing default format: "2026-05-18T05:17:50.123456Z  INFO target::module: message"
/// Output:                 "[SNIPER] 05:17:50 [INFO] message"
///
/// Returns None for blank/whitespace-only lines (suppresses empty log entries).
fn format_sniper_log_line(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    // Detect tracing-subscriber compact timestamp format:
    // starts with 4-digit year, has 'T' at position 10.
    let bytes = s.as_bytes();
    if bytes.len() > 25
        && bytes[0].is_ascii_digit()
        && bytes[4] == b'-'
        && bytes.get(10) == Some(&b'T')
    {
        // time portion starts at index 11: "HH:MM:SS"
        let time = &s[11..19.min(s.len())];

        // Find the 'Z' that ends the timestamp (always within first 30 chars)
        if let Some(z_rel) = s[19..s.len().min(30)].find('Z') {
            let z_abs = 19 + z_rel;
            let after = s[z_abs + 1..].trim_start(); // "INFO target::path: message"

            // Level is the first word; target ends at ": "
            let (level, message) = if let Some(colon) = after.find(": ") {
                let level_and_target = &after[..colon];
                let level = level_and_target.split_whitespace().next().unwrap_or("INFO");
                let msg = &after[colon + 2..];
                (level, msg)
            } else {
                ("INFO", after)
            };

            let msg = message.trim();
            if msg.is_empty() {
                return None;
            }
            return Some(format!("[SNIPER] {} [{}] {}", time, level, msg));
        }
    }

    // Non-tracing line (continuation, panic output, etc.) — pass through.
    Some(format!("[SNIPER] {}", s))
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
            sol_price_usd: RwLock::new(0.0),
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
            config_scroll: RwLock::new(0),
            price_history: RwLock::new(std::collections::HashMap::new()),
            radar_pools: RwLock::new(Vec::new()),
            live_positions: RwLock::new(Vec::new()),
            live_positions_file_seen: std::sync::atomic::AtomicBool::new(false),
            alert_history: RwLock::new(VecDeque::with_capacity(5)),
            tournament_stats: RwLock::new(None),
            deployer_reputation: RwLock::new(None),
            last_trade_flash_ts: std::sync::atomic::AtomicI64::new(0),
            last_trade_was_win: std::sync::atomic::AtomicBool::new(false),
            cumulative_pnl_sol: RwLock::new(0.0),
            session_peak_pnl_sol: RwLock::new(0.0),
            pnl_equity_curve: RwLock::new(VecDeque::with_capacity(500)),
            big_runner: RwLock::new(None),
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
                    while sl.len() > SPARKLINE_CAPACITY {
                        sl.pop_front();
                    }
                }
                // Best / worst
                {
                    let mut best = self.best_trade_pnl.write();
                    if pnl > *best {
                        *best = pnl;
                    }
                }
                {
                    let mut worst = self.worst_trade_pnl.write();
                    if pnl < *worst {
                        *worst = pnl;
                    }
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
                // Equity curve
                {
                    let mut cum = self.cumulative_pnl_sol.write();
                    *cum += pnl;
                    let cum_now = *cum;
                    let ts = event.timestamp.timestamp();
                    let mut curve = self.pnl_equity_curve.write();
                    curve.push_back((ts, cum_now));
                    while curve.len() > 500 {
                        curve.pop_front();
                    }
                    let mut peak = self.session_peak_pnl_sol.write();
                    if cum_now > *peak {
                        *peak = cum_now;
                    }
                }
                // Big runner callout (>= 500% PnL)
                if event.pnl_pct >= 500.0 {
                    *self.big_runner.write() =
                        Some((event.pnl_pct, chrono::Utc::now().timestamp()));
                }
            }
            // Border flash on any confirmed trade
            if event.status == "✓" {
                self.last_trade_flash_ts.store(
                    chrono::Utc::now().timestamp_millis(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                self.last_trade_was_win
                    .store(event.pnl >= 0.0, std::sync::atomic::Ordering::Relaxed);
            }
            // Alert history: record confirmed BUY/SELL events (last 5 visible in overview)
            if event.status == "✓" {
                let title = format!("{} {}", event.status, event.kind.clone());
                let body = if event.kind == "SELL" {
                    format!(
                        "{}… PnL: {:.4} SOL",
                        &event.mint[..8.min(event.mint.len())],
                        event.pnl
                    )
                } else {
                    format!(
                        "{}… {:.4} SOL",
                        &event.mint[..8.min(event.mint.len())],
                        event.amount
                    )
                };
                let mut ah = self.alert_history.write();
                if ah.len() >= 5 {
                    ah.pop_back();
                }
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
                exit_reason: event.exit_reason,
                pnl_pct: event.pnl_pct,
                position_age_secs: event.position_age_secs,
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

        let Ok(mut file) = std::fs::File::open(LOG_FILE) else {
            return;
        };

        // If the file shrank (rotation, manual truncate), reset to the new length so
        // we don't perma-block on a stale offset past EOF.
        if let Ok(meta) = file.metadata() {
            if meta.len() < offset {
                offset = meta.len();
                *self.sniper_log_offset.write() = offset;
            }
        }

        if file.seek(SeekFrom::Start(offset)).is_err() {
            return;
        }
        let mut reader = BufReader::new(&mut file);
        let mut new_offset = offset;
        loop {
            let mut raw_line = String::new();
            match reader.read_line(&mut raw_line) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    // read_line returns exact byte count including \n or \r\n —
                    // use it directly rather than approximating with len+1.
                    new_offset += n as u64;
                    let content = raw_line.trim_end_matches(|c| c == '\n' || c == '\r');
                    if let Some(formatted) = format_sniper_log_line(content) {
                        self.push_log(formatted);
                    }
                }
                Err(_) => break,
            }
        }
        if new_offset != offset {
            *self.sniper_log_offset.write() = new_offset;
        }
    }

    /// Read filter rejection stats from disk and cache them for the UI.
    pub fn poll_filter_stats_file(&self) {
        const STATS_FILE: &str = "scematica-filter-stats.json";
        let Ok(data) = std::fs::read_to_string(STATS_FILE) else {
            return;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else {
            return;
        };
        *self.filter_stats.write() = Some(v);
    }

    /// Read NN agent stats from scematica-nn-stats.json.
    pub fn poll_nn_stats_file(&self) {
        let Ok(data) = std::fs::read_to_string("scematica-nn-stats.json") else {
            return;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else {
            return;
        };
        *self.nn_stats.write() = Some(v);
    }

    /// Read NN tournament stats from scematica-nn-tournament.json.
    pub fn poll_tournament_file(&self) {
        let Ok(data) = std::fs::read_to_string("scematica-nn-tournament.json") else {
            return;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else {
            return;
        };
        *self.tournament_stats.write() = Some(v);
    }

    /// Read deployer reputation ledger from scematica-deployer-reputation.json.
    pub fn poll_deployer_reputation_file(&self) {
        let Ok(data) = std::fs::read_to_string("scematica-deployer-reputation.json") else {
            return;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else {
            return;
        };
        *self.deployer_reputation.write() = Some(v);
    }

    /// Read live open positions from scematica-positions.json (sniper writes every 1 s).
    pub fn poll_live_positions_file(&self) {
        const POS_FILE: &str = "scematica-positions.json";
        let Ok(data) = std::fs::read_to_string(POS_FILE) else {
            return;
        };
        let Ok(positions) = serde_json::from_str::<Vec<LivePosition>>(&data) else {
            return;
        };
        *self.live_positions.write() = positions;
        // Mark that the file has been seen at least once. After this point,
        // open_position_count() trusts the file count (even when 0) instead of
        // falling back to the trade-history deque — preventing phantom positions.
        self.live_positions_file_seen
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Read pool radar entries from scematica-pool-radar.json.
    /// Drops any entries older than 5 minutes.
    pub fn poll_radar_file(&self) {
        const RADAR_FILE: &str = "scematica-pool-radar.json";
        let Ok(data) = std::fs::read_to_string(RADAR_FILE) else {
            return;
        };
        let Ok(mut pools) = serde_json::from_str::<Vec<RadarPool>>(&data) else {
            return;
        };
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
            if entry.len() > 60 {
                entry.pop_front();
            }
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
        *tab = (*tab + 1) % DASHBOARD_TAB_COUNT;
    }

    pub fn prev_tab(&self) {
        let mut tab = self.selected_tab.write();
        *tab = tab
            .checked_sub(1)
            .unwrap_or(DASHBOARD_TAB_COUNT.saturating_sub(1));
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
        if self
            .live_positions_file_seen
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            self.live_positions.read().len()
        } else {
            // Sniper hasn't written the positions file yet — derive from trade history
            let live = self.live_positions.read().len();
            if live > 0 {
                live
            } else {
                self.open_position_mints().len()
            }
        }
    }

    /// Derive currently open positions from trade history.
    /// Returns mints that have more confirmed BUYs than confirmed SELLs in the deque.
    pub fn open_position_mints(&self) -> Vec<String> {
        let trades = self.trades.read();
        let mut counts: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
        for t in trades.iter() {
            if t.status != "✓" {
                continue;
            }
            match t.kind.as_str() {
                "BUY" => *counts.entry(t.mint.clone()).or_insert(0) += 1,
                "SELL" => *counts.entry(t.mint.clone()).or_insert(0) -= 1,
                _ => {}
            }
        }
        counts
            .into_iter()
            .filter(|(_, v)| *v > 0)
            .map(|(k, _)| k)
            .collect()
    }

    /// Export the trades deque to a CSV file. Returns the path written.
    pub fn export_trades_csv(&self) -> anyhow::Result<String> {
        use std::io::Write;
        let path = format!(
            "trades-export-{}.csv",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        );
        let mut f = std::fs::File::create(&path)?;
        writeln!(
            f,
            "timestamp,kind,status,mint,amount,pnl,pnl_pct,position_age_secs,exit_reason,signature"
        )?;
        let trades = self.trades.read();
        for t in trades.iter() {
            writeln!(
                f,
                "{},{},{},{},{:.6},{:.6},{:.2},{:.1},{},{}",
                t.timestamp.format("%Y-%m-%dT%H:%M:%SZ"),
                t.kind,
                t.status,
                t.mint,
                t.amount,
                t.pnl,
                t.pnl_pct,
                t.position_age_secs,
                t.exit_reason,
                t.signature
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
