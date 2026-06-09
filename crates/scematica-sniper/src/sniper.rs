use chrono::Timelike;
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use scematica_ai::agents::AiCoordinator;
use scematica_core::{
    config::{ExecutionConfig, SniperConfig},
    metrics::BotMetrics,
    token::{apply_slippage, get_ata, resolve_mint, ui_to_raw},
    types::known_tokens,
};
use scematica_nn::{DQNAgent, TradeAction, TradeState as NNState};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer};
use spl_associated_token_account;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::Result;
use scematica_core::types::DexKind;
use scematica_executor::{get_builder, SwapInstructionBuilder};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::{
    alerts::AlertManager,
    ath_tracker::AthTracker,
    cache::{CachedPool, MarketCache, PoolCache, SnipeListCache},
    day_weight::DayWeighter,
    executor::{DefaultExecutor, JitoExecutor, TxExecutor},
    fibonacci_momentum::FibonacciMomentum,
    fibonacci_recovery_system::{
        FibonacciRecoveryConfig, FibonacciRecoveryStats, FibonacciRecoverySystem,
    },
    filters::FilterPipeline,
    grief_breaker::GriefBreaker,
    kelly::KellySizer,
    listener::ListenerEvent,
    pool_scorer::PoolScorer,
    reputation::DeployerLedger,
};
use scematica_core::metrics::{
    artifact_path, artifact_path_string, PoolDecisionEvent, StrategySnapshot, TradeEvent,
    NN_ADVICE_FILE, POOL_DECISIONS_FILE, POOL_RADAR_FILE, SELL_MODE_FILE, STRATEGY_FILE,
    TRADES_FILE,
};

/// Raydium constant-product AMM output with 0.25% fee.
/// out = (reserve_out * amount_in * 9975) / (reserve_in * 10000 + amount_in * 9975)
#[inline]
fn amm_out(amount_in: u64, reserve_in: u64, reserve_out: u64) -> u64 {
    let num = (reserve_out as u128) * (amount_in as u128) * 9975u128;
    let den = (reserve_in as u128) * 10000u128 + (amount_in as u128) * 9975u128;
    if den == 0 {
        return 0;
    }
    (num / den) as u64
}

/// RAII guard: holds the one_token_at_a_time processing lock and unconditionally
/// releases it on Drop. Prevents lock leaks on early returns, panics, or
/// awaits cancelled by an enclosing timeout — any of which previously left the
/// sniper unable to process *any* further pool until restart.
///
/// Call `release_to_sell_monitor()` when you intentionally want to hand the lock
/// off to the post-buy sell monitor (only released when the position closes).
struct ProcessingSlot<'a> {
    lock: &'a Arc<AtomicBool>,
    armed: bool,
}

impl<'a> ProcessingSlot<'a> {
    /// Try to claim the slot atomically. Returns `None` if already held.
    fn try_acquire(lock: &'a Arc<AtomicBool>) -> Option<Self> {
        if lock
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            Some(Self { lock, armed: true })
        } else {
            None
        }
    }
}

impl<'a> Drop for ProcessingSlot<'a> {
    fn drop(&mut self) {
        if self.armed {
            self.lock.store(false, Ordering::Relaxed);
        }
    }
}

/// Live-adjustable trading parameters — updated by the Strategy Agent at runtime.
/// Wrapped in RwLock so the strategy loop can write while the sniper reads.
#[derive(Debug, Clone)]
pub struct LiveParams {
    pub take_profit_pct: f64,
    pub stop_loss_pct: f64,
    /// Multiplier applied to the base quote_amount_raw from config
    pub amount_multiplier: f64,
    /// Human-readable market regime label from the AI
    pub market_regime: String,
    /// Current active rate mode name
    pub active_mode_name: String,
    /// Entry size in SOL for the active rate mode (used when wallet_pct == 0)
    pub quote_amount_mode: f64,
    /// TP% for the active rate mode
    pub take_profit_pct_mode: f64,
    /// SL% for the active rate mode
    pub stop_loss_pct_mode: f64,
    /// Max TP escalations for the active rate mode
    pub momentum_max_escalations_mode: u32,
    /// Wallet-percentage sizing: position = wallet_balance × wallet_pct / 100.
    /// 0.0 = use fixed quote_amount_mode instead.
    pub wallet_pct: f64,
}

impl LiveParams {
    pub fn from_config(config: &SniperConfig) -> Self {
        let active_mode = config.get_active_rate_mode().cloned();
        let (quote_amount, tp_pct, sl_pct, momentum_max, wallet_pct) =
            if let Some(mode) = active_mode {
                (
                    mode.quote_amount,
                    mode.take_profit_pct,
                    mode.stop_loss_pct,
                    mode.momentum_max_escalations,
                    mode.wallet_pct,
                )
            } else {
                (
                    config.quote_amount,
                    config.take_profit_pct,
                    config.stop_loss_pct,
                    config.momentum_max_escalations,
                    0.0,
                )
            };

        Self {
            take_profit_pct: config.take_profit_pct,
            stop_loss_pct: config.stop_loss_pct,
            amount_multiplier: 1.0,
            market_regime: "neutral".into(),
            active_mode_name: config.active_mode_name.clone(),
            quote_amount_mode: quote_amount,
            take_profit_pct_mode: tp_pct,
            stop_loss_pct_mode: sl_pct,
            momentum_max_escalations_mode: momentum_max,
            wallet_pct,
        }
    }
}

/// Core sniper bot: receives pool events and executes buy/sell
pub struct Sniper {
    pub config: SniperConfig,
    wallet: Arc<Keypair>,
    rpc: Arc<RpcClient>,
    pool_cache: PoolCache,
    #[allow(dead_code)]
    market_cache: MarketCache,
    snipe_list: Option<SnipeListCache>,
    filter_pipeline: FilterPipeline,
    executor: Arc<dyn TxExecutor>,
    metrics: Arc<BotMetrics>,
    /// AI coordinator — None if no API key is configured
    ai: Option<Arc<AiCoordinator>>,
    /// Deep Q* agent used for current-pool entry advice and trade learning.
    nn_agent: Option<Arc<std::sync::Mutex<DQNAgent>>>,
    /// AtomicBool: true while a buy + sell-monitor pair owns the processing slot.
    processing_lock: Arc<std::sync::atomic::AtomicBool>,
    quote_mint: Pubkey,
    quote_decimals: u8,
    quote_amount_raw: u64,
    /// Raydium swap instruction builder
    raydium_builder: Arc<dyn SwapInstructionBuilder>,
    /// Live-adjustable parameters updated by the Strategy Agent
    pub live_params: Arc<RwLock<LiveParams>>,
    /// Recent trade outcomes for strategy agent input: (was_profitable, pnl_sol)
    trade_history: Arc<Mutex<Vec<(bool, f64)>>>,
    /// Set true by the sell-mode file watcher to pause buys and force-sell everything
    pub sell_mode: Arc<AtomicBool>,
    /// Set true by the dump-mode file watcher — sells all positions immediately with min_out=0
    pub dump_mode: Arc<AtomicBool>,
    /// HIGH-SPEED MODE — set true by the high-speed-mode file watcher. When on:
    ///   • the filter pipeline is bypassed entirely (we trust the listener gate)
    ///   • AI risk + pool-scorer + radar pre-fetch are skipped
    ///   • the executor uses a tighter per-attempt deadline + lower 429 backoff
    ///   • compute_unit_price is escalated 3× to win priority races
    /// Deliberately accepts 429 / failed-tx noise as the cost of doing business.
    pub high_speed_mode: Arc<AtomicBool>,
    /// Last unix-second when we surfaced a "skipped due to sell/dump mode" log line.
    /// Used to throttle the (otherwise per-pool spammy) info messages to ~once / 30 s,
    /// so the operator can see *why* nothing is being bought without the log scrolling.
    skip_log_throttle_secs: Arc<AtomicU64>,
    /// Semaphore: limits concurrent sell transactions to avoid 429 hammering
    sell_sem: Arc<tokio::sync::Semaphore>,
    /// Counts successful buys this session — activates sell mode when config.max_buys is hit
    pub buy_count: Arc<std::sync::atomic::AtomicU32>,
    /// Number of currently open positions (active sell monitors)
    pub open_positions: Arc<std::sync::atomic::AtomicU32>,
    /// Consecutive losing trades counter (reset on win)
    pub consecutive_losses: Arc<std::sync::atomic::AtomicU32>,
    /// Unix timestamp (ms) after which buying resumes following a cooldown
    pub cooldown_until_ms: Arc<std::sync::atomic::AtomicU64>,
    /// Accumulated daily PnL in lamports (negative = loss; reset at midnight)
    pub daily_pnl_lamports: Arc<parking_lot::Mutex<i64>>,
    /// SOL balance (lamports) at session start — used for drawdown calculation
    pub session_start_lamports: Arc<std::sync::atomic::AtomicU64>,
    /// Alert manager for Telegram / Discord / desktop notifications
    pub alerts: Arc<AlertManager>,
    /// Deployer reputation ledger — tracks rug/success history per deployer pubkey
    pub deployer_ledger: Arc<Mutex<DeployerLedger>>,
    /// Sliding-window grief-loss circuit breaker (None = disabled)
    pub grief_breaker: Option<Arc<GriefBreaker>>,
    /// Session ATH wallet balance tracker
    pub ath_tracker: Arc<AthTracker>,
    /// Kelly Criterion position sizer (None = disabled)
    pub kelly_sizer: Option<KellySizer>,
    /// Pool predictive scorer
    pub pool_scorer: PoolScorer,
    /// ATH drawdown % that pauses buying (0.0 = disabled)
    ath_drawdown_pct: f64,
    /// Session heat loss timestamps (unix seconds) — rolling window for session_heat_losses check
    pub loss_heat_timestamps: Arc<parking_lot::Mutex<Vec<u64>>>,
    /// Gas war mode: compute unit price escalation counter
    /// Stores timestamp of last pool detection in ms for burst detection
    pub gas_war_last_pool_ms: Arc<AtomicU64>,
    /// Session PnL baseline in lamports (used for profit extraction)
    pub session_pnl_baseline_lamports: Arc<Mutex<i64>>,
    /// Live-mutable wallet growth target in lamports. Set by the builder-mode
    /// file watcher; 0 means "use config.wallet_target_sol". Sell-monitor reads
    /// this on every check so dashboard toggles take effect mid-position.
    pub wallet_target_lamports_override: Arc<AtomicU64>,
    /// When true, buy() applies a progressive multiplier on top of the rate-mode
    /// multiplier: as approx_wallet/target ratio rises, position size grows.
    /// Engaged by Super Builder mode.
    pub progressive_scaling: Arc<AtomicBool>,
    /// Moon Chase: when true, the sell monitor swaps the momentum-hold params
    /// for the aggressive moon-chase set (more escalations, higher factor,
    /// wider pullback). Toggled by the dashboard via scematica-moon-chase.json.
    pub moon_chase: Arc<AtomicBool>,
    /// Live position registry — each SellMonitor inserts on entry, updates per
    /// price check, removes on exit. A background flush task in main.rs writes
    /// this map to `scematica-positions.json` every 1s for the dashboard panel.
    pub live_positions: Arc<DashMap<String, LivePositionSnapshot>>,
    /// Dedup guard: mint → unix-second timestamp of last confirmed buy.
    /// Using unix seconds (not Instant) so the map can be persisted to
    /// `scematica-mint-cooldown.json` and survive process restarts.
    recently_bought: Arc<DashMap<Pubkey, u64>>,
    /// Fibonacci recovery system for entry/exit optimization
    pub fib_system: Arc<FibonacciRecoverySystem>,
    /// Fibonacci recovery statistics
    pub fib_stats: Arc<Mutex<FibonacciRecoveryStats>>,
    /// DexScreener boost cache — tokens with active paid boosts bypass Fibonacci + pool score gates
    dex_cache: Arc<crate::dexscreener::DexScreenerCache>,
}

/// Per-position snapshot written to `scematica-positions.json`. Each running
/// SellMonitor task updates its entry every price check; the dashboard reads
/// the JSON every tick and renders one row per snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LivePositionSnapshot {
    pub mint: String,
    pub entry_lamports: u64,
    pub current_value_lamports: u64,
    pub peak_value_lamports: u64,
    pub entry_unix_secs: i64,
    /// Per-position dynamic TP (updated by momentum escalation)
    pub dynamic_tp_pct: f64,
    /// How many momentum escalations have fired (0 = none)
    pub escalations: u32,
    /// Unix seconds of last price-check update — staleness indicator
    pub last_check_unix_secs: i64,
    /// Current active stop-loss level in lamports (reflects trailing stop + profit lock)
    pub current_sl_lamports: u64,
    /// Current SL as % from entry (negative = loss floor, positive = breakeven or above)
    pub current_sl_pct: f64,
    /// Consecutive declining price-check ticks (≥3 triggers dump-detection sell)
    pub decline_streak: u32,
}

const MINT_COOLDOWN_FILE: &str = "scematica-mint-cooldown.json";

/// Load the persisted mint re-entry cooldown map from disk.
/// Entries that have already expired (older than mint_cooldown_secs) are dropped on load
/// so the in-memory map stays compact.
fn load_mint_cooldown(config: &scematica_core::config::SniperConfig) -> Arc<DashMap<Pubkey, u64>> {
    let map: DashMap<Pubkey, u64> = DashMap::new();
    let cooldown = config.mint_cooldown_secs;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if let Ok(data) = std::fs::read_to_string(MINT_COOLDOWN_FILE) {
        if let Ok(entries) = serde_json::from_str::<std::collections::HashMap<String, u64>>(&data) {
            let loaded = entries.len();
            let mut kept = 0usize;
            for (key_str, ts) in entries {
                if now_secs.saturating_sub(ts) < cooldown {
                    if let Ok(pk) = key_str.parse::<Pubkey>() {
                        map.insert(pk, ts);
                        kept += 1;
                    }
                }
            }
            if loaded > 0 {
                tracing::info!(
                    loaded,
                    kept,
                    "Loaded mint cooldown file — {} active cooldowns restored",
                    kept
                );
            }
        }
    }
    Arc::new(map)
}

/// Atomically persist the mint cooldown map to disk so it survives restarts.
fn save_mint_cooldown(map: &Arc<DashMap<Pubkey, u64>>) {
    let entries: std::collections::HashMap<String, u64> = map
        .iter()
        .map(|e| (e.key().to_string(), *e.value()))
        .collect();
    if let Ok(json) = serde_json::to_string(&entries) {
        let tmp = format!("{}.tmp", MINT_COOLDOWN_FILE);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, MINT_COOLDOWN_FILE);
        }
    }
}

impl Sniper {
    pub fn new(
        config: SniperConfig,
        wallet: Arc<Keypair>,
        rpc: Arc<RpcClient>,
        metrics: Arc<BotMetrics>,
        alerts: Arc<AlertManager>,
        nn_agent: Option<Arc<std::sync::Mutex<DQNAgent>>>,
        execution: ExecutionConfig,
    ) -> Self {
        let quote_mint = resolve_mint(&config.quote_mint).unwrap_or(known_tokens::WSOL_MINT);
        let quote_decimals = if config.quote_mint.to_uppercase() == "USDC" {
            6
        } else {
            9
        };
        let quote_amount_raw = ui_to_raw(config.quote_amount, quote_decimals);

        let deployer_ledger_shared = Arc::new(Mutex::new(DeployerLedger::load()));
        let filter_pipeline = FilterPipeline::new(
            config.filters.clone(),
            rpc.clone(),
            quote_amount_raw,
            &config.blacklist_path,
            Some(Arc::clone(&deployer_ledger_shared)),
        );

        let executor: Arc<dyn TxExecutor> = match execution.executor.as_str() {
            "jito" => Arc::new(JitoExecutor::new(
                execution.jito_url.clone(),
                execution.custom_fee_sol,
            )),
            _ if std::env::var("EXECUTOR").unwrap_or_default() == "jito" => {
                Arc::new(JitoExecutor::new(
                    std::env::var("JITO_URL")
                        .unwrap_or_else(|_| "https://mainnet.block-engine.jito.wtf".into()),
                    0.006,
                ))
            }
            _ => Arc::new(
                DefaultExecutor::new(
                    execution.compute_unit_limit,
                    execution.compute_unit_price,
                    execution.skip_preflight,
                    config.max_buy_retries.max(1),
                )
                .with_dynamic_fees()
                .with_priority_fee_hard_cap(execution.compute_unit_price_hard_cap)
                .with_loaded_accounts_data_size_limit(execution.loaded_accounts_data_size_limit),
            ),
        };

        let snipe_list = if config.use_snipe_list {
            let sl = SnipeListCache::new(&config.snipe_list_path);
            let _ = sl.load();
            Some(sl)
        } else {
            None
        };

        let live_params = Arc::new(RwLock::new(LiveParams::from_config(&config)));

        // Construct new safety/sizing modules from config
        let grief_breaker = if config.grief_loss_limit_sol > 0.0 {
            Some(Arc::new(GriefBreaker::new(
                config.grief_loss_window_secs,
                config.grief_loss_limit_sol,
            )))
        } else {
            None
        };

        let kelly_sizer = if config.kelly_sizing {
            Some(KellySizer::with_min_trades(
                config.kelly_fraction,
                config.kelly_min_trades,
            ))
        } else {
            None
        };

        let ath_drawdown_pct = config.ath_drawdown_pct;
        let recently_bought = load_mint_cooldown(&config);

        let fib_config = FibonacciRecoveryConfig::default();
        let fib_system = Arc::new(FibonacciRecoverySystem::new(fib_config));
        let fib_stats = Arc::new(Mutex::new(FibonacciRecoveryStats::default()));
        let dex_cache = Arc::new(crate::dexscreener::DexScreenerCache::new());

        Self {
            config,
            wallet,
            rpc: rpc.clone(),
            pool_cache: PoolCache::new(),
            market_cache: MarketCache::new(),
            snipe_list,
            filter_pipeline,
            executor,
            metrics,
            ai: AiCoordinator::from_env_optional().map(Arc::new),
            nn_agent,
            processing_lock: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            quote_mint,
            quote_decimals,
            quote_amount_raw,
            raydium_builder: Arc::from(
                get_builder(DexKind::Raydium, rpc.clone()).expect("Raydium builder not found"),
            ),
            live_params,
            trade_history: Arc::new(Mutex::new(Vec::new())),
            sell_mode: Arc::new(AtomicBool::new(false)),
            dump_mode: Arc::new(AtomicBool::new(false)),
            high_speed_mode: Arc::new(AtomicBool::new(false)),
            skip_log_throttle_secs: Arc::new(AtomicU64::new(0)),
            sell_sem: Arc::new(tokio::sync::Semaphore::new(5)),
            buy_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            open_positions: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            consecutive_losses: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            cooldown_until_ms: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            daily_pnl_lamports: Arc::new(parking_lot::Mutex::new(0i64)),
            session_start_lamports: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            alerts,
            deployer_ledger: deployer_ledger_shared,
            grief_breaker,
            ath_tracker: Arc::new(AthTracker::new()),
            kelly_sizer,
            pool_scorer: PoolScorer,
            ath_drawdown_pct,
            loss_heat_timestamps: Arc::new(parking_lot::Mutex::new(Vec::new())),
            gas_war_last_pool_ms: Arc::new(AtomicU64::new(0)),
            session_pnl_baseline_lamports: Arc::new(Mutex::new(0i64)),
            wallet_target_lamports_override: Arc::new(AtomicU64::new(0)),
            progressive_scaling: Arc::new(AtomicBool::new(false)),
            moon_chase: Arc::new(AtomicBool::new(false)),
            live_positions: Arc::new(DashMap::new()),
            recently_bought,
            fib_system,
            fib_stats,
            dex_cache,
        }
    }

    /// Config base values — exposed so the builder-mode watcher in main.rs
    /// can compute mode-specific TP/SL/mult relative to the configured baseline.
    fn build_nn_entry_state(
        &self,
        pool_size_sol: f64,
        pool_age_secs: f64,
        velocity_sol_per_sec: f64,
        buy_pressure_ratio: f64,
        pool_score: f64,
        inflow_rate_sol_per_sec: f64,
        wallet_balance_lamports: u64,
    ) -> NNState {
        let regime = match self.live_params.read().market_regime.as_str() {
            "bull" | "mania" => 1,
            "bear" | "panic" => -1,
            _ => 0,
        };
        NNState {
            pool_age_secs,
            initial_liquidity_sol: pool_size_sol,
            price_change_pct: 0.0,
            volume_5min_sol: (inflow_rate_sol_per_sec.max(0.0) * 300.0).min(100.0),
            buy_sell_ratio: buy_pressure_ratio.max(0.0).min(5.0),
            lp_burned: true,
            mint_renounced: true,
            current_pnl_pct: 0.0,
            position_age_secs: 0.0,
            daily_pnl_sol: *self.daily_pnl_lamports.lock() as f64 / 1e9,
            consecutive_wins: 0,
            consecutive_losses: self.consecutive_losses.load(Ordering::Relaxed) as i32,
            sol_balance_sol: wallet_balance_lamports as f64 / 1e9,
            regime,
            volatility: 0.0,
            spread_pct: self.config.buy_slippage_pct.max(0.0) / 100.0,
            time_of_day_norm: chrono::Utc::now().hour() as f64 / 24.0,
            open_positions: self.open_positions.load(Ordering::Relaxed) as i32,
            peak_pnl_pct: 0.0,
            pool_score_norm: (pool_score / 100.0).clamp(0.0, 1.0),
            deployer_rug_rate: 0.5,
            volume_velocity: (inflow_rate_sol_per_sec / 5.0).clamp(-1.0, 1.0),
            price_velocity: (velocity_sol_per_sec / 5.0).clamp(-1.0, 1.0),
            price_acceleration: 0.0,
        }
    }

    pub fn base_take_profit_pct(&self) -> f64 {
        self.config.take_profit_pct
    }
    pub fn base_stop_loss_pct(&self) -> f64 {
        self.config.stop_loss_pct
    }

    /// Approximate current wallet in SOL (session_start + realised daily PnL).
    /// Avoids an RPC call; used by builder-mode compounding equations every 5 s.
    pub fn approx_wallet_sol(&self) -> f64 {
        use std::sync::atomic::Ordering;
        let start = self.session_start_lamports.load(Ordering::Relaxed) as i64;
        let pnl = *self.daily_pnl_lamports.lock();
        (start + pnl).max(0) as f64 / 1e9
    }

    /// Run the Strategy Agent adjustment loop.
    /// Call this in a background task after constructing the Sniper.
    /// Evaluates recent trade history every `interval_secs` and updates live_params.
    pub async fn run_strategy_loop(self: Arc<Self>, interval_secs: u64) {
        let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
        // Make the no-op case observable: without an AI key the loop ticks forever
        // doing nothing, which looks identical to a crash from the outside (stale
        // scematica-strategy.json). Say so explicitly at startup.
        if self.ai.is_some() {
            info!(
                "Strategy agent loop started (interval: {}s) — AI configured, TP/SL will adapt",
                interval_secs
            );
        } else {
            warn!(
                "Strategy agent loop started (interval: {}s) but AI is DISABLED (no API key) — \
                 TP/SL/regime will NOT adapt and scematica-strategy.json will stay stale. \
                 Set an AI API key to enable.",
                interval_secs
            );
        }
        loop {
            ticker.tick().await;

            let ai = match &self.ai {
                Some(ai) => ai.clone(),
                None => continue, // no AI configured
            };

            let history = self.trade_history.lock().clone();
            if history.len() < 5 {
                debug!(
                    "Strategy agent: not enough trades yet ({}/5)",
                    history.len()
                );
                continue;
            }

            let params = self.live_params.read().clone();
            let snap = self.metrics.snapshot();
            let total_pnl = snap.total_pnl_sol();
            let win_rate = snap.win_rate();

            let adjustment = ai
                .strategy
                .get_adjustment(
                    &history,
                    total_pnl,
                    params.take_profit_pct,
                    params.stop_loss_pct,
                    scematica_core::token::raw_to_ui(self.quote_amount_raw, self.quote_decimals),
                    win_rate,
                )
                .await;

            // Apply the adjustment
            let mut live = self.live_params.write();
            if let Some(tp) = adjustment.take_profit_pct {
                info!(
                    "Strategy agent: take_profit {} → {:.1}% ({})",
                    live.take_profit_pct, tp, adjustment.market_regime
                );
                live.take_profit_pct = tp;
                // Propagate to mode-specific field so the sell monitor (which
                // reads take_profit_pct_mode first) actually uses the AI value.
                live.take_profit_pct_mode = tp;
            }
            if let Some(sl) = adjustment.stop_loss_pct {
                info!(
                    "Strategy agent: stop_loss {} → {:.1}% ({})",
                    live.stop_loss_pct, sl, adjustment.market_regime
                );
                live.stop_loss_pct = sl;
                live.stop_loss_pct_mode = sl;
            }
            if (adjustment.amount_multiplier - 1.0).abs() > 0.01 {
                info!(
                    "Strategy agent: amount_multiplier → {:.2}x ({})",
                    adjustment.amount_multiplier, adjustment.reasoning
                );
                live.amount_multiplier = adjustment.amount_multiplier;
            }
            let prev_regime = live.market_regime.clone();
            live.market_regime = adjustment.market_regime.clone();

            // Signal the NN agent to re-explore if regime changed
            if prev_regime != live.market_regime {
                info!(
                    "Strategy agent: regime shift {} → {} — writing NN signal",
                    prev_regime, live.market_regime
                );
                let _ = std::fs::write(
                    artifact_path("scematica-regime-shift.json"),
                    r#"{"shift":true}"#,
                );
            }

            // Persist snapshot so the dashboard can display live params
            StrategySnapshot {
                take_profit_pct: live.take_profit_pct,
                stop_loss_pct: live.stop_loss_pct,
                amount_multiplier: live.amount_multiplier,
                market_regime: live.market_regime.clone(),
                last_updated: chrono::Utc::now(),
            }
            .write_to_file(STRATEGY_FILE);
        }
    }

    /// Persist pool cache to disk so sell lookups survive restarts.
    pub fn persist_pool_cache(&self, path: &str) {
        self.pool_cache.persist_to_file(path);
    }

    /// Load pool cache from disk at startup. Merges with any in-memory entries.
    pub fn load_pool_cache(&self, path: &str) {
        self.pool_cache.load_from_file(path);
    }

    /// Record a completed trade outcome for the strategy agent.
    #[allow(dead_code)]
    fn record_trade_outcome(&self, profitable: bool, pnl_sol: f64) {
        let mut history = self.trade_history.lock();
        history.push((profitable, pnl_sol));
        // Keep last 20 trades
        if history.len() > 20 {
            history.remove(0);
        }
    }

    /// Append a pool radar entry to `scematica-pool-radar.json`.
    /// Reads the existing file, appends the new entry, trims to the last
    /// `RADAR_MAX_ENTRIES` records, then atomically writes back via a .tmp file.
    fn write_radar_entry(&self, pool: &CachedPool, pool_size_sol: f64, passed: bool, score: f64) {
        const RADAR_MAX_ENTRIES: usize = 100;

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let age_secs: f64 = if pool.open_time > 0 {
            (now_secs as u64).saturating_sub(pool.open_time) as f64
        } else {
            0.0
        };

        let entry = serde_json::json!({
            "mint":           pool.base_mint.to_string(),
            "age_secs":       age_secs,
            "size_sol":       pool_size_sol,
            "passed_filters": passed,
            "score":          score,
            "timestamp":      now_secs,
        });

        // Load existing entries (silently skip if file is absent or corrupt)
        let radar_path = artifact_path(POOL_RADAR_FILE);
        let mut entries: Vec<serde_json::Value> = std::fs::read_to_string(&radar_path)
            .ok()
            .and_then(|d| serde_json::from_str(&d).ok())
            .unwrap_or_default();

        entries.push(entry);

        // Keep only the most recent RADAR_MAX_ENTRIES
        if entries.len() > RADAR_MAX_ENTRIES {
            let drain_count = entries.len() - RADAR_MAX_ENTRIES;
            entries.drain(0..drain_count);
        }

        if let Ok(json) = serde_json::to_string(&entries) {
            let tmp = artifact_path(format!("{}.tmp", POOL_RADAR_FILE));
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, radar_path);
            }
        }
    }

    fn write_pool_decision_minimal(
        &self,
        pool: &CachedPool,
        decision: &str,
        stage: &str,
        reason: impl Into<String>,
    ) {
        self.write_pool_decision(
            pool,
            decision,
            stage,
            reason,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            pool.pumpfun_score,
            0.0,
            self.high_speed_mode.load(Ordering::Relaxed),
            false,
            0.0,
            0,
            0.0,
            "",
            0.0,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn write_pool_decision(
        &self,
        pool: &CachedPool,
        decision: &str,
        stage: &str,
        reason: impl Into<String>,
        pool_size_sol: f64,
        pool_age_secs: f64,
        velocity_sol_per_sec: f64,
        buy_pressure_ratio: f64,
        pool_score: f64,
        pumpfun_score: f64,
        inflow_rate_sol_per_sec: f64,
        high_speed: bool,
        dex_boosted: bool,
        dex_boost_usd: f64,
        social_count: u8,
        effective_min_score: f64,
        dq_action: impl Into<String>,
        dq_confidence: f64,
    ) {
        let now = chrono::Utc::now();
        PoolDecisionEvent {
            timestamp: now,
            mint: pool.base_mint.to_string(),
            pool: pool.id.to_string(),
            quote_mint: pool.quote_mint.to_string(),
            decision: decision.to_string(),
            stage: stage.to_string(),
            reason: reason.into(),
            pool_size_sol,
            pool_age_secs,
            velocity_sol_per_sec,
            buy_pressure_ratio,
            pool_score,
            pumpfun_score,
            inflow_rate_sol_per_sec,
            high_speed,
            dex_boosted,
            dex_boost_usd,
            social_count,
            effective_min_score,
            dq_action: dq_action.into(),
            dq_confidence,
            utc_hour: now.hour() as u8,
        }
        .append_to_file(POOL_DECISIONS_FILE);
    }

    /// Main event handler — called for each event from the listener
    #[allow(clippy::too_many_arguments)]
    fn elite_pool_rejection_reason(
        &self,
        pool_size_sol: f64,
        pool_age_secs: u64,
        historical_velocity_sol_per_sec: f64,
        buy_pressure_ratio: f64,
        pool_score: f64,
        pumpfun_score: f64,
        inflow_rate_sol_per_sec: f64,
    ) -> Option<String> {
        if !self.config.elite_pool_mode {
            return None;
        }

        let mut signals: Vec<&'static str> = Vec::new();
        let live_inflow = inflow_rate_sol_per_sec >= self.config.elite_min_inflow_sol_per_sec;
        let pregrad = pumpfun_score >= self.config.elite_min_pumpfun_score;
        let historical_velocity = pool_age_secs > 0
            && pool_age_secs <= self.config.elite_max_age_secs
            && historical_velocity_sol_per_sec
                >= self.config.elite_min_historical_velocity_sol_per_sec;
        let buy_pressure = buy_pressure_ratio >= self.config.elite_min_buy_pressure_ratio;

        if live_inflow {
            signals.push("live_inflow");
        }
        if pregrad {
            signals.push("pumpfun_pregrad");
        }
        if historical_velocity {
            signals.push("historical_velocity");
        }
        if buy_pressure {
            signals.push("buy_pressure");
        }

        let mut reasons = Vec::new();
        if pool_score < self.config.elite_min_score {
            reasons.push(format!(
                "score={:.1}<elite_min={:.1}",
                pool_score, self.config.elite_min_score
            ));
        }
        if pool_size_sol < self.config.elite_min_pool_size_sol {
            reasons.push(format!(
                "pool_size={:.2}<elite_min={:.2}",
                pool_size_sol, self.config.elite_min_pool_size_sol
            ));
        }
        if self.config.elite_max_pool_size_sol > 0.0
            && pool_size_sol > self.config.elite_max_pool_size_sol
        {
            reasons.push(format!(
                "pool_size={:.2}>elite_max={:.2}",
                pool_size_sol, self.config.elite_max_pool_size_sol
            ));
        }
        if pool_age_secs > self.config.elite_max_age_secs {
            reasons.push(format!(
                "age_secs={}>elite_max={}",
                pool_age_secs, self.config.elite_max_age_secs
            ));
        }

        let min_signals = self.config.elite_min_signal_count.max(1) as usize;
        if signals.len() < min_signals {
            reasons.push(format!("signals={}<min={}", signals.len(), min_signals));
        }
        if self.config.elite_require_live_or_pregrad && !(live_inflow || pregrad) {
            reasons.push("missing_live_or_pregrad_momentum".to_string());
        }

        if reasons.is_empty() {
            None
        } else {
            let signal_text = if signals.is_empty() {
                "none".to_string()
            } else {
                signals.join("|")
            };
            Some(format!("{};signals={}", reasons.join(";"), signal_text))
        }
    }

    pub async fn handle_event(&self, event: ListenerEvent) {
        match event {
            ListenerEvent::NewPool(pool) => {
                self.on_new_pool(pool).await;
            }
            ListenerEvent::WalletUpdate {
                account,
                mint,
                amount,
            } => {
                self.on_wallet_update(account, mint, amount).await;
            }
            ListenerEvent::NewMarket(_) => {}
        }
    }

    async fn on_new_pool(&self, pool: CachedPool) {
        let mint_str = pool.base_mint.to_string();

        // NOTE: We deliberately do NOT dedup against the persisted pool_cache here.
        // pool-cache.json is a SELL-side lookup store (mint→pool) that accumulates
        // every pool ever evaluated and is reloaded (up to 1000 entries) on restart.
        // Using it as a buy gate permanently blocked ~34% of incoming pools across
        // restarts (measured in scematica-pool-decisions.jsonl). Intra-session
        // duplicate pool events are already dropped by the listener's seen_pool_ids
        // set; re-buying a held mint is prevented by max_concurrent_positions +
        // one_token_at_a_time; rapid re-entry is bounded by the mint cooldown.

        // Skip if quote mint doesn't match
        if pool.quote_mint != self.quote_mint {
            debug!(mint = %pool.base_mint, "Skipping pool: wrong quote mint");
            self.write_pool_decision_minimal(&pool, "ignored", "quote_mint", "wrong_quote_mint");
            return;
        }

        // Snipe list check
        if let Some(sl) = &self.snipe_list {
            if !sl.is_listed(&mint_str) {
                debug!(mint = %pool.base_mint, "Skipping: not in snipe list");
                self.write_pool_decision_minimal(&pool, "rejected", "snipe_list", "not_listed");
                return;
            }
        }

        // one_token_at_a_time gate is enforced atomically right before the buy.
        // Filters still run on every pool so we keep stats current.

        // Skip all buys when sell mode or dump mode is active. Emit a visible log
        // line at most once every 30 s so the operator sees the bot is alive and
        // why it's not buying — without flooding the dashboard for every Raydium
        // program notification.
        let sell_on = self.sell_mode.load(Ordering::Relaxed);
        let dump_on = self.dump_mode.load(Ordering::Relaxed);
        if sell_on || dump_on {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let last = self.skip_log_throttle_secs.load(Ordering::Relaxed);
            if now_secs.saturating_sub(last) >= 30 {
                if self
                    .skip_log_throttle_secs
                    .compare_exchange(last, now_secs, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    let mode = if dump_on { "DUMP" } else { "SELL" };
                    let open = self.open_positions.load(Ordering::Relaxed);
                    info!(
                        mode,
                        open_positions = open,
                        "Pools being skipped: {} mode active (buy-limit sell mode auto-clears when all {} positions close; for dashboard/drawdown-triggered mode press [b] to clear)",
                        mode,
                        open,
                    );
                }
            }
            self.write_pool_decision_minimal(
                &pool,
                "ignored",
                "operator_mode",
                if dump_on { "dump_mode" } else { "sell_mode" },
            );
            return;
        }

        // Buy limit
        if self.config.max_buys > 0 {
            let count = self.buy_count.load(Ordering::Relaxed);
            if count >= self.config.max_buys {
                warn!(mint = %pool.base_mint, count, limit = self.config.max_buys, "Buy limit reached — sell mode should be active");
                self.write_pool_decision_minimal(&pool, "ignored", "buy_limit", "max_buys_reached");
                return;
            }
        }

        // Max concurrent positions
        if self.config.max_concurrent_positions > 0 {
            let open = self.open_positions.load(Ordering::Relaxed);
            if open >= self.config.max_concurrent_positions {
                debug!(mint = %pool.base_mint, open, "Max concurrent positions reached — skipping");
                self.write_pool_decision_minimal(
                    &pool,
                    "ignored",
                    "exposure",
                    "max_concurrent_positions",
                );
                return;
            }
        }

        // Loss cooldown: removed by operator decision — the bot keeps running through
        // losses regardless of the consecutive-loss counter. We still TRACK consecutive
        // losses (used by the reputation ledger + dashboard streak display) but never
        // gate buys on them. The `cooldown_after_losses` and `cooldown_minutes` config
        // fields are retained for back-compat parsing but have no effect.

        // Daily loss limit
        if self.config.daily_loss_limit_sol > 0.0 {
            let loss_lamports = -*self.daily_pnl_lamports.lock();
            let loss_sol = loss_lamports as f64 / 1e9;
            if loss_sol >= self.config.daily_loss_limit_sol {
                warn!(mint = %pool.base_mint, loss_sol, "Daily loss limit reached — skipping buy");
                self.write_pool_decision_minimal(&pool, "ignored", "risk", "daily_loss_limit");
                return;
            }
        }

        // ── Grief-loss circuit breaker ─────────────────────────────────────────
        if let Some(gb) = &self.grief_breaker {
            if gb.is_tripped() {
                warn!(
                    mint = %pool.base_mint,
                    window_loss_sol = gb.window_loss_sol(),
                    "Grief-loss circuit breaker tripped — skipping buy"
                );
                self.write_pool_decision_minimal(&pool, "ignored", "risk", "grief_breaker");
                return;
            }
        }

        // ── ATH drawdown guard ─────────────────────────────────────────────────
        if self.ath_drawdown_pct > 0.0 {
            let current = self
                .rpc
                .get_balance(&self.wallet.pubkey())
                .await
                .unwrap_or(0);
            let dd = self.ath_tracker.drawdown_pct(current);
            if dd >= self.ath_drawdown_pct {
                warn!(
                    mint = %pool.base_mint,
                    drawdown_pct = %format!("{:.1}%", dd),
                    threshold_pct = %format!("{:.1}%", self.ath_drawdown_pct),
                    "ATH drawdown limit reached — skipping buy"
                );
                self.write_pool_decision_minimal(&pool, "ignored", "risk", "ath_drawdown");
                return;
            }
        }

        // Session heat cooldown: if too many losses tripped the heat gate, pause buys
        if self.config.session_heat_losses > 0 {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let until_ms = self.cooldown_until_ms.load(Ordering::Relaxed);
            if until_ms > now_ms {
                let remaining_secs = (until_ms - now_ms) / 1000;
                debug!(mint = %pool.base_mint, remaining_secs, "Session heat cooldown active — skipping buy");
                self.write_pool_decision_minimal(&pool, "ignored", "risk", "session_heat_cooldown");
                return;
            }
        }

        info!(mint = %pool.base_mint, pool = %pool.id, "New pool detected — evaluating");

        // Cache the pool
        self.pool_cache.save(&pool.id.to_string(), pool.clone());

        // Skip pools whose open_time is more than 60 seconds in the future.
        // Raydium rejects swaps before pool_open_time — attempting anyway wastes RPC calls.
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if pool.open_time > now_secs + 60 {
            info!(
                mint = %pool.base_mint,
                open_time = pool.open_time,
                now = now_secs,
                "Pool not yet open — skipping"
            );
            self.write_pool_decision_minimal(&pool, "rejected", "pool_age", "not_open_yet");
            return;
        }

        // Skip pools that opened more than 5 minutes ago — early pump is likely over.
        // open_time == 0 means unknown, so we allow those through.
        if pool.open_time > 0 && now_secs > pool.open_time + 300 {
            let age_secs = now_secs - pool.open_time;
            info!(
                mint = %pool.base_mint,
                age_secs,
                "Pool is stale (>5 min old) — skipping to avoid buying the top"
            );
            self.write_pool_decision_minimal(&pool, "rejected", "pool_age", "stale_pool");
            return;
        }

        // Apply buy delay
        if self.config.buy_delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(self.config.buy_delay_ms)).await;
        }

        // Fetch pool reserves UPFRONT (both vaults) — used for radar, AI scoring,
        // pool-scorer, and buy min_out calculation. Fetching once here avoids 3
        // duplicate RPC calls further down the hot path.
        let (upfront_pool_size_lamports, upfront_base_vault_lamports) = {
            let (qv, bv) = tokio::join!(
                tokio::time::timeout(
                    tokio::time::Duration::from_secs(2),
                    self.rpc.get_token_account_balance(&pool.quote_vault),
                ),
                tokio::time::timeout(
                    tokio::time::Duration::from_secs(2),
                    self.rpc.get_token_account_balance(&pool.base_vault),
                ),
            );
            let q = qv
                .ok()
                .and_then(|r| r.ok())
                .and_then(|b| b.amount.parse::<u64>().ok())
                .unwrap_or(0);
            let b = bv
                .ok()
                .and_then(|r| r.ok())
                .and_then(|b| b.amount.parse::<u64>().ok())
                .unwrap_or(0);
            (q, b)
        };
        let reserve_snapshot_at = std::time::Instant::now();
        let upfront_pool_size_sol = upfront_pool_size_lamports as f64 / 1e9;
        let detected_at_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let pool_age_secs = if pool.open_time > 0 && pool.open_time <= detected_at_secs {
            detected_at_secs.saturating_sub(pool.open_time)
        } else {
            0
        };
        let historical_velocity_sol_per_sec = if pool_age_secs > 0 {
            upfront_pool_size_sol / pool_age_secs as f64
        } else {
            0.0
        };
        let buy_pressure_ratio = if upfront_base_vault_lamports > 0 {
            upfront_pool_size_lamports as f64 / upfront_base_vault_lamports as f64
        } else {
            0.0
        };
        let upfront_score = PoolScorer::score(
            &pool,
            upfront_pool_size_lamports,
            upfront_base_vault_lamports,
            detected_at_secs,
        );
        let mut entry_score = upfront_score;
        let mut selected_score_floor = self.config.min_pool_score;

        // High-speed mode: skip filters, AI, scorer — go straight to buy. The operator
        // has explicitly opted in to extra rugs / failed buys / 429s in exchange for
        // entry latency. We still respect the listener's open_time gate above.
        let high_speed = self.high_speed_mode.load(Ordering::Relaxed);
        if high_speed && self.config.elite_pool_mode {
            info!(mint = %pool.base_mint, "HIGH-SPEED: bypassing standard filters/AI, elite pool gate still enforced");
        } else if high_speed {
            if self.config.require_moonshot_confirmation {
                info!(mint = %pool.base_mint, "⚡ HIGH-SPEED — bypassing standard filters/AI, moonshot gate still enforced");
            } else {
                info!(mint = %pool.base_mint, "⚡ HIGH-SPEED — bypassing filters/AI/scorer");
            }
        }

        // Run filters (unless snipe list mode or high-speed mode) — hard cap at 25s
        // so a hung RPC node can't stall this evaluation task and starve the pool stream.
        if !high_speed && self.snipe_list.is_none() {
            let filter_result = tokio::time::timeout(
                tokio::time::Duration::from_secs(25),
                self.filter_pipeline.execute(&pool),
            )
            .await;
            match filter_result {
                Ok(true) => {}
                Ok(false) => {
                    info!(mint = %pool.base_mint, "Pool rejected by filters");
                    self.write_radar_entry(&pool, upfront_pool_size_sol, false, upfront_score);
                    self.write_pool_decision(
                        &pool,
                        "rejected",
                        "filters",
                        "filter_rejected",
                        upfront_pool_size_sol,
                        pool_age_secs as f64,
                        historical_velocity_sol_per_sec,
                        buy_pressure_ratio,
                        upfront_score,
                        pool.pumpfun_score,
                        0.0,
                        high_speed,
                        false,
                        0.0,
                        0,
                        selected_score_floor,
                        "",
                        0.0,
                    );
                    return;
                }
                Err(_) => {
                    warn!(mint = %pool.base_mint, "Filter pipeline timed out (25s) — skipping pool");
                    self.write_radar_entry(&pool, upfront_pool_size_sol, false, upfront_score);
                    self.write_pool_decision(
                        &pool,
                        "rejected",
                        "filters",
                        "filter_timeout",
                        upfront_pool_size_sol,
                        pool_age_secs as f64,
                        historical_velocity_sol_per_sec,
                        buy_pressure_ratio,
                        upfront_score,
                        pool.pumpfun_score,
                        0.0,
                        high_speed,
                        false,
                        0.0,
                        0,
                        selected_score_floor,
                        "",
                        0.0,
                    );
                    return;
                }
            }
        }

        // Read social metadata populated by SocialLinksFilter during filter execution.
        // Provides real name/symbol for AI and social_count for pool scorer enrichment.
        let (token_name, token_symbol, social_count) = {
            let key = pool.base_mint.to_string();
            self.filter_pipeline
                .metadata
                .get(&key)
                .map(|m| (m.name.clone(), m.symbol.clone(), m.social_count))
                .unwrap_or_default()
        };
        let display_name = if token_name.is_empty() {
            "UNKNOWN".to_string()
        } else {
            token_name
        };
        let display_symbol = if token_symbol.is_empty() {
            "UNKNOWN".to_string()
        } else {
            token_symbol
        };

        // ── DexScreener paid-boost check ───────────────────────────────────────
        // A non-zero boostAmount means the team has purchased DexScreener advertising.
        // This is a paid, verified marketing signal: the team spent real money, DexScreener
        // visitors are being directed to the token, and rug probability drops sharply.
        // Moonshot mode still requires live/pre-graduation momentum confirmation.
        let (dex_boosted, dex_boost_usd) = if !high_speed {
            match self
                .dex_cache
                .check_boost(&pool.base_mint.to_string())
                .await
            {
                Some(usd) => (true, usd),
                None => (false, 0.0),
            }
        } else {
            (false, 0.0)
        };
        if dex_boosted {
            info!(
                mint = %pool.base_mint,
                name = %display_name,
                symbol = %display_symbol,
                boost_usd = %format!("${:.0}", dex_boost_usd),
                "🚀 DEXSCREENER PAID BOOST — marketing signal detected"
            );
        }
        let pregrad_moonshot = pool.pumpfun_score >= self.config.moonshot_min_pumpfun_score;
        let historical_moonshot = pool_age_secs > 0
            && pool_age_secs <= 20
            && historical_velocity_sol_per_sec
                >= self.config.moonshot_min_historical_velocity_sol_per_sec;

        // AI risk assessment (if available, and not high-speed)
        if !high_speed {
            if let Some(ai) = &self.ai {
                let pool_size_sol = scematica_core::token::raw_to_ui(
                    upfront_pool_size_lamports,
                    pool.quote_decimals,
                );
                let open_hour = (pool.open_time % 86400 / 3600) as u8;

                // Compute quantitative AMM signals to pass to the AI — these are the
                // real predictors of profitability, not just binary safety flags.
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let pool_age_secs = if pool.open_time > 0 && pool.open_time <= now_secs {
                    now_secs.saturating_sub(pool.open_time)
                } else {
                    0
                };
                let velocity_sol_per_sec = if pool.open_time > 0 && pool_age_secs > 0 {
                    pool_size_sol / pool_age_secs as f64
                } else {
                    0.0
                };
                let buy_pressure_ratio = if upfront_base_vault_lamports > 0 {
                    upfront_pool_size_lamports as f64 / upfront_base_vault_lamports as f64
                } else {
                    0.0
                };
                let amm_expected_inflow_pct = if pool_size_sol > 0.0 && velocity_sol_per_sec > 0.0 {
                    (velocity_sol_per_sec * 10.0 / pool_size_sol) * 100.0
                } else {
                    0.0
                };

                // Feed safe values for disabled on-chain filters: the AI should evaluate
                // momentum signals, not re-check things the filter pipeline already handled.
                let risk = ai
                    .risk
                    .score_token_v2(
                        &pool.base_mint.to_string(),
                        &display_name,
                        &display_symbol,
                        pool_size_sol,
                        true,  // mint_renounced (disabled filter — treated as safe)
                        false, // freezable     (disabled filter — treated as safe)
                        true,  // lp_burned     (disabled filter — treated as safe)
                        false, // mutable_metadata
                        self.config.filters.check_socials,
                        open_hour,
                        velocity_sol_per_sec,
                        buy_pressure_ratio,
                        amm_expected_inflow_pct,
                        pool_age_secs,
                    )
                    .await;

                info!(
                    mint = %pool.base_mint,
                    name = %display_name,
                    symbol = %display_symbol,
                    social_count,
                    score = risk.score,
                    recommendation = %risk.recommendation,
                    velocity = %format!("{:.3} SOL/s", velocity_sol_per_sec),
                    pressure = %format!("{:.4}", buy_pressure_ratio),
                    reasoning = %risk.reasoning,
                    "AI risk assessment"
                );

                // Only block on a genuine AI rejection (low score, real flags).
                // When AI infrastructure fails (no key, rate limit, network), the
                // default_pass function returns score=70/buy — we proceed on on-chain filters.
                if !risk.should_buy() {
                    let ai_failed = risk.red_flags.iter().any(|f| {
                        f.contains("AI assessment failed") || f.contains("AI unavailable")
                    });
                    if ai_failed {
                        warn!(
                            mint = %pool.base_mint,
                            "AI infrastructure unavailable — proceeding on on-chain filters"
                        );
                    } else {
                        info!(
                            mint = %pool.base_mint,
                            score = risk.score,
                            flags = ?risk.red_flags,
                            "AI rejected token — skipping buy"
                        );
                        self.filter_pipeline.stats.record_rejection("ai_risk");
                        self.write_pool_decision(
                            &pool,
                            "rejected",
                            "ai_risk",
                            format!("score={:.1};flags={}", risk.score, risk.red_flags.join("|")),
                            upfront_pool_size_sol,
                            pool_age_secs as f64,
                            historical_velocity_sol_per_sec,
                            buy_pressure_ratio,
                            upfront_score,
                            pool.pumpfun_score,
                            0.0,
                            high_speed,
                            dex_boosted,
                            dex_boost_usd,
                            social_count,
                            selected_score_floor,
                            "",
                            0.0,
                        );
                        return;
                    }
                }
            }
        } // close `if let Some(ai)` and the outer `if !high_speed` guard

        // ── Fibonacci entry gate ────────────────────────────────────────────────
        // Evaluate pool against Fibonacci golden ratio patterns. This gate runs BEFORE
        // the pool scorer so we filter out dead pools (thin liquidity, stale age, low
        // velocity) as early as possible. High-speed mode and DexScreener-boosted tokens bypass.
        // Exceptional velocity (≥ φ² = 2.618 SOL/s) also bypasses: these are confirmed runners.
        let exceptional_velocity = pool_age_secs > 0 && historical_velocity_sol_per_sec >= 2.618;
        if !high_speed && !dex_boosted {
            let fib_decision = self.fib_system.evaluate_entry(
                upfront_pool_size_lamports,
                upfront_base_vault_lamports,
                detected_at_secs,
            );

            if !fib_decision.should_enter && !exceptional_velocity {
                info!(
                    mint = %pool.base_mint,
                    score = fib_decision.fibonacci_score,
                    reason = %fib_decision.reason,
                    "❌ Fibonacci gate: REJECTED"
                );
                self.filter_pipeline
                    .stats
                    .record_rejection("fibonacci_gate");
                self.write_radar_entry(&pool, upfront_pool_size_sol, false, upfront_score);
                self.write_pool_decision(
                    &pool,
                    "rejected",
                    "fibonacci_gate",
                    fib_decision.reason.clone(),
                    upfront_pool_size_sol,
                    pool_age_secs as f64,
                    historical_velocity_sol_per_sec,
                    buy_pressure_ratio,
                    upfront_score,
                    pool.pumpfun_score,
                    0.0,
                    high_speed,
                    dex_boosted,
                    dex_boost_usd,
                    social_count,
                    selected_score_floor,
                    "",
                    0.0,
                );
                return;
            }
            if exceptional_velocity && !fib_decision.should_enter {
                info!(
                    mint = %pool.base_mint,
                    score = fib_decision.fibonacci_score,
                    "⚡ Fibonacci gate bypassed — exceptional velocity (≥2.618 SOL/s)"
                );
            }

            info!(
                mint = %pool.base_mint,
                score = fib_decision.fibonacci_score,
                multiplier = fib_decision.position_multiplier,
                expected_tp = fib_decision.expected_tp_pct,
                reason = %fib_decision.reason,
                "✅ Fibonacci gate: PASSED"
            );

            // Record entry for stats tracking
            self.fib_stats
                .lock()
                .record_entry(fib_decision.fibonacci_score);
        }

        // ── Momentum confirmation (before pool scorer) ──────────────────────────
        // Moonshot mode is fail-closed: if a pool is not visibly gaining liquidity
        // now, it needs strong pump.fun pre-graduation momentum or trusted historical
        // AMM velocity to continue.
        let mut live_momentum_confirmed = false;
        let inflow_rate_now_sol_per_sec: f64;
        let should_check_momentum = upfront_pool_size_lamports > 0
            && (self.config.require_moonshot_confirmation
                || self.config.elite_pool_mode
                || (!dex_boosted && !exceptional_velocity));
        if should_check_momentum {
            let vault_now = tokio::time::timeout(
                tokio::time::Duration::from_millis(800),
                self.rpc.get_token_account_balance(&pool.quote_vault),
            )
            .await;
            let (confirmed, rate) = match vault_now {
                Ok(Ok(qb)) => {
                    let current = qb.amount.parse::<u64>().unwrap_or(0);
                    let min_growth_pct = self.config.moonshot_min_growth_pct.max(0.0);
                    let min_growth_by_pct =
                        (upfront_pool_size_lamports as f64 * min_growth_pct / 100.0) as u64;
                    let min_growth_floor =
                        (self.config.moonshot_min_growth_sol.max(0.0) * 1e9) as u64;
                    let min_growth = min_growth_by_pct.max(min_growth_floor).max(1);
                    let growth_lam = current.saturating_sub(upfront_pool_size_lamports);
                    let elapsed_secs = reserve_snapshot_at
                        .elapsed()
                        .as_secs_f64()
                        .clamp(0.20, 30.0);
                    let spot_rate = growth_lam as f64 / 1e9 / elapsed_secs;
                    let growth_sol = growth_lam as f64 / 1e9;
                    let grew = current >= upfront_pool_size_lamports.saturating_add(min_growth);
                    let rate_ok = spot_rate >= self.config.moonshot_min_inflow_sol_per_sec;
                    let live_ok = if self.config.require_moonshot_confirmation {
                        grew && rate_ok
                    } else {
                        grew
                    };
                    if live_ok {
                        info!(
                            mint = %pool.base_mint,
                            growth_sol = %format!("+{:.4}", growth_sol),
                            inflow_rate = %format!("{:.3} SOL/s", spot_rate),
                            elapsed_secs = %format!("{:.2}", elapsed_secs),
                            min_required_sol = %format!("{:.4}", min_growth as f64 / 1e9),
                            "✅ Moonshot momentum confirmed — quote vault is growing"
                        );
                    } else {
                        let drift = current as f64 / 1e9 - upfront_pool_size_sol;
                        info!(
                            mint = %pool.base_mint,
                            drift_sol = %format!("{:.4}", drift),
                            inflow_rate = %format!("{:.3} SOL/s", spot_rate),
                            min_inflow = %format!("{:.3} SOL/s", self.config.moonshot_min_inflow_sol_per_sec),
                            needed_sol = %format!("{:.4}", min_growth as f64 / 1e9),
                            "❌ Moonshot momentum failed — insufficient live buying"
                        );
                        if !(self.config.require_moonshot_confirmation
                            && (pregrad_moonshot || historical_moonshot))
                        {
                            self.filter_pipeline.stats.record_rejection("no_momentum");
                        }
                    }
                    (live_ok, spot_rate)
                }
                _ => {
                    if self.config.require_moonshot_confirmation
                        && !pregrad_moonshot
                        && !historical_moonshot
                    {
                        info!(
                            mint = %pool.base_mint,
                            "Moonshot momentum RPC timeout — skipping because momentum could not be verified"
                        );
                        self.filter_pipeline
                            .stats
                            .record_rejection("momentum_unknown");
                        self.write_pool_decision(
                            &pool,
                            "rejected",
                            "momentum_gate",
                            "momentum_rpc_timeout",
                            upfront_pool_size_sol,
                            pool_age_secs as f64,
                            historical_velocity_sol_per_sec,
                            buy_pressure_ratio,
                            upfront_score,
                            pool.pumpfun_score,
                            0.0,
                            high_speed,
                            dex_boosted,
                            dex_boost_usd,
                            social_count,
                            selected_score_floor,
                            "",
                            0.0,
                        );
                        return;
                    }
                    debug!(mint = %pool.base_mint, "Momentum check RPC timeout — proceeding on alternate moonshot signal");
                    (pregrad_moonshot || historical_moonshot, 0.0)
                }
            };
            if !confirmed
                && !(self.config.require_moonshot_confirmation
                    && (pregrad_moonshot || historical_moonshot))
            {
                self.write_pool_decision(
                    &pool,
                    "rejected",
                    "momentum_gate",
                    format!("inflow_rate={:.3};live_momentum=false", rate),
                    upfront_pool_size_sol,
                    pool_age_secs as f64,
                    historical_velocity_sol_per_sec,
                    buy_pressure_ratio,
                    upfront_score,
                    pool.pumpfun_score,
                    rate,
                    high_speed,
                    dex_boosted,
                    dex_boost_usd,
                    social_count,
                    selected_score_floor,
                    "",
                    0.0,
                );
                return;
            }
            live_momentum_confirmed =
                confirmed && rate >= self.config.moonshot_min_inflow_sol_per_sec;
            inflow_rate_now_sol_per_sec = rate;
        } else {
            inflow_rate_now_sol_per_sec = 0.0;
        }

        // ── Pool predictive scoring (skipped in high-speed mode and for boosted tokens) ──
        // Uses all available signals: historical (size, age, velocity, buy-pressure, EV),
        // pumpfun_score (pre-graduation momentum carried on the pool struct), social_count,
        // and inflow_rate_now (live spot rate computed above).
        if (!high_speed || self.config.require_moonshot_confirmation || self.config.elite_pool_mode)
            && !dex_boosted
            && self.config.min_pool_score > 0.0
        {
            let final_score = crate::pool_scorer::PoolScorer::score_full(
                &pool,
                upfront_pool_size_lamports,
                upfront_base_vault_lamports,
                detected_at_secs,
                social_count,
                inflow_rate_now_sol_per_sec,
            );
            // Pools actively pumping right now get a lower threshold because live
            // momentum is stronger than a stale historical score. In moonshot mode,
            // pools without live inflow must clear the stricter moonshot score floor.
            let effective_min = if self.config.elite_pool_mode {
                self.config.min_pool_score.max(self.config.elite_min_score)
            } else if self.config.require_moonshot_confirmation
                && inflow_rate_now_sol_per_sec < self.config.moonshot_min_inflow_sol_per_sec
            {
                self.config
                    .min_pool_score
                    .max(self.config.moonshot_min_score)
            } else if inflow_rate_now_sol_per_sec >= self.config.moonshot_min_inflow_sol_per_sec {
                (self.config.min_pool_score - 10.0).max(45.0)
            } else {
                self.config.min_pool_score
            };
            selected_score_floor = effective_min;
            if final_score < effective_min {
                info!(
                    mint = %pool.base_mint,
                    score = %format!("{:.1}", final_score),
                    effective_min = %format!("{:.1}", effective_min),
                    inflow_rate = %format!("{:.3} SOL/s", inflow_rate_now_sol_per_sec),
                    pumpfun_score = pool.pumpfun_score,
                    "Pool score too low — skipping buy"
                );
                self.write_radar_entry(&pool, upfront_pool_size_sol, false, final_score);
                self.filter_pipeline.stats.record_rejection("pool_scorer");
                self.write_pool_decision(
                    &pool,
                    "rejected",
                    "pool_scorer",
                    format!("score={:.1}<min={:.1}", final_score, effective_min),
                    upfront_pool_size_sol,
                    pool_age_secs as f64,
                    historical_velocity_sol_per_sec,
                    buy_pressure_ratio,
                    final_score,
                    pool.pumpfun_score,
                    inflow_rate_now_sol_per_sec,
                    high_speed,
                    dex_boosted,
                    dex_boost_usd,
                    social_count,
                    effective_min,
                    "",
                    0.0,
                );
                return;
            }
            entry_score = final_score;
            info!(
                mint = %pool.base_mint,
                score = %format!("{:.1}", final_score),
                effective_min = %format!("{:.1}", effective_min),
                inflow_rate = %format!("{:.3} SOL/s", inflow_rate_now_sol_per_sec),
                pumpfun_score = pool.pumpfun_score,
                social_count,
                name = %display_name,
                symbol = %display_symbol,
                "Pool scorer: passed"
            );
        }

        if self.config.require_moonshot_confirmation {
            let moonshot_score = crate::pool_scorer::PoolScorer::score_full(
                &pool,
                upfront_pool_size_lamports,
                upfront_base_vault_lamports,
                detected_at_secs,
                social_count,
                inflow_rate_now_sol_per_sec,
            );
            if moonshot_score > entry_score {
                entry_score = moonshot_score;
            }

            let has_moonshot_signal =
                live_momentum_confirmed || pregrad_moonshot || historical_moonshot;
            let score_floor = if self.config.elite_pool_mode {
                self.config.min_pool_score.max(self.config.elite_min_score)
            } else if live_momentum_confirmed {
                (self.config.min_pool_score - 10.0).max(45.0)
            } else {
                self.config
                    .min_pool_score
                    .max(self.config.moonshot_min_score)
            };
            selected_score_floor = score_floor;

            if !has_moonshot_signal || entry_score < score_floor {
                info!(
                    mint = %pool.base_mint,
                    score = %format!("{:.1}", entry_score),
                    score_floor = %format!("{:.1}", score_floor),
                    live_inflow = %format!("{:.3} SOL/s", inflow_rate_now_sol_per_sec),
                    historical_velocity = %format!("{:.3} SOL/s", historical_velocity_sol_per_sec),
                    buy_pressure = %format!("{:.6}", buy_pressure_ratio),
                    pumpfun_score = %format!("{:.1}", pool.pumpfun_score),
                    pregrad_moonshot,
                    historical_moonshot,
                    live_momentum_confirmed,
                    "Moonshot gate: skipping pool without enough runner confirmation"
                );
                self.write_radar_entry(&pool, upfront_pool_size_sol, false, entry_score);
                self.filter_pipeline.stats.record_rejection("moonshot_gate");
                self.write_pool_decision(
                    &pool,
                    "rejected",
                    "moonshot_gate",
                    format!(
                        "signal={};score={:.1}<floor={:.1}",
                        has_moonshot_signal, entry_score, score_floor
                    ),
                    upfront_pool_size_sol,
                    pool_age_secs as f64,
                    historical_velocity_sol_per_sec,
                    buy_pressure_ratio,
                    entry_score,
                    pool.pumpfun_score,
                    inflow_rate_now_sol_per_sec,
                    high_speed,
                    dex_boosted,
                    dex_boost_usd,
                    social_count,
                    score_floor,
                    "",
                    0.0,
                );
                return;
            }
        }

        // ── Time-of-day block gate ────────────────────────────────────────────────
        // Completely skip buying during configured UTC hours (e.g. 01:00, 21:00 which
        // show 0% win rate in live data). Distinct from time_of_day_weighting (sizing).
        if self.config.elite_pool_mode {
            let elite_score = crate::pool_scorer::PoolScorer::score_full(
                &pool,
                upfront_pool_size_lamports,
                upfront_base_vault_lamports,
                detected_at_secs,
                social_count,
                inflow_rate_now_sol_per_sec,
            );
            if elite_score > entry_score {
                entry_score = elite_score;
            }
            selected_score_floor = selected_score_floor.max(self.config.elite_min_score);

            if let Some(reason) = self.elite_pool_rejection_reason(
                upfront_pool_size_sol,
                pool_age_secs,
                historical_velocity_sol_per_sec,
                buy_pressure_ratio,
                entry_score,
                pool.pumpfun_score,
                inflow_rate_now_sol_per_sec,
            ) {
                info!(
                    mint = %pool.base_mint,
                    score = %format!("{:.1}", entry_score),
                    live_inflow = %format!("{:.3} SOL/s", inflow_rate_now_sol_per_sec),
                    historical_velocity = %format!("{:.3} SOL/s", historical_velocity_sol_per_sec),
                    buy_pressure = %format!("{:.6}", buy_pressure_ratio),
                    pumpfun_score = %format!("{:.1}", pool.pumpfun_score),
                    reason = %reason,
                    "Elite pool gate: rejected"
                );
                self.write_radar_entry(&pool, upfront_pool_size_sol, false, entry_score);
                self.filter_pipeline
                    .stats
                    .record_rejection("elite_pool_gate");
                self.write_pool_decision(
                    &pool,
                    "rejected",
                    "elite_pool_gate",
                    reason,
                    upfront_pool_size_sol,
                    pool_age_secs as f64,
                    historical_velocity_sol_per_sec,
                    buy_pressure_ratio,
                    entry_score,
                    pool.pumpfun_score,
                    inflow_rate_now_sol_per_sec,
                    high_speed,
                    dex_boosted,
                    dex_boost_usd,
                    social_count,
                    selected_score_floor,
                    "",
                    0.0,
                );
                return;
            }

            info!(
                mint = %pool.base_mint,
                score = %format!("{:.1}", entry_score),
                live_inflow = %format!("{:.3} SOL/s", inflow_rate_now_sol_per_sec),
                historical_velocity = %format!("{:.3} SOL/s", historical_velocity_sol_per_sec),
                pumpfun_score = %format!("{:.1}", pool.pumpfun_score),
                "Elite pool gate: passed"
            );
        }

        if !self.config.blocked_hours_utc.is_empty() {
            let utc_hour = chrono::Utc::now().hour() as u8;
            if self.config.blocked_hours_utc.contains(&utc_hour) {
                info!(
                    mint = %pool.base_mint,
                    utc_hour,
                    "Time gate: blocked hour — skipping buy"
                );
                self.filter_pipeline.stats.record_rejection("time_gate");
                self.write_pool_decision(
                    &pool,
                    "rejected",
                    "time_gate",
                    format!("blocked_hour_utc={}", utc_hour),
                    upfront_pool_size_sol,
                    pool_age_secs as f64,
                    historical_velocity_sol_per_sec,
                    buy_pressure_ratio,
                    entry_score,
                    pool.pumpfun_score,
                    inflow_rate_now_sol_per_sec,
                    high_speed,
                    dex_boosted,
                    dex_boost_usd,
                    social_count,
                    selected_score_floor,
                    "",
                    0.0,
                );
                return;
            }
        }

        // ── Regime gate: in bear/panic, only buy exceptional-velocity or high-score pools ──
        if !high_speed && !dex_boosted {
            let regime = self.live_params.read().market_regime.clone();
            if regime == "bear" || regime == "panic" {
                let min_regime_score = 65.0;
                if entry_score < min_regime_score && !exceptional_velocity {
                    info!(
                        mint = %pool.base_mint,
                        regime = %regime,
                        score = %format!("{:.1}", entry_score),
                        "Regime gate: bear/panic — skipping low-score pool"
                    );
                    self.filter_pipeline.stats.record_rejection("regime_gate");
                    self.write_pool_decision(
                        &pool,
                        "rejected",
                        "regime_gate",
                        format!(
                            "regime={};score={:.1}<min={:.1}",
                            regime, entry_score, min_regime_score
                        ),
                        upfront_pool_size_sol,
                        pool_age_secs as f64,
                        historical_velocity_sol_per_sec,
                        buy_pressure_ratio,
                        entry_score,
                        pool.pumpfun_score,
                        inflow_rate_now_sol_per_sec,
                        high_speed,
                        dex_boosted,
                        dex_boost_usd,
                        social_count,
                        min_regime_score,
                        "",
                        0.0,
                    );
                    return;
                }
            }
        }

        // Track last pool time for gas war burst detection
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.gas_war_last_pool_ms.store(now_ms, Ordering::Relaxed);

        // Write pool radar entry — pool passed all checks and is about to be bought
        self.write_radar_entry(&pool, upfront_pool_size_sol, true, entry_score);
        self.write_pool_decision(
            &pool,
            "accepted",
            "buy_ready",
            "passed_all_gates",
            upfront_pool_size_sol,
            pool_age_secs as f64,
            historical_velocity_sol_per_sec,
            buy_pressure_ratio,
            entry_score,
            pool.pumpfun_score,
            inflow_rate_now_sol_per_sec,
            high_speed,
            dex_boosted,
            dex_boost_usd,
            social_count,
            selected_score_floor,
            "",
            0.0,
        );

        // Execute buy — pass upfront reserves so buy() can compute a real min_out
        // without another RPC round-trip.
        if let Err(e) = self
            .buy(
                &pool,
                upfront_pool_size_lamports,
                upfront_base_vault_lamports,
                entry_score,
                inflow_rate_now_sol_per_sec,
                detected_at_secs,
            )
            .await
        {
            error!(mint = %pool.base_mint, "buy() error: {}", e);
        }
    }

    async fn buy(
        &self,
        pool: &CachedPool,
        quote_reserve_lam: u64,
        base_reserve_lam: u64,
        upfront_score: f64,
        inflow_rate_now_sol_per_sec: f64,
        detected_at_secs: u64,
    ) -> Result<()> {
        // ── POOL SIZE VALIDATION: Enforce min/max pool size at buy time ──
        // The filter gate uses upfront data; we re-validate here to catch stale/drained pools
        let pool_size_sol = quote_reserve_lam as f64 / 1e9;

        // ── Pool metadata for TradeEvent feedback loop ──
        let pool_age_secs_at_buy = if pool.open_time > 0 && detected_at_secs >= pool.open_time {
            (detected_at_secs - pool.open_time) as f64
        } else {
            0.0
        };
        let velocity_sol_per_sec_at_buy = if pool_age_secs_at_buy > 0.0 {
            pool_size_sol / pool_age_secs_at_buy
        } else {
            0.0
        };
        let buy_pressure_ratio_at_buy = if base_reserve_lam > 0 {
            quote_reserve_lam as f64 / base_reserve_lam as f64
        } else {
            0.0
        };
        let mut dq_action = String::new();
        let mut dq_confidence = 0.0;
        let mut high_speed = self.high_speed_mode.load(Ordering::Relaxed);

        if pool_size_sol < self.config.filters.min_pool_size {
            info!(
                mint = %pool.base_mint,
                pool_size_sol = %format!("{:.2}", pool_size_sol),
                min_required = %format!("{:.2}", self.config.filters.min_pool_size),
                "Pool size too small at buy time — SKIPPING (filter gate passed but pool drained)"
            );
            self.write_pool_decision(
                pool,
                "rejected",
                "buy_pool_size",
                format!(
                    "pool_size={:.4}<min={:.4}",
                    pool_size_sol, self.config.filters.min_pool_size
                ),
                pool_size_sol,
                pool_age_secs_at_buy,
                velocity_sol_per_sec_at_buy,
                buy_pressure_ratio_at_buy,
                upfront_score,
                pool.pumpfun_score,
                inflow_rate_now_sol_per_sec,
                high_speed,
                false,
                0.0,
                0,
                self.config.min_pool_score,
                dq_action.as_str(),
                dq_confidence,
            );
            return Ok(());
        }

        if self.config.filters.max_pool_size > 0.0
            && pool_size_sol > self.config.filters.max_pool_size
        {
            info!(
                mint = %pool.base_mint,
                pool_size_sol = %format!("{:.2}", pool_size_sol),
                max_allowed = %format!("{:.2}", self.config.filters.max_pool_size),
                "Pool size too large at buy time — SKIPPING (whale influence)"
            );
            self.write_pool_decision(
                pool,
                "rejected",
                "buy_pool_size",
                format!(
                    "pool_size={:.4}>max={:.4}",
                    pool_size_sol, self.config.filters.max_pool_size
                ),
                pool_size_sol,
                pool_age_secs_at_buy,
                velocity_sol_per_sec_at_buy,
                buy_pressure_ratio_at_buy,
                upfront_score,
                pool.pumpfun_score,
                inflow_rate_now_sol_per_sec,
                high_speed,
                false,
                0.0,
                0,
                self.config.min_pool_score,
                dq_action.as_str(),
                dq_confidence,
            );
            return Ok(());
        }

        // ── Fetch wallet balance early — needed for wallet-pct sizing and reserve check ──
        let wallet_pubkey_early = self.wallet.pubkey();
        let native_balance_early = self
            .rpc
            .get_balance(&wallet_pubkey_early)
            .await
            .unwrap_or(0);

        // ── Compute effective quote amount ─────────────────────────────────────
        // When wallet_pct > 0: position = wallet × wallet_pct%, floored at the
        // mode's quote_amount so small wallets still use the intended base size.
        // This means wallet_pct scales UP as the wallet grows past the base, but
        // never drops BELOW the mode's base (e.g. Balanced never goes under 0.01 SOL).
        // When wallet_pct == 0: use the fixed quote_amount_mode.
        let mode_quote_amount_raw = {
            let lp = self.live_params.read();
            let base_raw = ui_to_raw(lp.quote_amount_mode, self.quote_decimals).max(1_000_000);
            if lp.wallet_pct > 0.0 {
                let pct_raw = (native_balance_early as f64 * lp.wallet_pct / 100.0) as u64;
                pct_raw.max(base_raw) // scale up via wallet%, but floor at mode's base amount
            } else {
                base_raw
            }
        };
        let mut effective_quote_amount_raw = mode_quote_amount_raw;

        // Apply live_params amount_multiplier (strategy agent / rate mode)
        {
            let lp = self.live_params.read();
            if (lp.amount_multiplier - 1.0).abs() > 0.01 {
                effective_quote_amount_raw =
                    (effective_quote_amount_raw as f64 * lp.amount_multiplier) as u64;
            }
        }

        // Apply time-of-day weighting
        if self.config.time_of_day_weighting {
            let tod_mult = DayWeighter::current_multiplier();
            effective_quote_amount_raw = (effective_quote_amount_raw as f64 * tod_mult) as u64;
        }

        // Apply progressive scaling (Super Builder mode): as the wallet grows
        // toward the target, scale position size up linearly. At 0% progress no
        // bonus; at 100% progress, position is 2.5× base. Capped at 2.5× so the
        // bot doesn't blow through its remaining capital once it crosses target.
        if self.progressive_scaling.load(Ordering::Relaxed) {
            let override_lam = self.wallet_target_lamports_override.load(Ordering::Relaxed);
            let target_lam = if override_lam > 0 {
                override_lam
            } else {
                (self.config.wallet_target_sol * 1e9) as u64
            };
            if target_lam > 0 {
                let start_lam = self.session_start_lamports.load(Ordering::Relaxed) as i64;
                let daily_pnl = *self.daily_pnl_lamports.lock();
                let approx_wallet = (start_lam + daily_pnl).max(0) as u64;
                let progress = (approx_wallet as f64 / target_lam as f64).clamp(0.0, 1.0);
                let prog_mult = 1.0 + 1.5 * progress;
                effective_quote_amount_raw = (effective_quote_amount_raw as f64 * prog_mult) as u64;
                if (prog_mult - 1.0).abs() > 0.05 {
                    info!(
                        mint = %pool.base_mint,
                        progress_pct = %format!("{:.0}%", progress * 100.0),
                        prog_multiplier = %format!("{:.2}x", prog_mult),
                        "Super Builder progressive scaling applied"
                    );
                }
            }
        }

        // Apply Kelly sizing
        if let Some(ref kelly) = self.kelly_sizer {
            let history = self.trade_history.lock().clone();
            let lookback_history: Vec<(bool, f64)> = history
                .iter()
                .rev()
                .take(self.config.kelly_lookback)
                .copied()
                .collect();
            let kelly_mult = kelly.compute_multiplier(&lookback_history);
            effective_quote_amount_raw = (effective_quote_amount_raw as f64 * kelly_mult) as u64;
            if (kelly_mult - 1.0).abs() > 0.05 {
                info!(
                    mint = %pool.base_mint,
                    kelly_multiplier = %format!("{:.2}x", kelly_mult),
                    "Kelly sizing applied"
                );
            }
        }

        if let Some(agent) = &self.nn_agent {
            let nn_state = self.build_nn_entry_state(
                pool_size_sol,
                pool_age_secs_at_buy,
                velocity_sol_per_sec_at_buy,
                buy_pressure_ratio_at_buy,
                upfront_score,
                inflow_rate_now_sol_per_sec,
                native_balance_early,
            );
            let advice = if let Ok(mut ag) = agent.lock() {
                let ready = ag.ready_to_advise();
                let stats = ag.stats();
                let (action, q_vals) = ag.advise(&nn_state);
                ag.write_explanation(&nn_state, &artifact_path_string(NN_ADVICE_FILE));
                Some((
                    action,
                    q_vals,
                    ready,
                    stats.epsilon,
                    stats.replay_size,
                    stats.train_steps,
                ))
            } else {
                None
            };

            if let Some((action, q_vals, ready, epsilon, replay_size, train_steps)) = advice {
                dq_action = action.label().to_string();
                dq_confidence = q_vals
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .map(f64::abs)
                    .fold(0.0, f64::max);
                let has_signal = q_vals.iter().any(|v| v.is_finite() && v.abs() > 1e-9);
                if !has_signal {
                    debug!(
                        mint = %pool.base_mint,
                        action = action.label(),
                        "DQ*: advice produced zero Q-signal; leaving size unchanged"
                    );
                } else if !ready {
                    info!(
                        mint = %pool.base_mint,
                        action = action.label(),
                        epsilon,
                        replay_size,
                        train_steps,
                        "DQ*: warm-up advice recorded; not enforcing yet"
                    );
                } else {
                    match action {
                        TradeAction::BuyAggressive => {
                            effective_quote_amount_raw =
                                (effective_quote_amount_raw as f64 * 1.5) as u64;
                            info!(
                                mint = %pool.base_mint,
                                epsilon,
                                replay_size,
                                train_steps,
                                "DQ*: BuyAggressive advice - sizing up 1.5x"
                            );
                        }
                        TradeAction::BuyStandard => {
                            info!(
                                mint = %pool.base_mint,
                                epsilon,
                                replay_size,
                                train_steps,
                                "DQ*: BuyStandard advice - keeping base size"
                            );
                        }
                        TradeAction::Hold => {
                            effective_quote_amount_raw =
                                (effective_quote_amount_raw as f64 * 0.5) as u64;
                            info!(
                                mint = %pool.base_mint,
                                epsilon,
                                replay_size,
                                train_steps,
                                "DQ*: Hold advice - sizing down 0.5x"
                            );
                        }
                        TradeAction::SellPartial | TradeAction::SellAll => {
                            // Veto guard: only let a bearish signal *fully suppress* a buy when
                            // it is meaningfully separated from the best buy alternative. The net
                            // can be only partially converged (high avg_loss) when train_steps
                            // first crosses the enforcement gate, and the validated edge is
                            // PF≈6.5 — a marginal SellPartial that barely outranks BuyStandard/
                            // BuyAggressive must not silently kill entries. A weak bearish lean
                            // downgrades to size-down (0.5x) instead of a hard skip.
                            //
                            // Require: the bearish Q is genuinely positive AND exceeds the best
                            // buy Q by ≥15% (relative), or the best buy Q is itself non-positive.
                            const NN_VETO_REL_MARGIN: f64 = 0.15;
                            let buy_q = q_vals
                                .get(TradeAction::BuyStandard.index())
                                .copied()
                                .unwrap_or(f64::NEG_INFINITY)
                                .max(
                                    q_vals
                                        .get(TradeAction::BuyAggressive.index())
                                        .copied()
                                        .unwrap_or(f64::NEG_INFINITY),
                                );
                            let sell_q = q_vals.get(action.index()).copied().unwrap_or(0.0);
                            let strong_veto = sell_q > 0.0
                                && (buy_q <= 0.0 || sell_q >= buy_q * (1.0 + NN_VETO_REL_MARGIN));

                            if !strong_veto {
                                effective_quote_amount_raw =
                                    (effective_quote_amount_raw as f64 * 0.5) as u64;
                                info!(
                                    mint = %pool.base_mint,
                                    action = action.label(),
                                    sell_q,
                                    buy_q,
                                    epsilon,
                                    replay_size,
                                    train_steps,
                                    "DQ*: weak bearish lean (sell_q within margin of buy_q) - sizing down 0.5x instead of skipping"
                                );
                            } else {
                                info!(
                                    mint = %pool.base_mint,
                                    action = action.label(),
                                    sell_q,
                                    buy_q,
                                    epsilon,
                                    replay_size,
                                    train_steps,
                                    "DQ*: strong exit/bearish advice - skipping buy"
                                );
                                self.write_pool_decision(
                                    pool,
                                    "rejected",
                                    "dq_advice",
                                    format!("action={};confidence={:.4}", dq_action, dq_confidence),
                                    pool_size_sol,
                                    pool_age_secs_at_buy,
                                    velocity_sol_per_sec_at_buy,
                                    buy_pressure_ratio_at_buy,
                                    upfront_score,
                                    pool.pumpfun_score,
                                    inflow_rate_now_sol_per_sec,
                                    high_speed,
                                    false,
                                    0.0,
                                    0,
                                    self.config.min_pool_score,
                                    dq_action.as_str(),
                                    dq_confidence,
                                );
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }

        // Fibonacci position sizing: scale by entry decision confidence
        high_speed = self.high_speed_mode.load(Ordering::Relaxed);
        let detected_at_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if !high_speed {
            let fib_decision = self.fib_system.evaluate_entry(
                quote_reserve_lam,
                base_reserve_lam,
                detected_at_secs,
            );
            if fib_decision.position_multiplier != 1.0 {
                effective_quote_amount_raw =
                    (effective_quote_amount_raw as f64 * fib_decision.position_multiplier) as u64;
                info!(
                    mint = %pool.base_mint,
                    fibonacci_score = fib_decision.fibonacci_score,
                    fibonacci_multiplier = fib_decision.position_multiplier,
                    "Fibonacci position sizing applied"
                );
            }
        }

        // Pool quality scaling: reduce position size on lower-quality pools
        if self.config.pool_quality_sizing && upfront_score > 0.0 {
            let quality_mult = (upfront_score / 100.0).clamp(0.1, 1.0);
            effective_quote_amount_raw = (effective_quote_amount_raw as f64 * quality_mult) as u64;
            if quality_mult < 0.95 {
                info!(
                    mint = %pool.base_mint,
                    pool_score = %format!("{:.1}", upfront_score),
                    quality_multiplier = %format!("{:.2}x", quality_mult),
                    "Pool quality sizing applied"
                );
            }
        }

        // Gas war: log escalation intent when pools arrive in rapid burst
        if self.config.gas_war_mode {
            let last_ms = self.gas_war_last_pool_ms.load(Ordering::Relaxed);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let gap_ms = now_ms.saturating_sub(last_ms);
            if gap_ms < 2000 && last_ms > 0 {
                info!(
                    mint = %pool.base_mint,
                    gap_ms,
                    max_cu_price = self.config.gas_war_max_cu_price,
                    "Gas war mode: rapid burst detected — escalating CU price"
                );
            }
        }

        info!(
            mint = %pool.base_mint,
            amount_sol = effective_quote_amount_raw as f64 / 1e9,
            quote = %self.config.quote_mint,
            "Buy evaluation started"
        );

        // Dedup: multiple listener sources (pumpfun, whale_copy, raydium listener) can
        // all fire for the same pool creation within milliseconds. Without this guard
        // the bot buys the same mint 2-3× before the processing_lock catches it.
        // Timestamps are unix seconds so the map persists across restarts — prevents
        // re-entering the same losing pool after a dashboard/sniper restart.
        {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let cooldown = self.config.mint_cooldown_secs;
            if let Some(entry) = self.recently_bought.get(&pool.base_mint) {
                if now_secs.saturating_sub(*entry) < cooldown {
                    info!(
                        mint = %pool.base_mint,
                        cooldown_remaining_mins = (cooldown.saturating_sub(now_secs.saturating_sub(*entry))) / 60,
                        "Dedup: mint purchased within cooldown window — skipping"
                    );
                    self.write_pool_decision(
                        pool,
                        "ignored",
                        "dedup",
                        "mint_cooldown",
                        pool_size_sol,
                        pool_age_secs_at_buy,
                        velocity_sol_per_sec_at_buy,
                        buy_pressure_ratio_at_buy,
                        upfront_score,
                        pool.pumpfun_score,
                        inflow_rate_now_sol_per_sec,
                        high_speed,
                        false,
                        0.0,
                        0,
                        self.config.min_pool_score,
                        dq_action.as_str(),
                        dq_confidence,
                    );
                    return Ok(());
                }
            }
            // Prune expired entries so the map doesn't grow unbounded during long sessions.
            self.recently_bought
                .retain(|_, ts| now_secs.saturating_sub(*ts) < cooldown);
        }

        let wallet_pubkey = wallet_pubkey_early; // reuse — same key, already fetched above
        let quote_ata = get_ata(&wallet_pubkey, &self.quote_mint);
        let base_ata = get_ata(&wallet_pubkey, &pool.base_mint);

        // Gate: ensure we have enough native SOL for quote amount + fees + ATA rent + reserve floor.
        // Reuse native_balance_early — fetched moments ago for wallet-pct sizing, still fresh.
        let native_balance = native_balance_early;
        let reserve_lam = ((self.config.min_sol_reserve * 1e9) as u64).max(6_000_000);
        let min_required = effective_quote_amount_raw + reserve_lam;
        if native_balance < min_required {
            warn!(
                mint = %pool.base_mint,
                balance_sol = native_balance as f64 / 1e9,
                required_sol = min_required as f64 / 1e9,
                "Insufficient SOL — skipping buy"
            );
            self.write_pool_decision(
                pool,
                "ignored",
                "wallet_balance",
                format!(
                    "balance_sol={:.4}<required_sol={:.4}",
                    native_balance as f64 / 1e9,
                    min_required as f64 / 1e9
                ),
                pool_size_sol,
                pool_age_secs_at_buy,
                velocity_sol_per_sec_at_buy,
                buy_pressure_ratio_at_buy,
                upfront_score,
                pool.pumpfun_score,
                inflow_rate_now_sol_per_sec,
                high_speed,
                false,
                0.0,
                0,
                self.config.min_pool_score,
                dq_action.as_str(),
                dq_confidence,
            );
            return Ok(());
        }

        // Compute buy min_out using the AMM constant-product formula + slippage.
        // Use the upfront reserves passed from on_new_pool (saved vs another RPC
        // call). Falls back to 0 (accept any) if reserves are unknown/zero.
        let buy_min_out = if quote_reserve_lam > 0 && base_reserve_lam > 0 {
            let expected = amm_out(
                effective_quote_amount_raw,
                quote_reserve_lam,
                base_reserve_lam,
            );
            if expected > 0 {
                scematica_core::token::apply_slippage(expected, self.config.buy_slippage_pct)
            } else {
                0
            }
        } else {
            0
        };

        // Confirmation window: wait briefly and verify price hasn't already pumped 15%+.
        // Skipped in high-speed mode; skipped when confirmation_window_ms == 0 or quote
        // reserves are unknown (can't compute the drain percentage without them).
        if self.config.confirmation_window_ms > 0
            && !self.high_speed_mode.load(Ordering::Relaxed)
            && quote_reserve_lam > 0
        {
            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.config.confirmation_window_ms,
            ))
            .await;
            if let Ok(Ok(qb)) = tokio::time::timeout(
                tokio::time::Duration::from_secs(2),
                self.rpc.get_token_account_balance(&pool.quote_vault),
            )
            .await
            {
                if let Ok(current_q) = qb.amount.parse::<u64>() {
                    // Quote vault draining = early buyers already dumping.
                    // Raised 15% → 25%: for 1000x runners, a 20% initial pump is
                    // irrelevant to the final outcome; at 15% we were rejecting fast
                    // movers that hadn't peaked yet. Only abort on clear dumps (>25%).
                    if current_q < quote_reserve_lam {
                        let drain_pct = (quote_reserve_lam - current_q) as f64
                            / quote_reserve_lam as f64
                            * 100.0;
                        if drain_pct > 25.0 {
                            info!(
                                mint = %pool.base_mint,
                                drain_pct = %format!("{:.1}%", drain_pct),
                                "Confirmation window: vault drained >25% — early dump, skipping buy"
                            );
                            self.write_pool_decision(
                                pool,
                                "rejected",
                                "confirmation_window",
                                format!("quote_vault_drain_pct={:.1}", drain_pct),
                                pool_size_sol,
                                pool_age_secs_at_buy,
                                velocity_sol_per_sec_at_buy,
                                buy_pressure_ratio_at_buy,
                                upfront_score,
                                pool.pumpfun_score,
                                inflow_rate_now_sol_per_sec,
                                high_speed,
                                false,
                                0.0,
                                0,
                                self.config.min_pool_score,
                                dq_action.as_str(),
                                dq_confidence,
                            );
                            return Ok(());
                        }
                    }
                }
            }
        }

        // Build swap instructions BEFORE acquiring the lock so a build failure
        // doesn't leave the lock permanently set (which would silently drop all
        // future pools).
        let ixs = match self
            .raydium_builder
            .build_swap(
                &pool.id,
                &wallet_pubkey,
                &self.quote_mint,
                &pool.base_mint,
                &quote_ata,
                &base_ata,
                effective_quote_amount_raw,
                buy_min_out,
            )
            .await
        {
            Ok(ixs) => ixs,
            Err(e) => {
                error!(mint = %pool.base_mint, "Failed to build swap instructions: {}", e);
                self.write_pool_decision(
                    pool,
                    "rejected",
                    "build_swap",
                    e.to_string(),
                    pool_size_sol,
                    pool_age_secs_at_buy,
                    velocity_sol_per_sec_at_buy,
                    buy_pressure_ratio_at_buy,
                    upfront_score,
                    pool.pumpfun_score,
                    inflow_rate_now_sol_per_sec,
                    high_speed,
                    false,
                    0.0,
                    0,
                    self.config.min_pool_score,
                    dq_action.as_str(),
                    dq_confidence,
                );
                self.metrics.record_trade_failed();
                return Err(e);
            }
        };

        // ── Pre-send vault re-check — abort if pool drained since filter evaluation ──
        // This catches the most common -100% rug pattern: pool passes filters then
        // gets drained in the 50-200ms between evaluation and tx send.
        if quote_reserve_lam > 0 {
            if let Ok(Ok(qb)) = tokio::time::timeout(
                tokio::time::Duration::from_millis(800),
                self.rpc.get_token_account_balance(&pool.quote_vault),
            )
            .await
            {
                if let Ok(current_q) = qb.amount.parse::<u64>() {
                    let drain_pct = if current_q < quote_reserve_lam {
                        (quote_reserve_lam - current_q) as f64 / quote_reserve_lam as f64 * 100.0
                    } else {
                        0.0
                    };
                    if drain_pct > 50.0 {
                        info!(
                            mint = %pool.base_mint,
                            drain_pct = %format!("{:.1}%", drain_pct),
                            "Pre-send vault check: pool drained >50% — aborting buy (rug in progress)"
                        );
                        self.filter_pipeline
                            .stats
                            .record_rejection("pre_send_drain");
                        self.write_pool_decision(
                            pool,
                            "rejected",
                            "pre_send_drain",
                            format!("quote_vault_drain_pct={:.1}", drain_pct),
                            pool_size_sol,
                            pool_age_secs_at_buy,
                            velocity_sol_per_sec_at_buy,
                            buy_pressure_ratio_at_buy,
                            upfront_score,
                            pool.pumpfun_score,
                            inflow_rate_now_sol_per_sec,
                            high_speed,
                            false,
                            0.0,
                            0,
                            self.config.min_pool_score,
                            dq_action.as_str(),
                            dq_confidence,
                        );
                        return Ok(());
                    }
                }
            }
        }

        // One-buy-at-a-time gate: prevents two buy TRANSACTIONS from racing on the
        // same WSOL ATA simultaneously. Released as soon as the buy tx confirms (NOT
        // held for the duration of sell monitoring — see release below).
        // High-speed mode bypasses so multiple pools can snipe in parallel.
        high_speed = self.high_speed_mode.load(Ordering::Relaxed);
        let slot = if self.config.one_token_at_a_time && !high_speed {
            match ProcessingSlot::try_acquire(&self.processing_lock) {
                Some(g) => Some(g),
                None => {
                    debug!(mint = %pool.base_mint, "one_token_at_a_time: buy tx in flight, skipping");
                    self.write_pool_decision(
                        pool,
                        "ignored",
                        "execution_slot",
                        "one_token_at_a_time_busy",
                        pool_size_sol,
                        pool_age_secs_at_buy,
                        velocity_sol_per_sec_at_buy,
                        buy_pressure_ratio_at_buy,
                        upfront_score,
                        pool.pumpfun_score,
                        inflow_rate_now_sol_per_sec,
                        high_speed,
                        false,
                        0.0,
                        0,
                        self.config.min_pool_score,
                        dq_action.as_str(),
                        dq_confidence,
                    );
                    return Ok(());
                }
            }
        } else {
            None
        };

        if self.config.max_concurrent_positions > 0 {
            let open = self.open_positions.load(Ordering::Relaxed);
            if open >= self.config.max_concurrent_positions {
                debug!(
                    mint = %pool.base_mint,
                    open,
                    max = self.config.max_concurrent_positions,
                    "Max concurrent positions reached after buy slot acquisition — skipping"
                );
                self.write_pool_decision(
                    pool,
                    "ignored",
                    "exposure",
                    "max_concurrent_positions_after_slot",
                    pool_size_sol,
                    pool_age_secs_at_buy,
                    velocity_sol_per_sec_at_buy,
                    buy_pressure_ratio_at_buy,
                    upfront_score,
                    pool.pumpfun_score,
                    inflow_rate_now_sol_per_sec,
                    high_speed,
                    false,
                    0.0,
                    0,
                    self.config.min_pool_score,
                    dq_action.as_str(),
                    dq_confidence,
                );
                return Ok(());
            }
        }

        // Count this as a real attempt only after the lock is secured —
        // pools skipped by try_acquire() above were never attempted.
        self.metrics.record_trade_attempt();
        info!(
            mint = %pool.base_mint,
            amount_sol = effective_quote_amount_raw as f64 / 1e9,
            "Executing buy"
        );
        self.write_pool_decision(
            pool,
            "accepted",
            "execution",
            format!(
                "amount_sol={:.6};min_out={}",
                effective_quote_amount_raw as f64 / 1e9,
                buy_min_out
            ),
            pool_size_sol,
            pool_age_secs_at_buy,
            velocity_sol_per_sec_at_buy,
            buy_pressure_ratio_at_buy,
            upfront_score,
            pool.pumpfun_score,
            inflow_rate_now_sol_per_sec,
            high_speed,
            false,
            0.0,
            0,
            self.config.min_pool_score,
            dq_action.as_str(),
            dq_confidence,
        );

        // Build the final instruction list:
        // 1. Create WSOL ATA (idempotent)
        // 2. Transfer SOL into WSOL ATA via system program
        // 3. SyncNative — reflect the lamports as WSOL token balance
        // 4. Create base token ATA (idempotent destination for the swap)
        // 5. Raydium swap
        let mut final_ixs: Vec<solana_sdk::instruction::Instruction> = Vec::new();

        if self.quote_mint == known_tokens::WSOL_MINT {
            // Ensure the WSOL ATA exists before funding it
            final_ixs.push(
                spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                    &wallet_pubkey,
                    &wallet_pubkey,
                    &known_tokens::WSOL_MINT,
                    &spl_token::id(),
                )
            );
            // Transfer the exact buy amount as native SOL into the WSOL ATA
            final_ixs.push(solana_sdk::system_instruction::transfer(
                &wallet_pubkey,
                &quote_ata,
                effective_quote_amount_raw,
            ));
            // SyncNative — Raydium reads the SPL token balance, not raw lamports
            final_ixs.push(solana_sdk::instruction::Instruction {
                program_id: spl_token::id(),
                accounts: vec![solana_sdk::instruction::AccountMeta::new(quote_ata, false)],
                data: vec![17u8],
            });
        }

        // Create the base token ATA (destination for bought tokens)
        final_ixs.push(
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                &wallet_pubkey,
                &wallet_pubkey,
                &pool.base_mint,
                &spl_token::id(),
            ),
        );

        final_ixs.extend(ixs);
        let ixs = final_ixs;

        // Per-attempt deadline — independently of the executor's own internal cap.
        // Defense in depth: even if a custom executor (e.g. Jito) blocks forever
        // on the bundle endpoint, the buy task here can never hold the slot beyond this.
        const BUY_ATTEMPT_DEADLINE: tokio::time::Duration = tokio::time::Duration::from_secs(8);

        for attempt in 0..self.config.max_buy_retries {
            info!(
                "Buy attempt {}/{}",
                attempt + 1,
                self.config.max_buy_retries
            );
            let exec_outcome = tokio::time::timeout(
                BUY_ATTEMPT_DEADLINE,
                self.executor.execute(ixs.clone(), &self.wallet, &self.rpc),
            )
            .await;
            let exec_result = match exec_outcome {
                Ok(r) => r,
                Err(_) => {
                    warn!(
                        mint = %pool.base_mint,
                        attempt = attempt + 1,
                        "executor.execute() exceeded {:?} — skipping to next attempt",
                        BUY_ATTEMPT_DEADLINE,
                    );
                    continue;
                }
            };
            match exec_result {
                Ok(result) if result.confirmed => {
                    info!(
                        mint = %pool.base_mint,
                        sig = ?result.signature,
                        "Buy confirmed"
                    );
                    self.metrics.record_trade_confirmed(0);
                    // Stamp the dedup guard so any subsequent pool events for this
                    // mint are skipped for mint_cooldown_secs. Write unix timestamp
                    // (not Instant) so the guard survives process restarts.
                    let buy_ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    self.recently_bought.insert(pool.base_mint, buy_ts);
                    save_mint_cooldown(&self.recently_bought);

                    // Emit trade event for the dashboard
                    TradeEvent {
                        timestamp: chrono::Utc::now(),
                        kind: "BUY".into(),
                        mint: pool.base_mint.to_string(),
                        symbol: self
                            .filter_pipeline
                            .metadata
                            .get(&pool.base_mint.to_string())
                            .map(|m| m.symbol.clone())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_default(),
                        amount: scematica_core::token::raw_to_ui(
                            effective_quote_amount_raw,
                            self.quote_decimals,
                        ),
                        pnl: 0.0,
                        status: "✓".into(),
                        signature: result.signature.map(|s| s.to_string()).unwrap_or_default(),
                        dex: "Raydium".into(),
                        hops: 1,
                        pnl_pct: 0.0,
                        position_age_secs: 0.0,
                        exit_reason: String::new(),
                        pool_size_sol,
                        pool_age_secs: pool_age_secs_at_buy,
                        velocity_sol_per_sec: velocity_sol_per_sec_at_buy,
                        buy_pressure_ratio: buy_pressure_ratio_at_buy,
                        pool_score: upfront_score,
                        pumpfun_score: pool.pumpfun_score,
                        inflow_rate_sol_per_sec: inflow_rate_now_sol_per_sec,
                    }
                    .append_to_file(TRADES_FILE);

                    // Track open positions
                    self.open_positions.fetch_add(1, Ordering::Relaxed);

                    // Alert: buy confirmed
                    {
                        let alerts = self.alerts.clone();
                        let mint_str = pool.base_mint.to_string();
                        let amount_sol = scematica_core::token::raw_to_ui(
                            effective_quote_amount_raw,
                            self.quote_decimals,
                        );
                        tokio::spawn(async move {
                            alerts
                                .send(
                                    "BUY Confirmed",
                                    &format!(
                                        "Mint: {}...\nAmount: {:.4} SOL",
                                        &mint_str[..8],
                                        amount_sol
                                    ),
                                )
                                .await;
                        });
                    }

                    // Buy limit tracking — activate sell mode when limit is hit
                    if self.config.max_buys > 0 {
                        let count = self.buy_count.fetch_add(1, Ordering::Relaxed) + 1;
                        info!("Buy #{}/{} confirmed", count, self.config.max_buys);
                        if count >= self.config.max_buys {
                            warn!(
                                "Buy limit of {} reached — activating sell mode, waiting for all positions to close",
                                self.config.max_buys
                            );
                            self.sell_mode.store(true, Ordering::Relaxed);
                            let _ = std::fs::write(
                                "scematica-sell-mode.json",
                                r#"{"active":true,"reason":"buy_limit"}"#,
                            );
                        }
                    }

                    // Release the buy lock immediately so the next pool can be
                    // evaluated without waiting for this position to close.
                    // Concurrent position count is gated by max_concurrent_positions
                    // (checked before instruction-building) via open_positions atomic.
                    drop(slot);

                    if self.config.auto_sell {
                        let pool_clone = pool.clone();
                        // CRITICAL: pass the EFFECTIVE entry amount (post all
                        // multipliers) to the SellMonitor — not the config baseline.
                        // The bug this fixes: Micro mode buys 0.001 SOL but the
                        // monitor computed SL/TP against 0.01 SOL and tripped SL
                        // on the first check every time, with a fake -90 % loss.
                        let monitor = self.clone_for_sell(effective_quote_amount_raw);
                        tokio::spawn(async move {
                            monitor.monitor_and_sell(pool_clone, base_ata).await;
                        });
                    }
                    return Ok(());
                }
                Ok(result) => {
                    warn!(
                        mint = %pool.base_mint,
                        error = ?result.error,
                        "Buy attempt failed"
                    );
                }
                Err(e) => {
                    error!(mint = %pool.base_mint, "Buy error: {}", e);
                }
            }
        }

        self.metrics.record_trade_failed();
        // Emit failed trade event
        TradeEvent {
            timestamp: chrono::Utc::now(),
            kind: "BUY".into(),
            mint: pool.base_mint.to_string(),
            symbol: self
                .filter_pipeline
                .metadata
                .get(&pool.base_mint.to_string())
                .map(|m| m.symbol.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_default(),
            amount: scematica_core::token::raw_to_ui(
                effective_quote_amount_raw,
                self.quote_decimals,
            ),
            pnl: 0.0,
            status: "✗".into(),
            signature: String::new(),
            dex: "Raydium".into(),
            hops: 1,
            pnl_pct: 0.0,
            position_age_secs: 0.0,
            exit_reason: String::new(),
            pool_size_sol,
            pool_age_secs: pool_age_secs_at_buy,
            velocity_sol_per_sec: velocity_sol_per_sec_at_buy,
            buy_pressure_ratio: buy_pressure_ratio_at_buy,
            pool_score: upfront_score,
            pumpfun_score: pool.pumpfun_score,
            inflow_rate_sol_per_sec: inflow_rate_now_sol_per_sec,
        }
        .append_to_file(TRADES_FILE);
        self.write_pool_decision(
            pool,
            "rejected",
            "execution",
            "buy_not_confirmed",
            pool_size_sol,
            pool_age_secs_at_buy,
            velocity_sol_per_sec_at_buy,
            buy_pressure_ratio_at_buy,
            upfront_score,
            pool.pumpfun_score,
            inflow_rate_now_sol_per_sec,
            high_speed,
            false,
            0.0,
            0,
            self.config.min_pool_score,
            dq_action.as_str(),
            dq_confidence,
        );
        // ProcessingSlot::Drop releases the lock when `slot` goes out of scope here.
        Ok(())
    }

    #[allow(dead_code)]
    async fn monitor_and_sell(&self, pool: CachedPool, base_ata: Pubkey) {
        if self.config.auto_sell_delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.config.auto_sell_delay_ms,
            ))
            .await;
        }

        let interval = tokio::time::Duration::from_millis(self.config.price_check_interval_ms);
        let max_checks = if self.config.price_check_interval_ms > 0 {
            self.config.price_check_duration_ms / self.config.price_check_interval_ms
        } else {
            1
        };

        // Use live_params so Strategy Agent adjustments + rate mode TP/SL take effect immediately
        let (take_profit_pct, stop_loss_pct, entry_amount_raw) = {
            let p = self.live_params.read();
            // Use rate mode's TP/SL, or fall back to base config if not set
            let tp = if (p.take_profit_pct_mode - 0.0).abs() > 0.01 {
                p.take_profit_pct_mode
            } else {
                p.take_profit_pct
            };
            let sl = if (p.stop_loss_pct_mode - 0.0).abs() > 0.01 {
                p.stop_loss_pct_mode
            } else {
                p.stop_loss_pct
            };
            // Entry amount from rate mode (converted to raw lamports)
            let entry_raw = ui_to_raw(p.quote_amount_mode, self.quote_decimals);
            (tp, sl, entry_raw)
        };
        let take_profit_factor = 1.0 + take_profit_pct / 100.0;
        let stop_loss_factor = 1.0 - stop_loss_pct / 100.0;
        let target_profit = (entry_amount_raw as f64 * take_profit_factor) as u64;
        let stop_loss = (entry_amount_raw as f64 * stop_loss_factor) as u64;

        let mut checks = 0u64;
        loop {
            // Get current token balance
            match self.rpc.get_token_account_balance(&base_ata).await {
                Ok(balance) => {
                    let amount: u64 = balance.amount.parse().unwrap_or(0);
                    if amount == 0 {
                        info!(mint = %pool.base_mint, "Token balance is zero, skipping sell");
                        break;
                    }

                    // Estimate current value via pool reserves
                    if let Ok(quote_balance) =
                        self.rpc.get_token_account_balance(&pool.quote_vault).await
                    {
                        if let Ok(base_balance) =
                            self.rpc.get_token_account_balance(&pool.base_vault).await
                        {
                            let q: u64 = quote_balance.amount.parse().unwrap_or(1);
                            let b: u64 = base_balance.amount.parse().unwrap_or(1);
                            // Estimate: current_value ≈ amount * (q / b)
                            let current_value = (amount as u128 * q as u128 / b as u128) as u64;

                            debug!(
                                mint = %pool.base_mint,
                                current = current_value,
                                target = target_profit,
                                stop = stop_loss,
                                "Price check"
                            );

                            if current_value >= target_profit {
                                info!(mint = %pool.base_mint, "Take profit triggered");
                                self.sell(&pool, &base_ata, amount).await;
                                let pnl_sol = (current_value as f64 - entry_amount_raw as f64)
                                    / 1_000_000_000.0;
                                self.record_trade_outcome(true, pnl_sol);
                                break;
                            }
                            if current_value <= stop_loss {
                                info!(mint = %pool.base_mint, "Stop loss triggered");
                                self.sell(&pool, &base_ata, amount).await;
                                let pnl_sol = (current_value as f64 - entry_amount_raw as f64)
                                    / 1_000_000_000.0;
                                self.record_trade_outcome(false, pnl_sol);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(mint = %pool.base_mint, "Price check error: {}", e);
                }
            }

            checks += 1;
            if checks >= max_checks {
                info!(mint = %pool.base_mint, "Price check duration expired, force selling");
                if let Ok(balance) = self.rpc.get_token_account_balance(&base_ata).await {
                    let amount: u64 = balance.amount.parse().unwrap_or(0);
                    if amount > 0 {
                        self.sell(&pool, &base_ata, amount).await;
                    }
                }
                break;
            }

            tokio::time::sleep(interval).await;
        }
    }

    #[allow(dead_code)]
    async fn sell(&self, pool: &CachedPool, base_ata: &Pubkey, amount: u64) {
        self.sell_with_min_out(pool, base_ata, amount, None).await;
    }

    /// Sell with escalating slippage: each retry uses a looser min_out so that
    /// even a rugged pool (near-zero liquidity) eventually gets closed.
    ///
    /// Slippage ladder per attempt:
    ///   0: config.sell_slippage_pct (e.g. 85%)
    ///   1: 95%
    ///   2+: min_out = 1 lamport (accept any return, just close the position)
    ///
    /// The goal is to ALWAYS exit, even at a total loss, rather than hold a dead
    /// position for hours. A confirmed 100% loss is better than an unconfirmed
    /// position that blocks capital and generates retry noise.
    #[allow(dead_code)]
    async fn sell_with_min_out(
        &self,
        pool: &CachedPool,
        base_ata: &Pubkey,
        amount: u64,
        forced_min_out: Option<u64>,
    ) {
        info!(mint = %pool.base_mint, amount, "Executing sell");
        self.metrics.record_trade_attempt();

        let wallet_pubkey = self.wallet.pubkey();
        let quote_ata = get_ata(&wallet_pubkey, &self.quote_mint);

        let max_retries = self.config.max_sell_retries.max(3);

        for attempt in 0..max_retries {
            // Escalating slippage: wide on first try, near-zero min_out by attempt 2
            let min_out = if let Some(forced) = forced_min_out {
                forced
            } else {
                match attempt {
                    0 => {
                        // Estimate expected SOL return via AMM formula then apply slippage
                        // Use pool vault data if available; fall back to 1 lamport floor
                        let expected = self.estimate_sell_out(pool, amount).await;
                        apply_slippage(expected.max(1), self.config.sell_slippage_pct)
                    }
                    1 => {
                        // 95% slippage — accept almost any return
                        let expected = self.estimate_sell_out(pool, amount).await;
                        apply_slippage(expected.max(1), 95.0)
                    }
                    _ => {
                        // Fully drained pool: accept 1 lamport to close position
                        1
                    }
                }
            };

            let ixs = match self
                .raydium_builder
                .build_swap(
                    &pool.id,
                    &wallet_pubkey,
                    &pool.base_mint,
                    &self.quote_mint,
                    base_ata,
                    &quote_ata,
                    amount,
                    min_out,
                )
                .await
            {
                Ok(ixs) => ixs,
                Err(e) => {
                    error!(
                        "Failed to build sell instructions (attempt {}): {}",
                        attempt + 1,
                        e
                    );
                    if attempt + 1 >= max_retries {
                        break;
                    }
                    continue;
                }
            };

            info!(
                "Sell attempt {}/{} (min_out={})",
                attempt + 1,
                max_retries,
                min_out
            );
            match self
                .executor
                .execute(ixs.clone(), &self.wallet, &self.rpc)
                .await
            {
                Ok(result) if result.confirmed => {
                    info!(
                        mint = %pool.base_mint,
                        sig = ?result.signature,
                        attempt = attempt + 1,
                        "Sell confirmed"
                    );
                    self.metrics.record_trade_confirmed(0);
                    TradeEvent {
                        timestamp: chrono::Utc::now(),
                        kind: "SELL".into(),
                        mint: pool.base_mint.to_string(),
                        symbol: String::new(),
                        amount: scematica_core::token::raw_to_ui(amount, pool.base_decimals),
                        pnl: 0.0,
                        status: "✓".into(),
                        signature: result.signature.map(|s| s.to_string()).unwrap_or_default(),
                        dex: "Raydium".into(),
                        hops: 1,
                        pnl_pct: 0.0,
                        position_age_secs: 0.0,
                        exit_reason: String::new(),
                        pool_size_sol: 0.0,
                        pool_age_secs: 0.0,
                        velocity_sol_per_sec: 0.0,
                        buy_pressure_ratio: 0.0,
                        pool_score: 0.0,
                        pumpfun_score: 0.0,
                        inflow_rate_sol_per_sec: 0.0,
                    }
                    .append_to_file(TRADES_FILE);
                    return;
                }
                Ok(result) => {
                    warn!(
                        attempt = attempt + 1,
                        min_out,
                        error = ?result.error,
                        "Sell attempt failed — escalating slippage next retry"
                    );
                }
                Err(e) => {
                    error!(attempt = attempt + 1, "Sell error: {}", e);
                }
            }
        }

        self.metrics.record_trade_failed();
        warn!(mint = %pool.base_mint, "All sell attempts exhausted — position may be stuck in dead pool");
        TradeEvent {
            timestamp: chrono::Utc::now(),
            kind: "SELL".into(),
            mint: pool.base_mint.to_string(),
            symbol: String::new(),
            amount: scematica_core::token::raw_to_ui(amount, pool.base_decimals),
            pnl: 0.0,
            status: "✗".into(),
            signature: String::new(),
            dex: "Raydium".into(),
            hops: 1,
            pnl_pct: -100.0,
            position_age_secs: 0.0,
            exit_reason: "failed".into(),
            pool_size_sol: 0.0,
            pool_age_secs: 0.0,
            velocity_sol_per_sec: 0.0,
            buy_pressure_ratio: 0.0,
            pool_score: 0.0,
            pumpfun_score: 0.0,
            inflow_rate_sol_per_sec: 0.0,
        }
        .append_to_file(TRADES_FILE);
    }

    /// Estimate SOL return for a sell via Raydium constant-product AMM.
    /// Returns 0 if vault data is unavailable (caller should use min_out=1).
    async fn estimate_sell_out(&self, pool: &CachedPool, token_amount_in: u64) -> u64 {
        let q = match tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            self.rpc.get_token_account_balance(&pool.quote_vault),
        )
        .await
        {
            Ok(Ok(b)) => b.amount.parse::<u64>().unwrap_or(0),
            _ => return 0,
        };
        let b = match tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            self.rpc.get_token_account_balance(&pool.base_vault),
        )
        .await
        {
            Ok(Ok(b)) => b.amount.parse::<u64>().unwrap_or(1),
            _ => return 0,
        };
        if q == 0 || b == 0 {
            return 0;
        }
        // AMM out: out = q * amount_in * 9975 / (b * 10000 + amount_in * 9975)
        let num = (q as u128) * (token_amount_in as u128) * 9975u128;
        let den = (b as u128) * 10000u128 + (token_amount_in as u128) * 9975u128;
        if den == 0 {
            return 0;
        }
        (num / den) as u64
    }

    async fn on_wallet_update(&self, account: Pubkey, mint: Pubkey, amount: u64) {
        debug!(account = %account, mint = %mint, amount, "Wallet update");
    }

    /// Scan wallet for existing token positions and spawn a sell monitor for each.
    /// Called once at startup so tokens bought in a previous run can be sold.
    pub async fn scan_existing_positions(&self) {
        use solana_client::rpc_request::TokenAccountsFilter;
        use solana_sdk::{program_pack::Pack, pubkey};

        // Token-2022 program — all Pump.fun meme coins use this, NOT legacy SPL Token.
        // Without scanning it, the sniper starts blind to existing pump.fun positions and
        // spawns no sell monitors for them — they sit unmonitored until sell_mode fires.
        const TOKEN_2022_PROGRAM: solana_sdk::pubkey::Pubkey =
            pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

        let wallet_pubkey = self.wallet.pubkey();
        info!("Startup scan: checking wallet for existing token positions (SPL + Token-2022)...");

        let mut all_keyed = Vec::new();
        for program_id in [spl_token::id(), TOKEN_2022_PROGRAM] {
            match self
                .rpc
                .get_token_accounts_by_owner(
                    &wallet_pubkey,
                    TokenAccountsFilter::ProgramId(program_id),
                )
                .await
            {
                Ok(a) => {
                    info!("Startup scan: {} accounts under {}", a.len(), program_id);
                    all_keyed.extend(a);
                }
                Err(e) => {
                    warn!(
                        "Startup scan: failed to list accounts under {}: {}",
                        program_id, e
                    );
                }
            }
        }
        let keyed_accounts = all_keyed;

        const NO_POOL_FILE: &str = "scematica-no-pool.json";

        // Load mints previously confirmed unsellable (no Raydium pool) so we skip
        // them without RPC calls on every subsequent sell-mode activation.
        let mut no_pool_blacklist: std::collections::HashSet<String> =
            std::fs::read_to_string(NO_POOL_FILE)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

        let skip_mints: std::collections::HashSet<Pubkey> = [
            self.quote_mint,
            known_tokens::WSOL_MINT,
            known_tokens::SCEMATICA_MINT,
        ]
        .into_iter()
        .collect();

        let mut spawned = 0u32;
        for keyed in keyed_accounts {
            let ata_pk = match keyed.pubkey.parse::<Pubkey>() {
                Ok(p) => p,
                Err(_) => continue,
            };

            // Unpack the raw token account to get mint and balance.
            // Token-2022 accounts have extension bytes after the base 165 bytes, so
            // spl_token::state::Account::unpack (which requires exactly 165 bytes) may
            // fail. Fall back to reading mint (bytes 0..32) and amount (bytes 64..72)
            // from the raw layout which is identical for both programs.
            let raw = match self.rpc.get_account(&ata_pk).await {
                Ok(a) => a,
                Err(_) => continue,
            };
            let (mint, amount) = if let Ok(acct) = spl_token::state::Account::unpack(&raw.data) {
                (acct.mint, acct.amount)
            } else if raw.data.len() >= 72 {
                // Token-2022 base layout: mint at 0..32, owner at 32..64, amount at 64..72
                let mint_bytes: [u8; 32] = match raw.data[0..32].try_into() {
                    Ok(b) => b,
                    Err(_) => continue,
                };
                let amount = u64::from_le_bytes(match raw.data[64..72].try_into() {
                    Ok(b) => b,
                    Err(_) => continue,
                });
                (Pubkey::from(mint_bytes), amount)
            } else {
                continue;
            };

            if amount == 0 {
                continue;
            }
            if skip_mints.contains(&mint) {
                continue;
            }

            // Skip mints previously confirmed to have no Raydium pool — these are
            // rugged tokens we can never sell; avoid wasting RPC calls on them.
            if no_pool_blacklist.contains(&mint.to_string()) {
                debug!(mint = %mint, "Startup scan: skipping blacklisted no-pool mint");
                continue;
            }

            info!(
                mint = %mint,
                amount,
                "Startup scan: found existing position — looking up Raydium pool"
            );

            let pool = match self.find_raydium_pool_for_mint(&mint).await {
                Some(p) => p,
                None => {
                    warn!(mint = %mint, "Startup scan: no Raydium pool found — blacklisting mint");
                    no_pool_blacklist.insert(mint.to_string());
                    // Persist the updated blacklist atomically
                    if let Ok(json) = serde_json::to_string(&no_pool_blacklist) {
                        let tmp = format!("{}.tmp", NO_POOL_FILE);
                        if std::fs::write(&tmp, &json).is_ok() {
                            let _ = std::fs::rename(&tmp, NO_POOL_FILE);
                        }
                    }
                    continue;
                }
            };

            if pool.quote_mint != self.quote_mint {
                debug!(mint = %mint, "Startup scan: pool quote mint mismatch — skipping");
                continue;
            }

            // Dedup guard: if live_positions already tracks this mint, a sell monitor
            // is already running (spawned by buy() or an earlier scan_existing_positions
            // call). Spawning a second monitor causes duplicate sells on the same ATA.
            let mint_str = mint.to_string();
            if self.live_positions.contains_key(&mint_str) {
                debug!(mint = %mint, "Startup scan: sell monitor already running — skipping");
                continue;
            }

            info!(
                mint = %mint,
                pool = %pool.id,
                amount,
                "Startup scan: spawning sell monitor"
            );

            // Pre-existing wallet position from a prior session — we don't know
            // the real entry amount, so use the config baseline as the floor.
            // sell_mode / dump_mode handles these positions anyway; the entry
            // value here only affects the trailing-stop reference.
            //
            // CRITICAL: increment open_positions BEFORE spawning so the counter
            // is balanced when monitor_and_sell calls fetch_sub on exit. Without
            // this, startup-scan monitors underflow the u32 → wrap to u32::MAX,
            // breaking the buy-limit sell-mode auto-clear logic for the whole session.
            self.open_positions.fetch_add(1, Ordering::Relaxed);
            // Register in live_positions now so subsequent scan calls see this mint
            // as already-monitored.
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            self.live_positions.insert(
                mint_str.clone(),
                LivePositionSnapshot {
                    mint: mint_str,
                    entry_lamports: self.quote_amount_raw,
                    current_value_lamports: self.quote_amount_raw,
                    peak_value_lamports: self.quote_amount_raw,
                    entry_unix_secs: now_secs,
                    dynamic_tp_pct: self.config.take_profit_pct,
                    escalations: 0,
                    last_check_unix_secs: now_secs,
                    current_sl_lamports: (self.quote_amount_raw as f64
                        * (1.0 - self.config.stop_loss_pct / 100.0))
                        as u64,
                    current_sl_pct: -self.config.stop_loss_pct,
                    decline_streak: 0,
                },
            );
            let monitor = self.clone_for_sell(self.quote_amount_raw);
            tokio::spawn(async move {
                monitor.monitor_and_sell(pool, ata_pk).await;
            });
            spawned += 1;
        }

        if spawned == 0 {
            info!("Startup scan: no existing positions found");
        } else {
            info!("Startup scan: spawned {} sell monitor(s)", spawned);
        }
    }

    /// Immediately force-sell every token position in the wallet.
    /// Unlike scan_existing_positions (which spawns price monitors), this calls
    /// sell_with_retry directly — with dump_mode active, min_out=0 so any price is accepted.
    ///
    /// Scans both legacy SPL Token and Token-2022 accounts (Pump.fun mints are Token-2022).
    /// Checks the in-memory pool_cache first before falling back to getProgramAccounts.
    pub async fn auto_dump(&self) {
        use solana_client::rpc_request::TokenAccountsFilter;
        use solana_sdk::program_pack::Pack;
        use solana_sdk::pubkey;

        // Token-2022 program — all Pump.fun meme coins use this
        const TOKEN_2022_PROGRAM: Pubkey = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

        let wallet_pubkey = self.wallet.pubkey();
        warn!("AUTO DUMP: scanning wallet (SPL + Token-2022) for positions");

        // Scan both token programs — Pump.fun coins are Token-2022
        let mut all_keyed = Vec::new();
        for program_id in [spl_token::id(), TOKEN_2022_PROGRAM] {
            match self
                .rpc
                .get_token_accounts_by_owner(
                    &wallet_pubkey,
                    TokenAccountsFilter::ProgramId(program_id),
                )
                .await
            {
                Ok(accounts) => {
                    info!(
                        "AUTO DUMP: found {} accounts under {}",
                        accounts.len(),
                        program_id
                    );
                    all_keyed.extend(accounts);
                }
                Err(e) => warn!("AUTO DUMP: scan failed for program {}: {}", program_id, e),
            }
        }

        if all_keyed.is_empty() {
            info!("AUTO DUMP: no token accounts found");
            return;
        }

        let skip_mints: std::collections::HashSet<Pubkey> = [
            self.quote_mint,
            known_tokens::WSOL_MINT,
            known_tokens::SCEMATICA_MINT,
        ]
        .into_iter()
        .collect();

        let mut spawned = 0u32;
        for keyed in all_keyed {
            let ata_pk = match keyed.pubkey.parse::<Pubkey>() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let raw = match self.rpc.get_account(&ata_pk).await {
                Ok(a) => a,
                Err(_) => continue,
            };

            // Try standard unpack; fall back to raw byte parse for Token-2022 accounts
            // with extension data (which makes data.len() > 165 and fails unpack).
            // SPL token account layout: [0..32]=mint, [32..64]=owner, [64..72]=amount (u64 LE)
            let (mint, amount) = if let Ok(acct) = spl_token::state::Account::unpack(&raw.data) {
                (acct.mint, acct.amount)
            } else if raw.data.len() >= 72 {
                let mint = match Pubkey::try_from(&raw.data[0..32]) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let amount = u64::from_le_bytes(raw.data[64..72].try_into().unwrap_or([0u8; 8]));
                (mint, amount)
            } else {
                continue;
            };

            if amount == 0 {
                continue;
            }
            if skip_mints.contains(&mint) {
                continue;
            }

            warn!(mint = %mint, amount, "AUTO DUMP: found position — looking up pool");

            // Check in-memory pool cache first (free, no RPC) — covers tokens bought this run.
            // Fall back to getProgramAccounts (expensive) for tokens from a previous run.
            let pool = if let Some(cached) = self.pool_cache.find_by_base_mint(&mint) {
                info!(mint = %mint, pool = %cached.id, "AUTO DUMP: using cached pool");
                cached
            } else {
                match self.find_raydium_pool_for_mint(&mint).await {
                    Some(p) => p,
                    None => {
                        warn!(mint = %mint, "AUTO DUMP: no Raydium pool found — skipping");
                        continue;
                    }
                }
            };

            if pool.quote_mint != self.quote_mint {
                debug!(mint = %mint, "AUTO DUMP: pool quote mint mismatch — skipping");
                continue;
            }

            warn!(mint = %mint, pool = %pool.id, amount, "AUTO DUMP: spawning force-sell");
            // Auto-dump fires sell_with_retry directly — PnL math is never read.
            // Pass config baseline; the value is unused on this code path.
            let monitor = self.clone_for_sell(self.quote_amount_raw);
            tokio::spawn(async move {
                // Auto-dump path: position age is unknown (could be from a previous session),
                // so feed 0 to the NN observer rather than fabricating a time signal.
                monitor
                    .sell_with_retry(&pool, &ata_pk, amount, 0.0, "dump_mode")
                    .await;
            });
            spawned += 1;
        }

        if spawned == 0 {
            info!("AUTO DUMP: no sellable positions found");
        } else {
            warn!("AUTO DUMP: force-selling {} position(s)", spawned);
        }
    }

    /// Find a Raydium AMM V4 pool where `base_mint` is the base token.
    /// Issues a single `getProgramAccounts` call with memcmp + dataSize filters.
    async fn find_raydium_pool_for_mint(&self, base_mint: &Pubkey) -> Option<CachedPool> {
        use scematica_core::dex::{program_ids, raydium_v4::*};
        use solana_account_decoder::UiAccountEncoding;
        use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
        use solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};

        let config = RpcProgramAccountsConfig {
            filters: Some(vec![
                RpcFilterType::DataSize(POOL_STATE_SIZE as u64),
                RpcFilterType::Memcmp(Memcmp::new(
                    BASE_MINT_OFFSET,
                    MemcmpEncodedBytes::Bytes(base_mint.to_bytes().to_vec()),
                )),
            ]),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                ..Default::default()
            },
            ..Default::default()
        };

        let pools = match self
            .rpc
            .get_program_accounts_with_config(&program_ids::RAYDIUM_AMM_V4, config)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                warn!(mint = %base_mint, "Pool lookup RPC call failed: {}", e);
                return None;
            }
        };

        let (pool_pk, pool_account) = pools.into_iter().next()?;
        let data = &pool_account.data;
        if data.len() < POOL_STATE_SIZE {
            return None;
        }

        let pool_base_mint =
            Pubkey::try_from(&data[BASE_MINT_OFFSET..BASE_MINT_OFFSET + 32]).ok()?;
        let pool_quote_mint =
            Pubkey::try_from(&data[QUOTE_MINT_OFFSET..QUOTE_MINT_OFFSET + 32]).ok()?;
        let base_vault = Pubkey::try_from(&data[BASE_VAULT_OFFSET..BASE_VAULT_OFFSET + 32]).ok()?;
        let quote_vault =
            Pubkey::try_from(&data[QUOTE_VAULT_OFFSET..QUOTE_VAULT_OFFSET + 32]).ok()?;
        let market_id = Pubkey::try_from(&data[MARKET_ID_OFFSET..MARKET_ID_OFFSET + 32]).ok()?;
        let open_time = u64::from_le_bytes(
            data[OPEN_TIME_OFFSET..OPEN_TIME_OFFSET + 8]
                .try_into()
                .ok()?,
        );

        if pool_quote_mint == Pubkey::default() || base_vault == Pubkey::default() {
            return None;
        }

        Some(CachedPool {
            id: pool_pk,
            base_mint: pool_base_mint,
            quote_mint: pool_quote_mint,
            base_vault,
            quote_vault,
            market_id,
            open_time,
            base_decimals: 9,
            quote_decimals: 9,
            pumpfun_score: 0.0,
        })
    }

    /// Clone the parts needed for the sell monitor task.
    ///
    /// `entry_amount_raw` is the lamports actually spent on THIS specific buy
    /// after all multipliers (rate mode × time-of-day × Kelly × progressive).
    /// All PnL / TP / SL math inside SellMonitor uses this — NOT the static
    /// config baseline (which would over-state losses by the multiplier ratio).
    fn clone_for_sell(&self, entry_amount_raw: u64) -> SellMonitor {
        SellMonitor {
            config: self.config.clone(),
            wallet: self.wallet.clone(),
            rpc: self.rpc.clone(),
            executor: self.executor.clone(),
            metrics: self.metrics.clone(),
            quote_mint: self.quote_mint,
            quote_amount_raw: self.quote_amount_raw,
            entry_amount_raw,
            raydium_builder: self.raydium_builder.clone(),
            live_params: self.live_params.clone(),
            trade_history: self.trade_history.clone(),
            sell_mode: self.sell_mode.clone(),
            dump_mode: self.dump_mode.clone(),
            sell_sem: self.sell_sem.clone(),
            buy_count: self.buy_count.clone(),
            open_positions: self.open_positions.clone(),
            consecutive_losses: self.consecutive_losses.clone(),
            cooldown_until_ms: self.cooldown_until_ms.clone(),
            daily_pnl_lamports: self.daily_pnl_lamports.clone(),
            alerts: self.alerts.clone(),
            pool_cache: self.pool_cache.clone(),
            deployer_ledger: self.deployer_ledger.clone(),
            grief_breaker: self.grief_breaker.clone(),
            ath_tracker: self.ath_tracker.clone(),
            session_start_lamports: self.session_start_lamports.clone(),
            wallet_target_lamports_override: self.wallet_target_lamports_override.clone(),
            moon_chase: self.moon_chase.clone(),
            live_positions: self.live_positions.clone(),
            loss_heat_timestamps: self.loss_heat_timestamps.clone(),
            fib_system: self.fib_system.clone(),
            fib_stats: self.fib_stats.clone(),
        }
    }
}

/// Lightweight struct for the sell monitor task (avoids cloning the full Sniper)
struct SellMonitor {
    config: SniperConfig,
    wallet: Arc<Keypair>,
    rpc: Arc<RpcClient>,
    executor: Arc<dyn TxExecutor>,
    metrics: Arc<BotMetrics>,
    quote_mint: Pubkey,
    /// Static config baseline — kept only for back-compat with paths that don't
    /// know the actual entry (auto_dump on pre-existing wallet positions).
    /// Inside monitor_and_sell, ALWAYS use `entry_amount_raw` instead so PnL/
    /// TP/SL math reflects what was actually spent on THIS position.
    #[allow(dead_code)]
    quote_amount_raw: u64,
    /// Actual lamports spent on this specific buy (post all multipliers).
    /// This is the correct baseline for PnL %, TP target, SL trigger, and the
    /// LivePositionSnapshot.entry_lamports field.
    entry_amount_raw: u64,
    raydium_builder: Arc<dyn SwapInstructionBuilder>,
    live_params: Arc<RwLock<LiveParams>>,
    trade_history: Arc<Mutex<Vec<(bool, f64)>>>,
    sell_mode: Arc<AtomicBool>,
    dump_mode: Arc<AtomicBool>,
    sell_sem: Arc<tokio::sync::Semaphore>,
    buy_count: Arc<std::sync::atomic::AtomicU32>,
    open_positions: Arc<std::sync::atomic::AtomicU32>,
    consecutive_losses: Arc<std::sync::atomic::AtomicU32>,
    cooldown_until_ms: Arc<std::sync::atomic::AtomicU64>,
    daily_pnl_lamports: Arc<parking_lot::Mutex<i64>>,
    alerts: Arc<AlertManager>,
    #[allow(dead_code)]
    pool_cache: crate::cache::PoolCache,
    deployer_ledger: Arc<Mutex<DeployerLedger>>,
    grief_breaker: Option<Arc<GriefBreaker>>,
    #[allow(dead_code)]
    ath_tracker: Arc<AthTracker>,
    /// Session start balance — used by profit-first mode to compute approximate
    /// current wallet (start + realised PnL) without an extra RPC hop per check.
    session_start_lamports: Arc<std::sync::atomic::AtomicU64>,
    /// Live override for `config.wallet_target_sol` (lamports). Set by the
    /// builder-mode file watcher. 0 means "fall back to config".
    wallet_target_lamports_override: Arc<AtomicU64>,
    /// Moon Chase toggle — switches momentum-hold to aggressive params.
    moon_chase: Arc<AtomicBool>,
    /// Shared live position registry (dashboard reads via JSON file IPC).
    live_positions: Arc<DashMap<String, LivePositionSnapshot>>,
    /// Session heat loss timestamps — updated on loss; drives the cooldown_until_ms gate
    loss_heat_timestamps: Arc<parking_lot::Mutex<Vec<u64>>>,
    /// Fibonacci recovery system for exit optimization
    fib_system: Arc<FibonacciRecoverySystem>,
    /// Fibonacci recovery statistics
    fib_stats: Arc<Mutex<FibonacciRecoveryStats>>,
}

/// Minimum quote-vault balance considered viable for a swap.
/// 10_000 lamports was too low — pools with up to 500k lamports (≈$0.10) can still
/// produce near-zero output that Raydium AMM rejects, burning all retry rounds.
const DRAIN_THRESHOLD_LAMPORTS: u64 = 500_000;

impl SellMonitor {
    async fn monitor_and_sell(&self, pool: CachedPool, base_ata: Pubkey) {
        // Track position entry so the NN observer gets accurate holding-time signal
        // on every sell path (sell_mode trigger / partial TP / dump / TP-SL / timeout).
        let position_started = std::time::Instant::now();
        let entry_unix_secs = chrono::Utc::now().timestamp();

        // Create Fibonacci momentum tracker for this position
        let mut fib_momentum =
            FibonacciMomentum::new(self.entry_amount_raw, entry_unix_secs as u64);

        // Register this position in the live registry so the dashboard sees it
        // immediately. We update the entry on every price check below and
        // remove it when the monitor exits (any branch).
        let pos_key = pool.base_mint.to_string();
        let initial_sl_lam = (self.entry_amount_raw as f64
            * (1.0 - self.live_params.read().stop_loss_pct / 100.0))
            as u64;
        self.live_positions.insert(
            pos_key.clone(),
            LivePositionSnapshot {
                mint: pos_key.clone(),
                entry_lamports: self.entry_amount_raw,
                current_value_lamports: self.entry_amount_raw,
                peak_value_lamports: self.entry_amount_raw,
                entry_unix_secs,
                dynamic_tp_pct: self.live_params.read().take_profit_pct,
                escalations: 0,
                last_check_unix_secs: entry_unix_secs,
                current_sl_lamports: initial_sl_lam,
                current_sl_pct: -(self.live_params.read().stop_loss_pct),
                decline_streak: 0,
            },
        );
        // RAII guard: ensure the position is removed from the registry no matter
        // which branch of monitor_and_sell exits the function.
        struct PositionGuard {
            map: Arc<DashMap<String, LivePositionSnapshot>>,
            key: String,
        }
        impl Drop for PositionGuard {
            fn drop(&mut self) {
                self.map.remove(&self.key);
            }
        }
        let _pos_guard = PositionGuard {
            map: self.live_positions.clone(),
            key: pos_key.clone(),
        };

        if self.config.auto_sell_delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.config.auto_sell_delay_ms,
            ))
            .await;
        }

        // Fast-poll the first 30 checks at 75 ms to catch the initial pump/dump, then switch to normal interval.
        // The 3x balance fetches per check now run in parallel (tokio::join!), so 75 ms is a realistic
        // floor on a Helius/Triton endpoint — the previous 100 ms left RPC capacity idle between polls.
        let fast_phase_checks: u64 = 30;
        let fast_poll_ms: u64 = 75;
        let fast_interval = tokio::time::Duration::from_millis(fast_poll_ms);
        let normal_floor_ms = 250u64;
        let normal_interval = tokio::time::Duration::from_millis(
            self.config.price_check_interval_ms.max(normal_floor_ms),
        );
        let fast_budget_ms = fast_phase_checks * fast_poll_ms;
        let remaining_budget = self
            .config
            .price_check_duration_ms
            .saturating_sub(fast_budget_ms);
        let normal_checks = if self.config.price_check_interval_ms > 0 {
            remaining_budget / self.config.price_check_interval_ms.max(normal_floor_ms)
        } else {
            0
        };
        let max_checks = fast_phase_checks + normal_checks;

        // Partial TP state
        let partial_tp_enabled =
            self.config.partial_tp_pct > 0.0 && self.config.partial_tp_trigger > 0.0;
        let partial_tp_target =
            (self.entry_amount_raw as f64 * (1.0 + self.config.partial_tp_trigger / 100.0)) as u64;
        let mut partial_tp_done = false;

        // Trailing stop state — peak tracks best value seen since entry
        let trailing_enabled = self.config.trailing_stop_loss_pct > 0.0;
        let mut peak_value: u64 = self.entry_amount_raw;

        // Dump-detection: exit immediately on 3 consecutive declining price checks
        let mut prev_value: u64 = self.entry_amount_raw;
        let mut decline_streak: u32 = 0;

        // Volume exhaustion / whale exit: track quote vault across checks
        let mut entry_q: u64 = 0;
        let mut prev_q: u64 = 0;

        // Live swell: sliding window of per-check quote vault deltas (SOL in/out).
        // Positive avg = net buyers still flowing in (pool swelling).
        // Negative avg = net sellers / LP withdraw (pool draining).
        let swell_win_cap = 6usize;
        let mut swell_window: std::collections::VecDeque<i64> =
            std::collections::VecDeque::with_capacity(swell_win_cap);

        // Momentum-hold state for long-term sniping. dynamic_tp_pct floats above
        // the configured TP via velocity-driven escalation. velocity_window stores
        // recent per-check % deltas (vs entry) for the momentum signal.
        //
        // Moon Chase override: when the dashboard toggles `[m]`, the params below
        // swap to a "chase parabolic outliers" preset that allows ~8 escalations
        // (vs default 4), uses factor 1.75× (vs 1.5×), tolerates 25 % pullback
        // (vs 15 %), and trips on 3 %/check velocity (vs 5 %).
        let moon_chase = self.moon_chase.load(Ordering::Relaxed);
        let (mom_max_esc, mom_factor, mom_pullback, mom_threshold) = if moon_chase {
            (8u32, 1.75f64, 25.0f64, 3.0f64)
        } else {
            // Read escalation count from live_params (set by rate-mode watcher on switch),
            // falling back to static config if the mode field is 0 (unset).
            let mode_esc = self.live_params.read().momentum_max_escalations_mode;
            let max_esc = if mode_esc > 0 {
                mode_esc
            } else {
                self.config.momentum_max_escalations
            };
            (
                max_esc,
                self.config.momentum_escalation_factor,
                self.config.momentum_pullback_exit_pct,
                self.config.momentum_escalation_threshold_pct,
            )
        };
        let initial_tp_pct = self.live_params.read().take_profit_pct;
        // Minimum profit floor in lamports: once position reaches TP, SL is raised
        // to this value so the exit is always >= initial_tp_pct gain (e.g. 0.05 SOL).
        let min_profit_floor_lamports =
            (self.entry_amount_raw as f64 * (1.0 + initial_tp_pct / 100.0)) as u64;
        let mut dynamic_tp_pct: f64 = initial_tp_pct;
        let mut escalation_count: u32 = 0;
        let momentum_window_cap = self.config.momentum_window_checks.max(1) as usize;
        let mut velocity_window: std::collections::VecDeque<f64> =
            std::collections::VecDeque::with_capacity(momentum_window_cap);

        // v0.9.6: separate, larger velocity window for the decay-exit detector.
        // Needs 2 × velocity_decay_window samples to compare "recent half" vs
        // "previous half" averages. Sized independently from momentum_window
        // so the two signals can use different sensitivities.
        let decay_half = self.config.velocity_decay_window.max(1) as usize;
        let decay_window_cap = decay_half * 2;
        let mut decay_window: std::collections::VecDeque<f64> =
            std::collections::VecDeque::with_capacity(decay_window_cap);

        // Tiered partial-TP state: each level fires at most once. We track
        // which trigger %s have already been honored so a sustained pump above
        // the level doesn't re-fire the same partial sell on every check.
        let tiered_enabled =
            self.config.tiered_partial_tp && !self.config.tiered_partial_tp_levels.is_empty();
        let mut tiered_fired: Vec<bool> = vec![false; self.config.tiered_partial_tp_levels.len()];

        // Profit-lock counter: tracks consecutive checks where value > entry.
        // Once it reaches `profit_lock_checks`, the SL floor is raised to near
        // breakeven (entry × 0.98) so a sustained winner can't reverse to a loss.
        let mut profit_lock_counter: u32 = 0;
        let profit_lock_floor = (self.entry_amount_raw as f64 * 0.98) as u64;

        // Zero-balance grace: RPC nodes can lag by 1-3 checks after a confirmed buy.
        // Don't exit immediately on 0 balance — require 5 consecutive zeros before
        // treating the position as closed. This prevents "falling through the cracks"
        // where the monitor exits before the token transfer propagates.
        let mut consecutive_zero_balance: u32 = 0;
        const ZERO_BALANCE_EXIT_THRESHOLD: u32 = 5;

        // Peak stagnation: track the last time peak_value improved. If it hasn't
        // advanced in `peak_stagnation_secs` AND current pnl > threshold → exit.
        // Catches flat pools that pumped once then stopped making new highs — they
        // bleed slowly back while capital sits idle (the 7-11 min 99% exit pattern).
        let mut last_peak_improved = std::time::Instant::now();

        let mut checks = 0u64;
        loop {
            // Hard position time cap: force-sell if we've held longer than the
            // configured max. Prevents capital from being locked in dead pools
            // even when profit-first mode keeps extending the watch window.
            if self.config.max_position_hold_mins > 0 {
                let elapsed_mins = position_started.elapsed().as_secs() / 60;
                if elapsed_mins >= self.config.max_position_hold_mins as u64 {
                    tracing::warn!(
                        mint = %pool.base_mint,
                        elapsed_mins,
                        "Position time cap reached — force selling to free capital"
                    );
                    if let Ok(bal) = self.rpc.get_token_account_balance(&base_ata).await {
                        let amount: u64 = bal.amount.parse().unwrap_or(0);
                        if amount > 0 {
                            self.sell_with_retry(
                                &pool,
                                &base_ata,
                                amount,
                                position_started.elapsed().as_secs_f64(),
                                "timeout",
                            )
                            .await;
                        }
                    }
                    break;
                }
            }

            // Sell/dump mode activated mid-monitor → sell immediately.
            // Exception: pure dump_mode (not sell_mode) respects min_dump_hold_secs —
            // fresh positions get a chance to exit via normal TP/SL rather than being
            // force-sold at min_out=0 mid-pump.
            let sell_on = self.sell_mode.load(Ordering::Relaxed);
            let dump_on = self.dump_mode.load(Ordering::Relaxed);
            if sell_on || dump_on {
                let hold_secs = self.config.min_dump_hold_secs;
                let age_secs = position_started.elapsed().as_secs();
                if dump_on && !sell_on && hold_secs > 0 && age_secs < hold_secs {
                    tracing::info!(
                        mint = %pool.base_mint,
                        age_secs,
                        min_hold = hold_secs,
                        "Dump mode active but position too fresh — holding until min_dump_hold_secs"
                    );
                } else {
                    tracing::info!(mint = %pool.base_mint, "Sell/dump mode active — forcing immediate sell");
                    if let Ok(balance) = self.rpc.get_token_account_balance(&base_ata).await {
                        let amount: u64 = balance.amount.parse().unwrap_or(0);
                        if amount > 0 {
                            let mode_reason = if dump_on { "dump_mode" } else { "sell_mode" };
                            self.sell_with_retry(
                                &pool,
                                &base_ata,
                                amount,
                                position_started.elapsed().as_secs_f64(),
                                mode_reason,
                            )
                            .await;
                        }
                    }
                    break;
                }
            }

            // Re-read live_params every iteration — strategy agent adjustments apply mid-position
            let (take_profit_pct, stop_loss_pct) = {
                let p = self.live_params.read();
                (p.take_profit_pct, p.stop_loss_pct)
            };
            // The dynamic TP is the source of truth; live_params changes can only
            // PUSH IT UP (operator/strategy-agent raising the floor), never below
            // the current momentum-escalated value. This way a Strategy-Agent
            // adjustment doesn't undo an in-flight escalation.
            if take_profit_pct > dynamic_tp_pct {
                dynamic_tp_pct = take_profit_pct;
            }
            let mut target_profit =
                (self.entry_amount_raw as f64 * (1.0 + dynamic_tp_pct / 100.0)) as u64;

            // Profit-first growth mode: while the wallet is still being built up to
            // `wallet_target_sol`, gate the stop-loss so the bot doesn't bleed out
            // small losses on dips that would have recovered. The rug-only floor at
            // `profit_first_floor_pct` is the safety net so a true rug still exits.
            //
            // Approximation of current wallet: session_start + realised daily PnL.
            // Avoids an extra RPC `get_balance` per check (3 already happen).
            //
            // The builder-mode dashboard toggle writes to wallet_target_lamports_override;
            // a non-zero value wins over config.wallet_target_sol so the operator can
            // hot-switch between Growth/Builder/SuperBuilder without restarting.
            let effective_stop_loss_pct = if self.config.profit_first_mode {
                let start_lam = self.session_start_lamports.load(Ordering::Relaxed);
                let daily_pnl = *self.daily_pnl_lamports.lock();
                let approx_wallet_sol = (start_lam as i64 + daily_pnl) as f64 / 1e9;
                let override_lam = self.wallet_target_lamports_override.load(Ordering::Relaxed);
                let target_sol = if override_lam > 0 {
                    override_lam as f64 / 1e9
                } else {
                    self.config.wallet_target_sol
                };
                if target_sol > 0.0 && approx_wallet_sol < target_sol {
                    // Below target → use the wider rug-only floor
                    self.config.profit_first_floor_pct
                } else {
                    stop_loss_pct
                }
            } else {
                stop_loss_pct
            };
            let mut stop_loss_amount =
                (self.entry_amount_raw as f64 * (1.0 - effective_stop_loss_pct / 100.0)) as u64;

            // Fan out the three balance reads concurrently with a 2 s wall-clock cap.
            // Without this cap, a single stalled RPC would freeze the whole sell loop —
            // the position would sit through any pump/dump without ever re-evaluating
            // TP/SL until `max_checks` finally elapsed (potentially minutes).
            const SELL_POLL_DEADLINE: tokio::time::Duration = tokio::time::Duration::from_secs(2);
            let fetch_fut = async {
                tokio::join!(
                    self.rpc.get_token_account_balance(&base_ata),
                    self.rpc.get_token_account_balance(&pool.quote_vault),
                    self.rpc.get_token_account_balance(&pool.base_vault),
                )
            };
            let (ata_res, qb_res, bb_res) =
                match tokio::time::timeout(SELL_POLL_DEADLINE, fetch_fut).await {
                    Ok(t) => t,
                    Err(_) => {
                        tracing::debug!(
                            mint = %pool.base_mint,
                            "Sell-monitor balance fetch timed out — skipping this iteration"
                        );
                        checks += 1;
                        if checks >= max_checks {
                            break;
                        }
                        let sleep_dur = if checks < fast_phase_checks {
                            fast_interval
                        } else {
                            normal_interval
                        };
                        tokio::time::sleep(sleep_dur).await;
                        continue;
                    }
                };
            match ata_res {
                Ok(balance) => {
                    let amount: u64 = balance.amount.parse().unwrap_or(0);
                    if amount == 0 {
                        consecutive_zero_balance += 1;
                        if consecutive_zero_balance >= ZERO_BALANCE_EXIT_THRESHOLD {
                            tracing::info!(
                                mint = %pool.base_mint,
                                consecutive_zero_balance,
                                "Token balance zero for {} consecutive checks — exiting monitor",
                                ZERO_BALANCE_EXIT_THRESHOLD
                            );
                            break;
                        }
                        // Grace period: skip the rest of this iteration but stay in loop
                        checks += 1;
                        if checks >= max_checks {
                            break;
                        }
                        let sleep_dur = if checks < fast_phase_checks {
                            fast_interval
                        } else {
                            normal_interval
                        };
                        tokio::time::sleep(sleep_dur).await;
                        continue;
                    }
                    consecutive_zero_balance = 0;

                    if let (Ok(qb), Ok(bb)) = (qb_res, bb_res) {
                        let q: u64 = qb.amount.parse().unwrap_or(1);
                        let b: u64 = bb.amount.parse().unwrap_or(1);
                        // Use correct AMM formula with 0.25% fee — more accurate than naive ratio
                        let current_value = amm_out(amount, b, q);

                        // ── Fibonacci momentum update and exit evaluation ───────────
                        let current_time_secs = chrono::Utc::now().timestamp() as u64;
                        let _fib_signal = fib_momentum.update(current_value, current_time_secs, q);

                        let fib_exit = self.fib_system.evaluate_exit(
                            &fib_momentum,
                            current_value,
                            current_time_secs,
                            q,
                        );

                        if fib_exit.should_exit {
                            let pnl_lamports = current_value as i64 - self.entry_amount_raw as i64;
                            let pnl_sol = pnl_lamports as f64 / 1e9;
                            let hold_time = current_time_secs - entry_unix_secs as u64;

                            tracing::info!(
                                mint = %pool.base_mint,
                                reason = %fib_exit.exit_reason,
                                expected_pnl_pct = fib_exit.expected_pnl_pct,
                                actual_pnl_sol = pnl_sol,
                                is_dead_pool = fib_exit.is_dead_pool,
                                hold_time_secs = hold_time,
                                "🎯 Fibonacci exit triggered"
                            );

                            self.sell_with_retry(
                                &pool,
                                &base_ata,
                                amount,
                                position_started.elapsed().as_secs_f64(),
                                "fibonacci",
                            )
                            .await;
                            self.fib_stats
                                .lock()
                                .record_exit(&fib_exit, hold_time, pnl_lamports);
                            self.record_sell_outcome(
                                pnl_lamports > 0,
                                pnl_lamports,
                                pnl_sol,
                                &pool.base_mint.to_string(),
                            )
                            .await;

                            // Log Fibonacci stats every 10 exits
                            let stats = self.fib_stats.lock();
                            let total_exits = stats.dead_pool_exits
                                + stats.fibonacci_tp_exits
                                + stats.golden_retrace_exits;
                            if total_exits > 0 && total_exits % 10 == 0 {
                                tracing::info!(
                                    "📊 Fibonacci Recovery Stats: Entries={} ({:.0}% high-score) | Win Rate={:.1}% | Avg PnL={:.4} SOL | Avg Hold={:.1}s | Dead Pool Exits={} | Fib TP Exits={} | Total PnL={:.4} SOL",
                                    stats.total_entries,
                                    (stats.fibonacci_entries as f64 / stats.total_entries.max(1) as f64 * 100.0),
                                    stats.win_rate() * 100.0,
                                    stats.avg_pnl_sol(),
                                    stats.avg_hold_time_secs,
                                    stats.dead_pool_exits,
                                    stats.fibonacci_tp_exits,
                                    stats.total_pnl_lamports as f64 / 1e9,
                                );
                            }

                            break;
                        }

                        // Momentum: track consecutive declines for dump detection
                        if current_value < prev_value {
                            decline_streak += 1;
                        } else {
                            decline_streak = 0;
                        }

                        // Capture entry quote vault on the first valid check
                        if entry_q == 0 {
                            entry_q = q;
                        }

                        // ── Swell signal: net SOL flow into pool ────────────────
                        // Positive avg = active buys still arriving (pool swelling).
                        // Negative avg = sells / LP drain dominating (pool dumping).
                        if prev_q > 0 {
                            swell_window.push_back(q as i64 - prev_q as i64);
                            if swell_window.len() > swell_win_cap {
                                swell_window.pop_front();
                            }
                        }
                        let swell_avg: i64 = if swell_window.is_empty() {
                            0
                        } else {
                            swell_window.iter().sum::<i64>() / swell_window.len() as i64
                        };
                        let _pool_is_draining = swell_avg < 0;
                        let current_pnl_pct_raw = (current_value as f64
                            - self.entry_amount_raw as f64)
                            / self.entry_amount_raw as f64
                            * 100.0;
                        // Exit gate: block ALL momentum/timing exits below the initial TP
                        // so every profitable exit is >= min_profit_floor (0.05 SOL).
                        // Hard SL and no-pump timeout are exempt (they protect against rugs).
                        let exit_gate_met = current_pnl_pct_raw >= initial_tp_pct;

                        // ── Whale exit: single-check vault drop ─────────────────
                        if self.config.whale_exit_vault_drop_pct > 0.0
                            && prev_q > 0
                            && q < prev_q
                            && (current_pnl_pct_raw < 0.0 || exit_gate_met)
                        {
                            let vault_drop_pct = (prev_q - q) as f64 / prev_q as f64 * 100.0;
                            if vault_drop_pct >= self.config.whale_exit_vault_drop_pct {
                                let pnl_lamports =
                                    current_value as i64 - self.entry_amount_raw as i64;
                                let pnl_sol = pnl_lamports as f64 / 1e9;
                                tracing::warn!(
                                    mint = %pool.base_mint,
                                    vault_drop_pct = %format!("{:.1}%", vault_drop_pct),
                                    threshold = self.config.whale_exit_vault_drop_pct,
                                    pnl_sol,
                                    "🐋 Whale exit: vault drop detected — selling immediately"
                                );
                                self.sell_with_retry(
                                    &pool,
                                    &base_ata,
                                    amount,
                                    position_started.elapsed().as_secs_f64(),
                                    "dump_detected",
                                )
                                .await;
                                self.record_sell_outcome(
                                    pnl_lamports > 0,
                                    pnl_lamports,
                                    pnl_sol,
                                    &pool.base_mint.to_string(),
                                )
                                .await;
                                break;
                            }
                        }
                        prev_q = q;

                        // ── Volume exhaustion exit ───────────────────────────────
                        // Only fires when in profit — if we're down, don't compound
                        // a loss by selling into low volume (may be a temporary dip).
                        if self.config.volume_exhaustion_pct > 0.0
                            && entry_q > 0
                            && current_value > self.entry_amount_raw
                            && exit_gate_met
                        {
                            let exhaustion_floor = (entry_q as f64
                                * (1.0 - self.config.volume_exhaustion_pct / 100.0))
                                as u64;
                            if q < exhaustion_floor {
                                let pnl_lamports =
                                    current_value as i64 - self.entry_amount_raw as i64;
                                let pnl_sol = pnl_lamports as f64 / 1e9;
                                tracing::info!(
                                    mint = %pool.base_mint,
                                    entry_q, current_q = q, exhaustion_floor,
                                    pnl_sol,
                                    "Volume exhaustion exit — locking profit before liquidity dries up"
                                );
                                self.sell_with_retry(
                                    &pool,
                                    &base_ata,
                                    amount,
                                    position_started.elapsed().as_secs_f64(),
                                    "volume_exhaustion",
                                )
                                .await;
                                self.record_sell_outcome(
                                    true,
                                    pnl_lamports,
                                    pnl_sol,
                                    &pool.base_mint.to_string(),
                                )
                                .await;
                                break;
                            }
                        }

                        // Update peak (used for both trailing stop AND momentum pullback exit)
                        if current_value > peak_value {
                            peak_value = current_value;
                            last_peak_improved = std::time::Instant::now();
                        }

                        // Profit floor: once position reaches initial TP, raise SL to
                        // min_profit_floor so every exit is >= the 0.05 SOL target.
                        if exit_gate_met && stop_loss_amount < min_profit_floor_lamports {
                            stop_loss_amount = min_profit_floor_lamports;
                        }
                        // Trailing stop: only activates at/above TP to prevent sub-entry exits.
                        // Uses configured trailing_stop_loss_pct (wide for 1000x riding).
                        if trailing_enabled && exit_gate_met {
                            let trail = (peak_value as f64
                                * (1.0 - self.config.trailing_stop_loss_pct / 100.0))
                                as u64;
                            if trail > stop_loss_amount {
                                stop_loss_amount = trail;
                            }
                        }

                        // Flash-crash detector: if a SINGLE check shows a value drop
                        // ≥ flash_crash_pct from the previous check's value AND we've
                        // already had at least 3 stabilising checks (avoids false-
                        // positives on the fill price vs entry discrepancy), exit now.
                        // This fires before the 3-consecutive-decline counter can even
                        // accumulate — crucial for vertical dumps (rug/snipe target).
                        if self.config.flash_crash_pct > 0.0
                            && checks >= 3
                            && prev_value > current_value
                            && (current_pnl_pct_raw < 0.0 || exit_gate_met)
                        {
                            let single_drop_pct = (prev_value as f64 - current_value as f64)
                                / self.entry_amount_raw as f64
                                * 100.0;
                            if single_drop_pct >= self.config.flash_crash_pct {
                                let pnl_lamports =
                                    current_value as i64 - self.entry_amount_raw as i64;
                                let pnl_sol = pnl_lamports as f64 / 1e9;
                                tracing::warn!(
                                    mint = %pool.base_mint,
                                    single_drop_pct = %format!("{:.1}%", single_drop_pct),
                                    pnl_sol,
                                    "⚡ Flash-crash detected — emergency exit"
                                );
                                self.sell_with_retry(
                                    &pool,
                                    &base_ata,
                                    amount,
                                    position_started.elapsed().as_secs_f64(),
                                    "dump_detected",
                                )
                                .await;
                                self.record_sell_outcome(
                                    pnl_lamports > 0,
                                    pnl_lamports,
                                    pnl_sol,
                                    &pool.base_mint.to_string(),
                                )
                                .await;
                                break;
                            }
                        }

                        // Profit-lock: after N consecutive checks above entry, raise the
                        // SL floor to near-breakeven so a sustained winner can't round-trip.
                        if self.config.profit_lock_checks > 0 {
                            if current_value > self.entry_amount_raw {
                                profit_lock_counter += 1;
                                if profit_lock_counter >= self.config.profit_lock_checks
                                    && stop_loss_amount < profit_lock_floor
                                {
                                    stop_loss_amount = profit_lock_floor;
                                    tracing::info!(
                                        mint = %pool.base_mint,
                                        checks = profit_lock_counter,
                                        floor_sol = profit_lock_floor as f64 / 1e9,
                                        "🔒 Profit lock engaged — SL raised to near-breakeven"
                                    );
                                }
                            } else {
                                profit_lock_counter = 0;
                            }
                        }

                        // ── Momentum-aware long-term sniping ──────────────────
                        // Capture velocity = per-check % change vs entry, averaged
                        // over the configured window. This is the signal driving
                        // both TP escalation (let winners run) and the pullback
                        // exit (lock in before round-trip).
                        if self.config.momentum_hold {
                            let delta_pct = (current_value as f64 - prev_value as f64)
                                / self.entry_amount_raw as f64
                                * 100.0;
                            velocity_window.push_back(delta_pct);
                            while velocity_window.len() > momentum_window_cap {
                                velocity_window.pop_front();
                            }

                            let avg_velocity: f64 = if velocity_window.is_empty() {
                                0.0
                            } else {
                                velocity_window.iter().sum::<f64>() / velocity_window.len() as f64
                            };

                            let current_pnl_pct = (current_value as f64
                                - self.entry_amount_raw as f64)
                                / self.entry_amount_raw as f64
                                * 100.0;
                            let peak_pnl_pct = (peak_value as f64 - self.entry_amount_raw as f64)
                                / self.entry_amount_raw as f64
                                * 100.0;
                            let pullback_pct = peak_pnl_pct - current_pnl_pct;

                            // (A) TP escalation: position is at/above current TP, momentum
                            //     is still strong → raise TP rather than exit.
                            //
                            // Window requirement: 1 sample minimum (not full window).
                            // Fast pumps reach TP in the first check — waiting for 5 samples
                            // means the sell fires before escalation ever runs. A single check
                            // showing strong velocity (or a direct jump 50%+ past TP) is
                            // sufficient evidence to escalate.
                            //
                            // CRITICAL: target_profit is updated immediately after escalation
                            // so the TP check at the bottom of this iteration uses the NEW
                            // threshold. Without this, the bot escalated TP to 315% then
                            // immediately sold at 175% because target_profit was stale.
                            let single_jump = current_pnl_pct >= dynamic_tp_pct + 50.0;
                            let velocity_ok = avg_velocity > mom_threshold || single_jump;
                            if current_pnl_pct >= dynamic_tp_pct
                                && velocity_ok
                                && escalation_count < mom_max_esc
                                && !velocity_window.is_empty()
                            {
                                let new_tp = dynamic_tp_pct * mom_factor;
                                tracing::info!(
                                    mint = %pool.base_mint,
                                    old_tp_pct = %format!("{:.0}%", dynamic_tp_pct),
                                    new_tp_pct = %format!("{:.0}%", new_tp),
                                    avg_velocity_pct = %format!("{:.2}%", avg_velocity),
                                    escalation = escalation_count + 1,
                                    single_jump,
                                    moon_chase,
                                    "🚀 TP escalated — momentum strong, holding for bigger move"
                                );
                                dynamic_tp_pct = new_tp;
                                escalation_count += 1;
                                // Refresh target_profit so the TP check at the bottom of THIS
                                // iteration uses the new (higher) threshold — not the stale one.
                                target_profit = (self.entry_amount_raw as f64
                                    * (1.0 + dynamic_tp_pct / 100.0))
                                    as u64;
                            }

                            // (B) Adaptive pullback-from-peak exit. The threshold
                            //     scales with peak height: bigger winners get more
                            //     room to breathe before the exit fires.
                            //         θ_eff = base × sqrt(1 + peak/100)
                            //     peak=20%  → 1.10× base.
                            //     peak=100% → 1.41× base.
                            //     peak=500% → 2.45× base.
                            //     This stops the bot dumping a parabolic move on a
                            //     normal wiggle while still locking in on real reversals.
                            let pullback_eff = if self.config.adaptive_pullback {
                                mom_pullback * (1.0 + peak_pnl_pct.max(0.0) / 100.0).sqrt()
                            } else {
                                mom_pullback
                            };
                            if exit_gate_met
                                && peak_pnl_pct >= self.config.momentum_min_peak_pct
                                && pullback_pct >= pullback_eff
                            {
                                let pnl_lamports =
                                    current_value as i64 - self.entry_amount_raw as i64;
                                let pnl_sol = pnl_lamports as f64 / 1e9;
                                tracing::warn!(
                                    mint = %pool.base_mint,
                                    peak_pnl_pct = %format!("{:.1}%", peak_pnl_pct),
                                    current_pnl_pct = %format!("{:.1}%", current_pnl_pct),
                                    pullback_pct = %format!("{:.1}%", pullback_pct),
                                    pullback_eff = %format!("{:.1}%", pullback_eff),
                                    pnl_sol,
                                    "📉 Adaptive pullback exit — locking gains before reversal"
                                );
                                self.sell_with_retry(
                                    &pool,
                                    &base_ata,
                                    amount,
                                    position_started.elapsed().as_secs_f64(),
                                    "trailing_stop",
                                )
                                .await;
                                self.record_sell_outcome(
                                    pnl_lamports > 0,
                                    pnl_lamports,
                                    pnl_sol,
                                    &pool.base_mint.to_string(),
                                )
                                .await;
                                break;
                            }

                            // (C) Velocity-decay exit — the "perfect exit" trigger.
                            //     Catches the velocity inflection point: when the
                            //     last N checks show meaningfully slower upward
                            //     motion than the previous N checks AND we're in
                            //     profit, momentum is dying — exit before price
                            //     actually rolls over. Acts on the SECOND derivative
                            //     of price, ~1-2 ticks before the trailing stop or
                            //     pullback exit would fire.
                            if self.config.velocity_decay_exit
                                && exit_gate_met
                                && current_pnl_pct >= self.config.velocity_decay_min_pnl_pct
                            {
                                decay_window.push_back(delta_pct);
                                while decay_window.len() > decay_window_cap {
                                    decay_window.pop_front();
                                }
                                if decay_window.len() == decay_window_cap {
                                    let prev_half_avg: f64 =
                                        decay_window.iter().take(decay_half).sum::<f64>()
                                            / decay_half as f64;
                                    let recent_half_avg: f64 =
                                        decay_window.iter().skip(decay_half).sum::<f64>()
                                            / decay_half as f64;
                                    let decay_drop = prev_half_avg - recent_half_avg;
                                    // Fire when velocity has fallen meaningfully
                                    // AND the previous half was still upward
                                    // (prevents firing after a recovery from a dip).
                                    if decay_drop >= self.config.velocity_decay_drop_threshold
                                        && prev_half_avg > 0.0
                                    {
                                        let pnl_lamports =
                                            current_value as i64 - self.entry_amount_raw as i64;
                                        let pnl_sol = pnl_lamports as f64 / 1e9;
                                        tracing::info!(
                                            mint = %pool.base_mint,
                                            current_pnl_pct = %format!("{:.1}%", current_pnl_pct),
                                            peak_pnl_pct = %format!("{:.1}%", peak_pnl_pct),
                                            prev_half_v = %format!("{:.2}%", prev_half_avg),
                                            recent_half_v = %format!("{:.2}%", recent_half_avg),
                                            decay_drop = %format!("{:.2}%", decay_drop),
                                            pnl_sol,
                                            "🎯 Velocity-decay exit — momentum inflection caught"
                                        );
                                        self.sell_with_retry(
                                            &pool,
                                            &base_ata,
                                            amount,
                                            position_started.elapsed().as_secs_f64(),
                                            "velocity_decay",
                                        )
                                        .await;
                                        self.record_sell_outcome(
                                            pnl_lamports > 0,
                                            pnl_lamports,
                                            pnl_sol,
                                            &pool.base_mint.to_string(),
                                        )
                                        .await;
                                        break;
                                    }
                                }
                            }
                        }

                        // Update the live-position registry so the dashboard sees
                        // current value / peak / dynamic TP / SL / decline streak
                        // without needing its own RPC reads. One tiny write per check.
                        if let Some(mut entry) = self.live_positions.get_mut(&pos_key) {
                            entry.current_value_lamports = current_value;
                            entry.peak_value_lamports = peak_value;
                            entry.dynamic_tp_pct = dynamic_tp_pct;
                            entry.escalations = escalation_count;
                            entry.last_check_unix_secs = chrono::Utc::now().timestamp();
                            entry.current_sl_lamports = stop_loss_amount;
                            entry.current_sl_pct = (stop_loss_amount as f64
                                - self.entry_amount_raw as f64)
                                / self.entry_amount_raw as f64
                                * 100.0;
                            entry.decline_streak = decline_streak;
                        }

                        prev_value = current_value;

                        // Dead-zone early exit: live data shows all profitable pump.fun
                        // trades exit within 6 s of entry. A position that never reached
                        // `no_pump_min_gain_pct` after `no_pump_timeout_secs` is dead
                        // capital — exit and redeploy rather than holding through the
                        // full monitoring window at a -0.5% AMM-spread loss.
                        if self.config.no_pump_timeout_secs > 0 {
                            let age_secs = position_started.elapsed().as_secs();
                            if age_secs >= self.config.no_pump_timeout_secs {
                                let peak_pnl_pct = (peak_value as f64
                                    - self.entry_amount_raw as f64)
                                    / self.entry_amount_raw as f64
                                    * 100.0;
                                if peak_pnl_pct < self.config.no_pump_min_gain_pct {
                                    let pnl_lamports =
                                        current_value as i64 - self.entry_amount_raw as i64;
                                    let pnl_sol = pnl_lamports as f64 / 1e9;
                                    tracing::info!(
                                        mint = %pool.base_mint,
                                        age_secs,
                                        peak_pnl_pct = %format!("{:.2}%", peak_pnl_pct),
                                        threshold_pct = self.config.no_pump_min_gain_pct,
                                        pnl_sol,
                                        "No pump detected — recycling dead position"
                                    );
                                    self.sell_with_retry(
                                        &pool,
                                        &base_ata,
                                        amount,
                                        position_started.elapsed().as_secs_f64(),
                                        "no_pump_timeout",
                                    )
                                    .await;
                                    self.record_sell_outcome(
                                        pnl_lamports > 0,
                                        pnl_lamports,
                                        pnl_sol,
                                        &pool.base_mint.to_string(),
                                    )
                                    .await;
                                    break;
                                }
                            }
                        }

                        // Tiered partial-TP ladder (v0.9.6) — preferred over the
                        // legacy single partial when `tiered_partial_tp` is on.
                        // Sells `sell_pct` of REMAINING position at each trigger
                        // level. After the first level fires, stop_loss moves to
                        // breakeven to protect the locked-in gain.
                        //
                        // v1.0.0 fix: track `remaining_tokens` so that if multiple
                        // tiers fire in a single check (e.g. price jumped from 20%→90%
                        // in one 75ms poll), each level sells the correct fraction of
                        // the remaining balance rather than all selling against the
                        // same starting `amount` and over-selling.
                        if tiered_enabled {
                            let current_pnl_pct_now = (current_value as f64
                                - self.entry_amount_raw as f64)
                                / self.entry_amount_raw as f64
                                * 100.0;
                            let mut any_fired_this_check = false;
                            let mut remaining_tokens = amount;
                            for (i, (trigger_pct, sell_pct)) in
                                self.config.tiered_partial_tp_levels.iter().enumerate()
                            {
                                if tiered_fired[i] || current_pnl_pct_now < *trigger_pct {
                                    continue;
                                }
                                let partial_amount =
                                    (remaining_tokens as f64 * sell_pct / 100.0) as u64;
                                if partial_amount == 0 {
                                    tiered_fired[i] = true;
                                    continue;
                                }
                                tracing::info!(
                                    mint = %pool.base_mint,
                                    level = i + 1,
                                    trigger_pct,
                                    sell_pct,
                                    remaining_tokens,
                                    partial_amount,
                                    current_pnl_pct = %format!("{:.1}%", current_pnl_pct_now),
                                    "🎯 Tiered partial TP — locking layer"
                                );
                                self.sell_with_retry(
                                    &pool,
                                    &base_ata,
                                    partial_amount,
                                    position_started.elapsed().as_secs_f64(),
                                    "tiered_tp",
                                )
                                .await;
                                remaining_tokens = remaining_tokens.saturating_sub(partial_amount);
                                tiered_fired[i] = true;
                                any_fired_this_check = true;
                                // Move stop to breakeven after the FIRST tier fires.
                                if i == 0 {
                                    stop_loss_amount = self.entry_amount_raw;
                                }
                            }
                            // Set legacy partial_tp_done so the fallback path below
                            // doesn't fire on top of the tiered ladder.
                            if any_fired_this_check {
                                partial_tp_done = true;
                                prev_value = current_value;
                                // Re-read the balance on the next iteration before making
                                // additional exit decisions. Without this, the main TP check
                                // below fires in the SAME iteration against the pre-partial
                                // token balance, causing a double-sell (partial + full exit).
                                checks += 1;
                                if checks >= max_checks {
                                    break;
                                }
                                let sleep_dur = if checks < fast_phase_checks {
                                    fast_interval
                                } else {
                                    normal_interval
                                };
                                tokio::time::sleep(sleep_dur).await;
                                continue;
                            }
                        } else if partial_tp_enabled
                            && !partial_tp_done
                            && current_value >= partial_tp_target
                        {
                            // Legacy single partial-TP path (fallback when tiered disabled)
                            let partial_amount =
                                (amount as f64 * self.config.partial_tp_pct / 100.0) as u64;
                            if partial_amount > 0 {
                                tracing::info!(mint = %pool.base_mint, "Partial TP triggered — selling {}%", self.config.partial_tp_pct);
                                self.sell_with_retry(
                                    &pool,
                                    &base_ata,
                                    partial_amount,
                                    position_started.elapsed().as_secs_f64(),
                                    "take_profit",
                                )
                                .await;
                                partial_tp_done = true;
                                stop_loss_amount = self.entry_amount_raw; // move stop to breakeven
                            }
                        }

                        // Dump detection: 3 consecutive declining checks after fast phase → exit.
                        // In profit-first mode we suppress this trigger unless the position
                        // is BOTH down meaningfully AND past the rug floor — otherwise the
                        // bot bails on every dip and never books any wins.
                        let dump_eligible = if self.config.profit_first_mode {
                            let override_lam =
                                self.wallet_target_lamports_override.load(Ordering::Relaxed);
                            let target_lam = if override_lam > 0 {
                                override_lam as i64
                            } else {
                                (self.config.wallet_target_sol * 1e9) as i64
                            };
                            let approx_wallet = self.session_start_lamports.load(Ordering::Relaxed)
                                as i64
                                + *self.daily_pnl_lamports.lock();
                            if target_lam > 0 && approx_wallet < target_lam {
                                // Approx-wallet < target → only honor dump-detection if we're already
                                // at or past the rug-only floor. Anything shallower, hold and wait.
                                current_value <= stop_loss_amount
                            } else {
                                true
                            }
                        } else {
                            true
                        };
                        if decline_streak >= 3
                            && checks >= fast_phase_checks
                            && dump_eligible
                            && (current_pnl_pct_raw < 0.0 || exit_gate_met)
                        {
                            let pnl_lamports = current_value as i64 - self.entry_amount_raw as i64;
                            let pnl_sol = pnl_lamports as f64 / 1e9;
                            tracing::warn!(
                                mint = %pool.base_mint,
                                decline_streak, current_value, pnl_sol,
                                "Dump momentum detected — exiting position"
                            );
                            self.sell_with_retry(
                                &pool,
                                &base_ata,
                                amount,
                                position_started.elapsed().as_secs_f64(),
                                "dump_detected",
                            )
                            .await;
                            self.record_sell_outcome(
                                pnl_lamports > 0,
                                pnl_lamports,
                                pnl_sol,
                                &pool.base_mint.to_string(),
                            )
                            .await;
                            break;
                        }

                        // Peak stagnation exit: pool pumped once then went flat. If peak
                        // hasn't made a new high in `peak_stagnation_secs` AND the position
                        // is still above `peak_stagnation_min_pnl_pct`, exit now rather than
                        // waiting for a slow bleed back through the profit floor to sub-floor
                        // prices. This eliminates the 7-11 min 99%-exit pattern where velocity
                        // decay is disarmed once current drops below velocity_decay_min_pnl_pct.
                        if self.config.peak_stagnation_secs > 0
                            && last_peak_improved.elapsed().as_secs()
                                >= self.config.peak_stagnation_secs
                            && current_pnl_pct_raw >= self.config.peak_stagnation_min_pnl_pct
                        {
                            let pnl_lamports = current_value as i64 - self.entry_amount_raw as i64;
                            let pnl_sol = pnl_lamports as f64 / 1e9;
                            tracing::info!(
                                mint = %pool.base_mint,
                                stagnant_secs = last_peak_improved.elapsed().as_secs(),
                                current_pnl_pct = %format!("{:.1}%", current_pnl_pct_raw),
                                peak_pnl_pct = %format!("{:.1}%", (peak_value as f64 - self.entry_amount_raw as f64) / self.entry_amount_raw as f64 * 100.0),
                                pnl_sol,
                                "⏱ Peak stagnation exit — pool flat, recycling capital"
                            );
                            self.sell_with_retry(
                                &pool,
                                &base_ata,
                                amount,
                                position_started.elapsed().as_secs_f64(),
                                "peak_stagnation",
                            )
                            .await;
                            self.record_sell_outcome(
                                pnl_lamports > 0,
                                pnl_lamports,
                                pnl_sol,
                                &pool.base_mint.to_string(),
                            )
                            .await;
                            break;
                        }

                        if current_value >= target_profit || current_value <= stop_loss_amount {
                            let profitable = current_value >= target_profit;
                            let pnl_lamports = current_value as i64 - self.entry_amount_raw as i64;
                            let pnl_sol = pnl_lamports as f64 / 1e9;
                            let exit_r = if profitable {
                                "take_profit"
                            } else {
                                "stop_loss"
                            };
                            tracing::info!(
                                mint = %pool.base_mint,
                                current_value, target_profit,
                                stop_loss = stop_loss_amount, pnl_sol,
                                peak_value,
                                "Sell triggered: {}", exit_r
                            );
                            self.sell_with_retry(
                                &pool,
                                &base_ata,
                                amount,
                                position_started.elapsed().as_secs_f64(),
                                exit_r,
                            )
                            .await;
                            self.record_sell_outcome(
                                profitable,
                                pnl_lamports,
                                pnl_sol,
                                &pool.base_mint.to_string(),
                            )
                            .await;
                            break;
                        }
                    }
                }
                Err(_) => {}
            }

            checks += 1;
            if checks >= max_checks {
                // Profit-first force-sell gate: when active AND below wallet target,
                // peek at current value first. If we're at a loss inside the rug floor,
                // extend the watch window by another full price_check_duration rather
                // than dumping into a dip — the whole point of profit-first is to wait
                // for green.
                let extend_for_recovery = if self.config.profit_first_mode {
                    let start_lam = self.session_start_lamports.load(Ordering::Relaxed);
                    let daily_pnl = *self.daily_pnl_lamports.lock();
                    let approx_wallet_sol = (start_lam as i64 + daily_pnl) as f64 / 1e9;
                    let override_lam = self.wallet_target_lamports_override.load(Ordering::Relaxed);
                    let target_sol = if override_lam > 0 {
                        override_lam as f64 / 1e9
                    } else {
                        self.config.wallet_target_sol
                    };
                    if target_sol > 0.0 && approx_wallet_sol < target_sol {
                        // prev_value tracked by the inner loop is the most recent estimate
                        let above_rug_floor = prev_value > stop_loss_amount;
                        let in_loss = prev_value < self.entry_amount_raw;
                        // Hold if we're in a small loss but not yet at the rug floor.
                        above_rug_floor && in_loss
                    } else {
                        false
                    }
                } else {
                    false
                };

                if extend_for_recovery {
                    tracing::info!(
                        mint = %pool.base_mint,
                        prev_value, entry = self.entry_amount_raw,
                        "Profit-first hold: window expired in shallow loss — extending watch",
                    );
                    // Reset the counter so we get another full window. Cap total hold at
                    // 6× the configured window so we don't sit on a dead pool forever.
                    let cap = 6u64;
                    if checks < cap * max_checks {
                        checks = checks.saturating_sub(normal_checks);
                        let sleep_dur = normal_interval;
                        tokio::time::sleep(sleep_dur).await;
                        continue;
                    }
                    tracing::warn!(
                        mint = %pool.base_mint,
                        "Profit-first hold cap reached — force-selling to free capital",
                    );
                }

                tracing::info!(mint = %pool.base_mint, "Price check window expired — force selling");
                if let Ok(balance) = self.rpc.get_token_account_balance(&base_ata).await {
                    let amount: u64 = balance.amount.parse().unwrap_or(0);
                    if amount > 0 {
                        self.sell_with_retry(
                            &pool,
                            &base_ata,
                            amount,
                            position_started.elapsed().as_secs_f64(),
                            "timeout",
                        )
                        .await;
                        // Parallel reserve fetch for PnL estimate.
                        let (qb_res, bb_res) = tokio::join!(
                            self.rpc.get_token_account_balance(&pool.quote_vault),
                            self.rpc.get_token_account_balance(&pool.base_vault),
                        );
                        let (pnl_lam, pnl_sol) = if let (Ok(qb), Ok(bb)) = (qb_res, bb_res) {
                            let q = qb.amount.parse::<u64>().unwrap_or(1);
                            let b = bb.amount.parse::<u64>().unwrap_or(1);
                            let val = amm_out(amount, b, q) as i64;
                            let lam = val - self.entry_amount_raw as i64;
                            (lam, lam as f64 / 1e9)
                        } else {
                            let lam = -(self.entry_amount_raw as i64 / 50);
                            (lam, lam as f64 / 1e9)
                        };
                        self.record_sell_outcome(
                            pnl_lam > 0,
                            pnl_lam,
                            pnl_sol,
                            &pool.base_mint.to_string(),
                        )
                        .await;
                    }
                }
                break;
            }
            // Fast-poll early; accelerate on consecutive declines; normal rate otherwise
            let sleep_dur = if checks < fast_phase_checks {
                fast_interval
            } else if self.config.check_interval_acceleration && decline_streak >= 3 {
                // Halve the interval (floor 25ms) when consecutive declines signal a dump
                let acc_ms = (self.config.price_check_interval_ms / 2).max(25);
                tokio::time::Duration::from_millis(acc_ms)
            } else {
                normal_interval
            };
            tokio::time::sleep(sleep_dur).await;
        }

        // After exit: close the base token ATA if the balance is now zero.
        // Recovering ~0.002 SOL rent per trade adds up over many positions.
        // The close is fire-and-forget — failure is logged but doesn't block exit.
        if self.config.close_ata_on_sell {
            if let Ok(bal) = self.rpc.get_token_account_balance(&base_ata).await {
                if bal.amount.parse::<u64>().unwrap_or(1) == 0 {
                    self.close_base_ata_background(pool.base_mint, base_ata);
                }
            }
        }

        // Position closed — decrement counter. fetch_sub returns the previous value,
        // so a return of 1 means we just dropped to 0 open positions.
        let prev_open = self.open_positions.fetch_sub(1, Ordering::Relaxed);
        if prev_open == 1 {
            // All positions are closed. If buy_limit auto-tripped sell mode, undo it
            // so a fresh trading session can begin: reset the buy counter, delete the
            // sell-mode file (the main-loop watcher will then flip sell_mode → false),
            // and clear the atomic immediately so the next pool isn't gated.
            //
            // We deliberately only auto-clear OUR own trigger (reason=="buy_limit").
            // Dashboard- or drawdown-triggered sell modes carry different reasons and
            // must persist until the user / operator clears them.
            let sell_mode_path = artifact_path(SELL_MODE_FILE);
            let was_buy_limit = std::fs::read_to_string(&sell_mode_path)
                .map(|s| s.contains("buy_limit"))
                .unwrap_or(false);
            if was_buy_limit {
                let prev = self.buy_count.swap(0, Ordering::Relaxed);
                self.sell_mode.store(false, Ordering::Relaxed);
                let _ = std::fs::remove_file(sell_mode_path);
                tracing::info!(
                    cleared_buys = prev,
                    "All positions closed — buy counter reset, sell-mode lifted (buy_limit cycle complete)",
                );
            } else {
                tracing::debug!("All positions closed (sell_mode external — not auto-resetting)");
            }
        }
    }

    /// Fire-and-forget: close the base token ATA after a full sell to reclaim rent.
    /// Waits 3 s for the sell to fully confirm on-chain, then sends a close_account
    /// instruction. Failure is logged at DEBUG so it never pollutes the main log.
    fn close_base_ata_background(&self, base_mint: Pubkey, base_ata: Pubkey) {
        let rpc = self.rpc.clone();
        let wallet = self.wallet.clone();
        let mint_str = base_mint.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            let wallet_pk = wallet.pubkey();
            // Try SPL Token close; pump.fun Token-2022 accounts will fail silently.
            let close_ix = match spl_token::instruction::close_account(
                &spl_token::id(),
                &base_ata,
                &wallet_pk,
                &wallet_pk,
                &[],
            ) {
                Ok(ix) => ix,
                Err(e) => {
                    tracing::debug!(mint = %mint_str, "ATA close ix build failed: {}", e);
                    return;
                }
            };
            let blockhash = match rpc.get_latest_blockhash().await {
                Ok(b) => b,
                Err(e) => {
                    tracing::debug!(mint = %mint_str, "ATA close: blockhash fetch failed: {}", e);
                    return;
                }
            };
            let msg = solana_sdk::message::Message::new_with_blockhash(
                &[close_ix],
                Some(&wallet_pk),
                &blockhash,
            );
            let tx = solana_sdk::transaction::Transaction::new(&[&*wallet], msg, blockhash);
            match rpc.send_transaction(&tx).await {
                Ok(sig) => {
                    tracing::info!(mint = %mint_str, %sig, "ATA closed — ~0.002 SOL rent reclaimed")
                }
                Err(e) => {
                    tracing::debug!(mint = %mint_str, "ATA close failed (may be Token-2022): {}", e)
                }
            }
        });
    }

    /// Atomically append a mint to the blacklist file so it is never bought again.
    fn auto_blacklist_mint(path: &str, mint: &str) {
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        if existing.lines().any(|l| l.trim() == mint) {
            return;
        }
        let new_content = format!("{}\n{}\n", existing.trim_end(), mint);
        let tmp = format!("{}.tmp", path);
        if std::fs::write(&tmp, &new_content).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
        tracing::warn!(mint = %&mint[..12.min(mint.len())], "🚫 Auto-blacklisted rug mint");
    }

    async fn record_sell_outcome(
        &self,
        profitable: bool,
        pnl_lamports: i64,
        pnl_sol: f64,
        mint: &str,
    ) {
        // Auto-blacklist mints that rug us completely — pnl ≤ −0.001 SOL and not profitable.
        // Prevents re-buying the same rugged pool (which caused 32 -100% losses from 14 mints).
        if !profitable && pnl_lamports <= -1_000_000 {
            Self::auto_blacklist_mint(&self.config.blacklist_path, mint);
        }

        // Strategy agent history
        {
            let mut history = self.trade_history.lock();
            history.push((profitable, pnl_sol));
            if history.len() > 20 {
                history.remove(0);
            }
        }

        // Record PnL to grief-loss circuit breaker
        if let Some(ref gb) = self.grief_breaker {
            gb.record_pnl(pnl_lamports);
        }

        // Update deployer reputation ledger.
        //
        // BUG FIX (v0.9.3): the previous code used `acct.owner` of the pool
        // account, but for Raydium AMM V4 that's always the program ID
        // (675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8) — every pool shares it.
        // One rug poisoned the ledger and CrossPoolCorrelation rejected 100%
        // of subsequent pools. We now key on the mint authority instead, which
        // is the actual dev wallet. When the mint has been renounced (authority
        // is None) we fall back to the mint pubkey itself so the ledger entry
        // still exists; rugged renounced mints rarely re-appear so this fallback
        // is effectively a no-op signal.
        if let Ok(mint_pk) = mint.parse::<solana_sdk::pubkey::Pubkey>() {
            use solana_sdk::program_pack::Pack;
            let deployer = match self.rpc.get_account(&mint_pk).await {
                Ok(mint_acct) => match spl_token::state::Mint::unpack(&mint_acct.data) {
                    Ok(m) => m
                        .mint_authority
                        .map(|pk| pk.to_string())
                        .unwrap_or_else(|| mint_pk.to_string()),
                    Err(_) => mint_pk.to_string(),
                },
                Err(_) => mint_pk.to_string(),
            };
            let mut ledger = self.deployer_ledger.lock();
            if profitable {
                ledger.record_success(&deployer);
            } else {
                ledger.record_rug(&deployer);
            }
        }

        // Daily PnL accumulator + total PnL metrics. This is an *exit*, not an entry,
        // so it only accumulates PnL via record_pnl — it must NOT bump trades_confirmed
        // (the entry funnel is counted at buy-confirm time). Bumping it here is what made
        // trades_confirmed exceed trades_attempted.
        {
            let mut daily = self.daily_pnl_lamports.lock();
            *daily += pnl_lamports;
        }
        self.metrics.record_pnl(pnl_lamports);

        // Consecutive loss / win tracking. Cooldown trigger removed by operator
        // decision — we keep the loss counter for the streak display + ledger, but
        // never pause buys on it. See the matching no-op block in buy() above.
        if profitable {
            self.consecutive_losses.store(0, Ordering::Relaxed);
        } else {
            self.consecutive_losses.fetch_add(1, Ordering::Relaxed);

            // Session heat: accumulate loss timestamps and trigger cooldown if threshold hit
            if self.config.session_heat_losses > 0 && self.config.session_heat_window_secs > 0 {
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let mut ts = self.loss_heat_timestamps.lock();
                ts.push(now_secs);
                let window = self.config.session_heat_window_secs;
                ts.retain(|&t| now_secs.saturating_sub(t) < window);
                if ts.len() >= self.config.session_heat_losses as usize {
                    let cooldown_secs = self.config.session_heat_cooldown_mins as u64 * 60;
                    let until_ms = (now_secs + cooldown_secs) * 1000;
                    self.cooldown_until_ms.store(until_ms, Ordering::Relaxed);
                    warn!(
                        losses_in_window = ts.len(),
                        window_secs = window,
                        cooldown_mins = self.config.session_heat_cooldown_mins,
                        "Session heat limit reached — pausing buys for cooldown"
                    );
                }
            }
        }

        // Sell alert
        let alerts = self.alerts.clone();
        let emoji = if profitable { "✅" } else { "❌" };
        let mint_short = mint[..8.min(mint.len())].to_string();
        tokio::spawn(async move {
            alerts
                .send(
                    &format!("{} SELL Confirmed", emoji),
                    &format!("Mint: {}...\nPnL: {:.4} SOL", mint_short, pnl_sol),
                )
                .await;
        });
    }

    /// Persistent sell wrapper with escalating slippage tolerance.
    ///
    /// Round 0 (immediate): normal slippage
    /// Round 1 (after 3 s): 2× slippage — reduces tx rejection during volatile moves
    /// Round 2 (after 8 s): min_out=0 — accept any price rather than hold indefinitely
    /// Emergency (after 15 s): min_out=0 + forced re-balance refresh, last resort
    ///
    /// Refreshes the token balance before each round so stale amounts don't
    /// block later rounds. `position_age_secs` is forwarded into the TradeEvent
    /// for the NN observer; pass 0.0 from call sites that don't track entry time.
    async fn sell_with_retry(
        &self,
        pool: &CachedPool,
        base_ata: &Pubkey,
        amount: u64,
        position_age_secs: f64,
        exit_reason: &str,
    ) {
        // (delay_before_secs, sell_round passed to do_sell for slippage escalation)
        //
        // Timing tuned for rug-pull dynamics: the critical window is the first 3 s after
        // a rug triggers. Getting to min_out=0 (round 2) within 4 s total means we can
        // still exit into the residual liquidity rather than a drained pool. The old
        // schedule (3 s, 8 s, 15 s) meant round 2 started 11 s in — often too late.
        // Fast drain pre-check: if the pool quote vault is empty before we even
        // start the retry loop, every round will fail on-chain. Skip them all.
        if let Ok(qb) = self.rpc.get_token_account_balance(&pool.quote_vault).await {
            let q: u64 = qb.amount.parse().unwrap_or(u64::MAX);
            if q < DRAIN_THRESHOLD_LAMPORTS {
                tracing::warn!(
                    mint = %pool.base_mint,
                    quote_vault_lamports = q,
                    "Pool quote vault effectively drained (sell_with_retry) — writing total-loss event"
                );
                self.metrics.record_trade_failed();
                TradeEvent {
                    timestamp: chrono::Utc::now(),
                    kind: "SELL".into(),
                    mint: pool.base_mint.to_string(),
                    symbol: String::new(),
                    amount: scematica_core::token::raw_to_ui(amount, pool.base_decimals),
                    pnl: -(self.entry_amount_raw as f64) / 1_000_000_000.0,
                    status: "✗".into(),
                    signature: "pool_drained".into(),
                    dex: "Raydium".into(),
                    hops: 1,
                    pnl_pct: -100.0,
                    position_age_secs,
                    exit_reason: exit_reason.to_string(),
                    pool_size_sol: 0.0,
                    pool_age_secs: 0.0,
                    velocity_sol_per_sec: 0.0,
                    buy_pressure_ratio: 0.0,
                    pool_score: 0.0,
                    pumpfun_score: 0.0,
                    inflow_rate_sol_per_sec: 0.0,
                }
                .append_to_file(TRADES_FILE);
                return;
            }
        }

        // Sell rounds: (delay_secs, sell_round).
        // sell_round 0 = normal slippage, 1 = 2× slippage, 2+ = min_out=0.
        // Speed optimised for pump.fun rugs that drain in <3 s:
        //   round 0 – immediate, normal slippage (fast confirmation attempt)
        //   round 1 – immediate, 2× slippage (catch small price moves)
        //   round 2 – 1 s wait, min_out=0 (accept any residual liquidity)
        //   round 3 – 2 s wait, min_out=0 (last resort before writing ✗)
        // Total wall-clock budget is now 3 s instead of 12 s.
        let rounds: &[(u64, u32)] = &[
            (0, 0), // immediate, normal slippage
            (0, 1), // immediate, 2× slippage
            (1, 2), // 1 s wait, min_out=0
            (2, 2), // 2 s wait, min_out=0 final attempt
        ];

        let mut current_amount = amount;
        for (round_idx, (delay_secs, sell_round)) in rounds.iter().enumerate() {
            if *delay_secs > 0 {
                tokio::time::sleep(tokio::time::Duration::from_secs(*delay_secs)).await;
            }
            // Refresh balance — token may have been partially or fully sold already
            if let Ok(bal) = self.rpc.get_token_account_balance(base_ata).await {
                let fresh = bal.amount.parse::<u64>().unwrap_or(0);
                if fresh == 0 {
                    tracing::info!(mint = %pool.base_mint, "Token fully sold — exiting sell_with_retry");
                    return;
                }
                current_amount = fresh;
            }
            match self
                .do_sell(
                    pool,
                    base_ata,
                    current_amount,
                    position_age_secs,
                    *sell_round,
                    exit_reason,
                )
                .await
            {
                Ok(()) => return,
                Err(e) => {
                    let is_last = round_idx + 1 >= rounds.len();
                    tracing::error!(
                        mint = %pool.base_mint,
                        round = round_idx + 1,
                        sell_round,
                        is_last,
                        "Sell round failed: {}",
                        e,
                    );
                }
            }
        }
        tracing::error!(mint = %pool.base_mint, "All sell rounds exhausted — position may be stuck. Check wallet manually.");
    }

    /// `sell_round` controls slippage escalation:
    ///   0 = normal slippage (`config.sell_slippage_pct`)
    ///   1 = 2× slippage (wider tolerance on retries)
    ///   2+ = min_out=0 (accept any price — last resort)
    async fn do_sell(
        &self,
        pool: &CachedPool,
        base_ata: &Pubkey,
        amount: u64,
        position_age_secs: f64,
        sell_round: u32,
        exit_reason: &str,
    ) -> Result<()> {
        use scematica_core::token::apply_slippage;

        // Limit concurrent sell transactions to avoid 429 RPC hammering
        let _permit = self
            .sell_sem
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("sell semaphore closed"))?;

        // NOTE: no record_trade_attempt() here — a sell is an exit, not an entry.
        // The attempt/confirm funnel counts buy entries only.
        let wallet_pubkey = self.wallet.pubkey();
        let quote_ata = get_ata(&wallet_pubkey, &self.quote_mint);

        // Estimate current quote output using the AMM constant-product formula with 0.25% fee.
        // Fetch both reserves in parallel to cut latency vs sequential calls.
        // Also capture the raw quote-vault balance for the drain guard below.
        let (estimated_out, quote_vault_lamports) = {
            let (qb_res, bb_res) = tokio::join!(
                self.rpc.get_token_account_balance(&pool.quote_vault),
                self.rpc.get_token_account_balance(&pool.base_vault),
            );
            if let (Ok(qb), Ok(bb)) = (qb_res, bb_res) {
                let q: u64 = qb.amount.parse().unwrap_or(1);
                let b: u64 = bb.amount.parse().unwrap_or(1);
                (amm_out(amount, b, q), q)
            } else {
                (0, u64::MAX) // MAX = RPC failure, unknown — don't drain-gate
            }
        };

        // Pool drain guard: if quote vault < DRAIN_THRESHOLD_LAMPORTS the Raydium on-chain
        // program rejects every swap regardless of min_out. Returns Ok(()) to free the lock.
        if quote_vault_lamports < DRAIN_THRESHOLD_LAMPORTS {
            tracing::warn!(
                mint = %pool.base_mint,
                quote_vault_lamports,
                "Pool quote vault drained — marking total loss and freeing lock"
            );
            self.metrics.record_trade_failed();
            TradeEvent {
                timestamp: chrono::Utc::now(),
                kind: "SELL".into(),
                mint: pool.base_mint.to_string(),
                symbol: String::new(),
                amount: scematica_core::token::raw_to_ui(amount, pool.base_decimals),
                pnl: -(self.entry_amount_raw as f64) / 1_000_000_000.0,
                status: "✗".into(),
                signature: "pool_drained".into(),
                dex: "Raydium".into(),
                hops: 1,
                pnl_pct: -100.0,
                position_age_secs,
                exit_reason: exit_reason.to_string(),
                pool_size_sol: 0.0,
                pool_age_secs: 0.0,
                velocity_sol_per_sec: 0.0,
                buy_pressure_ratio: 0.0,
                pool_score: 0.0,
                pumpfun_score: 0.0,
                inflow_rate_sol_per_sec: 0.0,
            }
            .append_to_file(TRADES_FILE);
            return Ok(());
        }

        // Escalate slippage by retry round to avoid repeated tx rejections:
        //   round 0: normal (e.g., 2.5%)
        //   round 1: 2× (5%)  — wider window for volatile price action
        //   round 2+: min_out=0 — accept any price, priority is closing the position
        let min_out = if self.dump_mode.load(Ordering::Relaxed) || sell_round >= 2 {
            0
        } else if estimated_out > 0 {
            let effective_slippage = if sell_round == 0 {
                self.config.sell_slippage_pct
            } else {
                // sell_round 1: double the slippage tolerance
                self.config.sell_slippage_pct * 2.0
            };
            apply_slippage(estimated_out, effective_slippage)
        } else {
            0
        };

        tracing::info!(
            mint = %pool.base_mint,
            amount,
            estimated_out,
            min_out,
            "Building sell instruction"
        );

        // For WSOL sells: recreate the ATA before the swap (close_account below will close it,
        // and any prior sell may have already closed it — idempotent so safe to prepend always).
        let mut pre_ixs: Vec<solana_sdk::instruction::Instruction> = Vec::new();
        if self.quote_mint == scematica_core::types::known_tokens::WSOL_MINT {
            pre_ixs.push(
                spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                    &wallet_pubkey,
                    &wallet_pubkey,
                    &scematica_core::types::known_tokens::WSOL_MINT,
                    &spl_token::id(),
                )
            );
        }

        let swap_ixs = self
            .raydium_builder
            .build_swap(
                &pool.id,
                &wallet_pubkey,
                &pool.base_mint,
                &self.quote_mint,
                base_ata,
                &quote_ata,
                amount,
                min_out,
            )
            .await?;

        let mut ixs = pre_ixs;
        ixs.extend(swap_ixs);

        // After selling base → WSOL, close the WSOL ATA to unwrap back to native SOL.
        if self.quote_mint == scematica_core::types::known_tokens::WSOL_MINT {
            ixs.push(spl_token::instruction::close_account(
                &spl_token::id(),
                &quote_ata,
                &wallet_pubkey,
                &wallet_pubkey,
                &[],
            )?);
        }

        let mut attempt = 0u32;
        let mut tried_zero_slippage = false;
        while attempt < self.config.max_sell_retries {
            tracing::info!(
                mint = %pool.base_mint,
                attempt = attempt + 1,
                retries = self.config.max_sell_retries,
                "Sell attempt"
            );
            match self
                .executor
                .execute(ixs.clone(), &self.wallet, &self.rpc)
                .await
            {
                Ok(result) if result.confirmed => {
                    tracing::info!(mint = %pool.base_mint, sig = ?result.signature, "Sell confirmed");

                    // Realized output basis = estimated_out, the per-swap AMM output
                    // computed from live reserves at sell time (constant-product incl. the
                    // 0.25% fee). We deliberately do NOT read the quote ATA balance here:
                    // the sell tx closes the WSOL ATA in the *same* transaction (see the
                    // close_account ix above), so the account no longer exists. An up-to-date
                    // node returns Err; a *lagging* node returns the gross pre-close balance
                    // (rent + residual WSOL + proceeds) which over-states receipts and was the
                    // root cause of the intermittent ~2× / "99%" phantom-gain pnl glitch.
                    let actual_received: u64 = estimated_out;
                    let pnl_raw = actual_received as i64 - self.entry_amount_raw as i64;
                    let pnl_pct = if self.entry_amount_raw > 0 {
                        pnl_raw as f64 / self.entry_amount_raw as f64 * 100.0
                    } else {
                        0.0
                    };
                    TradeEvent {
                        timestamp: chrono::Utc::now(),
                        kind: "SELL".into(),
                        mint: pool.base_mint.to_string(),
                        symbol: String::new(),
                        amount: scematica_core::token::raw_to_ui(amount, pool.base_decimals),
                        pnl: pnl_raw as f64 / 1_000_000_000.0,
                        status: "✓".into(),
                        signature: result.signature.map(|s| s.to_string()).unwrap_or_default(),
                        dex: "Raydium".into(),
                        hops: 1,
                        pnl_pct,
                        position_age_secs,
                        exit_reason: exit_reason.to_string(),
                        pool_size_sol: 0.0,
                        pool_age_secs: 0.0,
                        velocity_sol_per_sec: 0.0,
                        buy_pressure_ratio: 0.0,
                        pool_score: 0.0,
                        pumpfun_score: 0.0,
                        inflow_rate_sol_per_sec: 0.0,
                    }
                    .append_to_file(TRADES_FILE);

                    return Ok(());
                }
                Ok(result) => {
                    let err = result.error.as_deref().unwrap_or("").to_string();
                    tracing::warn!(
                        mint = %pool.base_mint,
                        attempt = attempt + 1,
                        error = %err,
                        "Sell attempt failed"
                    );
                    // Trigger zero-slippage fallback either when Raydium explicitly reports
                    // 0x26 (slippage error) OR when our send/confirm window times out (which
                    // it does when the leader drops the tx before reporting the slippage error
                    // back — common on thin pump.fun pools where price moves >20% during the
                    // 6s window). Without the timeout branch, we'd burn all 5 retries on a
                    // tx the leader keeps dropping.
                    let is_slippage_or_timeout = err.contains("0x26")
                        || err.contains("timeout after")
                        || err.contains("slippage");
                    if is_slippage_or_timeout && !tried_zero_slippage {
                        tried_zero_slippage = true;
                        // Refresh balance: if the slippage error actually meant the tx
                        // landed but we missed the confirmation, we'd be trying to sell
                        // zero tokens. A fresh read here prevents a "no tokens to sell"
                        // fail-loop that burns remaining retries uselessly.
                        let live_amount = if let Ok(bal) =
                            self.rpc.get_token_account_balance(base_ata).await
                        {
                            let fresh = bal.amount.parse::<u64>().unwrap_or(0);
                            if fresh == 0 {
                                tracing::info!(mint = %pool.base_mint, "Token already fully sold (zero-slippage rebuild) — exiting");
                                return Ok(());
                            }
                            fresh
                        } else {
                            amount // RPC failed — use original amount conservatively
                        };
                        tracing::warn!(mint = %pool.base_mint, error = %err, live_amount, "Slippage/timeout — rebuilding sell with min_out=0");
                        if let Ok(swap_rebuilt) = self
                            .raydium_builder
                            .build_swap(
                                &pool.id,
                                &wallet_pubkey,
                                &pool.base_mint,
                                &self.quote_mint,
                                base_ata,
                                &quote_ata,
                                live_amount,
                                0,
                            )
                            .await
                        {
                            let mut rebuilt: Vec<solana_sdk::instruction::Instruction> = Vec::new();
                            if self.quote_mint == scematica_core::types::known_tokens::WSOL_MINT {
                                rebuilt.push(
                                    spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                                        &wallet_pubkey, &wallet_pubkey,
                                        &scematica_core::types::known_tokens::WSOL_MINT,
                                        &spl_token::id(),
                                    )
                                );
                            }
                            rebuilt.extend(swap_rebuilt);
                            if self.quote_mint == scematica_core::types::known_tokens::WSOL_MINT {
                                if let Ok(close_ix) = spl_token::instruction::close_account(
                                    &spl_token::id(),
                                    &quote_ata,
                                    &wallet_pubkey,
                                    &wallet_pubkey,
                                    &[],
                                ) {
                                    rebuilt.push(close_ix);
                                }
                            }
                            ixs = rebuilt;
                        }
                        // retry immediately without spending a retry count
                        continue;
                    }
                    attempt += 1;
                }
                Err(e) => {
                    tracing::error!(mint = %pool.base_mint, attempt = attempt + 1, "Sell error: {}", e);
                    attempt += 1;
                }
            }
        }

        self.metrics.record_trade_failed();
        TradeEvent {
            timestamp: chrono::Utc::now(),
            kind: "SELL".into(),
            mint: pool.base_mint.to_string(),
            symbol: String::new(),
            amount: scematica_core::token::raw_to_ui(amount, pool.base_decimals),
            pnl: 0.0,
            status: "✗".into(),
            signature: String::new(),
            dex: "Raydium".into(),
            hops: 1,
            pnl_pct: -100.0, // total loss signal for the NN agent
            position_age_secs,
            exit_reason: exit_reason.to_string(),
            pool_size_sol: 0.0,
            pool_age_secs: 0.0,
            velocity_sol_per_sec: 0.0,
            buy_pressure_ratio: 0.0,
            pool_score: 0.0,
            pumpfun_score: 0.0,
            inflow_rate_sol_per_sec: 0.0,
        }
        .append_to_file(TRADES_FILE);

        anyhow::bail!(
            "sell exhausted {} retries for {}",
            self.config.max_sell_retries,
            pool.base_mint
        )
    }
}
