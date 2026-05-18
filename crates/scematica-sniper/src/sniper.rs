use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use spl_associated_token_account;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use scematica_ai::agents::AiCoordinator;
use scematica_core::{
    config::SniperConfig,
    metrics::BotMetrics,
    token::{apply_slippage, get_ata, resolve_mint, ui_to_raw},
    types::known_tokens,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
};

use std::sync::Arc;
use tracing::{debug, error, info, warn};
use anyhow::Result;
use scematica_executor::{get_builder, SwapInstructionBuilder};
use scematica_core::types::DexKind;

use crate::{
    alerts::AlertManager,
    ath_tracker::AthTracker,
    cache::{CachedPool, MarketCache, PoolCache, SnipeListCache},
    day_weight::DayWeighter,
    executor::{DefaultExecutor, JitoExecutor, TxExecutor},
    filters::FilterPipeline,
    grief_breaker::GriefBreaker,
    kelly::KellySizer,
    listener::ListenerEvent,
    pool_scorer::PoolScorer,
    reputation::DeployerLedger,
};
use scematica_core::metrics::{TradeEvent, TRADES_FILE};
use scematica_core::metrics::{StrategySnapshot, STRATEGY_FILE};

/// Raydium constant-product AMM output with 0.25% fee.
/// out = (reserve_out * amount_in * 9975) / (reserve_in * 10000 + amount_in * 9975)
#[inline]
fn amm_out(amount_in: u64, reserve_in: u64, reserve_out: u64) -> u64 {
    let num = (reserve_out as u128) * (amount_in as u128) * 9975u128;
    let den = (reserve_in as u128) * 10000u128 + (amount_in as u128) * 9975u128;
    if den == 0 { return 0; }
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
        if lock.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
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
}

impl LiveParams {
    pub fn from_config(config: &SniperConfig) -> Self {
        Self {
            take_profit_pct: config.take_profit_pct,
            stop_loss_pct: config.stop_loss_pct,
            amount_multiplier: 1.0,
            market_regime: "neutral".into(),
        }
    }
}

/// Core sniper bot: receives pool events and executes buy/sell
pub struct Sniper {
    config: SniperConfig,
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

impl Sniper {
    pub fn new(
        config: SniperConfig,
        wallet: Arc<Keypair>,
        rpc: Arc<RpcClient>,
        metrics: Arc<BotMetrics>,
        alerts: Arc<AlertManager>,
    ) -> Self {
        let quote_mint = resolve_mint(&config.quote_mint)
            .unwrap_or(known_tokens::WSOL_MINT);
        let quote_decimals = if config.quote_mint.to_uppercase() == "USDC" { 6 } else { 9 };
        let quote_amount_raw = ui_to_raw(config.quote_amount, quote_decimals);

        let deployer_ledger_shared = Arc::new(Mutex::new(DeployerLedger::load()));
        let filter_pipeline = FilterPipeline::new(
            config.filters.clone(),
            rpc.clone(),
            quote_amount_raw,
            &config.blacklist_path,
            Some(Arc::clone(&deployer_ledger_shared)),
        );

        let executor: Arc<dyn TxExecutor> = match config.quote_mint.as_str() {
            _ if std::env::var("EXECUTOR").unwrap_or_default() == "jito" => {
                Arc::new(JitoExecutor::new(
                    std::env::var("JITO_URL")
                        .unwrap_or_else(|_| "https://mainnet.block-engine.jito.wtf".into()),
                    0.006,
                ))
            }
            _ => Arc::new(DefaultExecutor::new(400_000, 200_000, true, 3).with_dynamic_fees()),
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
            Some(KellySizer::with_min_trades(config.kelly_fraction, config.kelly_min_trades))
        } else {
            None
        };

        let ath_drawdown_pct = config.ath_drawdown_pct;

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
            processing_lock: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            quote_mint,
            quote_decimals,
            quote_amount_raw,
            raydium_builder: Arc::from(get_builder(DexKind::Raydium, rpc.clone())
                .expect("Raydium builder not found")),
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
        }
    }

    /// Run the Strategy Agent adjustment loop.
    /// Call this in a background task after constructing the Sniper.
    /// Evaluates recent trade history every `interval_secs` and updates live_params.
    pub async fn run_strategy_loop(self: Arc<Self>, interval_secs: u64) {
        let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
        info!("Strategy agent loop started (interval: {}s)", interval_secs);
        loop {
            ticker.tick().await;

            let ai = match &self.ai {
                Some(ai) => ai.clone(),
                None => continue, // no AI configured
            };

            let history = self.trade_history.lock().clone();
            if history.len() < 5 {
                debug!("Strategy agent: not enough trades yet ({}/5)", history.len());
                continue;
            }

            let params = self.live_params.read().clone();
            let snap = self.metrics.snapshot();
            let total_pnl = snap.total_pnl_sol();
            let win_rate = snap.win_rate();

            let adjustment = ai.strategy.get_adjustment(
                &history,
                total_pnl,
                params.take_profit_pct,
                params.stop_loss_pct,
                scematica_core::token::raw_to_ui(self.quote_amount_raw, self.quote_decimals),
                win_rate,
            ).await;

            // Apply the adjustment
            let mut live = self.live_params.write();
            if let Some(tp) = adjustment.take_profit_pct {
                info!(
                    "Strategy agent: take_profit {} → {:.1}% ({})",
                    live.take_profit_pct, tp, adjustment.market_regime
                );
                live.take_profit_pct = tp;
            }
            if let Some(sl) = adjustment.stop_loss_pct {
                info!(
                    "Strategy agent: stop_loss {} → {:.1}% ({})",
                    live.stop_loss_pct, sl, adjustment.market_regime
                );
                live.stop_loss_pct = sl;
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
                let _ = std::fs::write("scematica-regime-shift.json", r#"{"shift":true}"#);
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
        const RADAR_FILE: &str = "scematica-pool-radar.json";
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
        let mut entries: Vec<serde_json::Value> =
            std::fs::read_to_string(RADAR_FILE)
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
            let tmp = format!("{}.tmp", RADAR_FILE);
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, RADAR_FILE);
            }
        }
    }

    /// Main event handler — called for each event from the listener
    pub async fn handle_event(&self, event: ListenerEvent) {
        match event {
            ListenerEvent::NewPool(pool) => {
                self.on_new_pool(pool).await;
            }
            ListenerEvent::WalletUpdate { account, mint, amount } => {
                self.on_wallet_update(account, mint, amount).await;
            }
            ListenerEvent::NewMarket(_) => {}
        }
    }

    async fn on_new_pool(&self, pool: CachedPool) {
        let mint_str = pool.base_mint.to_string();

        // Cross-session dedup. The listener-level seen_pool_ids set already drops
        // duplicate events within a session before they reach us, so this guard
        // mostly fires for pools persisted to pool-cache.json from a prior run.
        if self.pool_cache.contains(&pool.id.to_string()) {
            debug!(
                mint = %pool.base_mint,
                pool = %pool.id,
                "Pool already in persisted cache — skipping (clear pool-cache.json to re-eval)"
            );
            return;
        }

        // Skip if quote mint doesn't match
        if pool.quote_mint != self.quote_mint {
            debug!(mint = %pool.base_mint, "Skipping pool: wrong quote mint");
            return;
        }

        // Snipe list check
        if let Some(sl) = &self.snipe_list {
            if !sl.is_listed(&mint_str) {
                debug!(mint = %pool.base_mint, "Skipping: not in snipe list");
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
                if self.skip_log_throttle_secs
                    .compare_exchange(last, now_secs, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    let mode = if dump_on { "DUMP" } else { "SELL" };
                    info!(
                        mode,
                        "Pools being skipped because {} mode is active — press [b] on dashboard Logs tab to clear",
                        mode,
                    );
                }
            }
            return;
        }

        // Buy limit
        if self.config.max_buys > 0 {
            let count = self.buy_count.load(Ordering::Relaxed);
            if count >= self.config.max_buys {
                debug!(mint = %pool.base_mint, count, limit = self.config.max_buys, "Buy limit reached — skipping");
                return;
            }
        }

        // Max concurrent positions
        if self.config.max_concurrent_positions > 0 {
            let open = self.open_positions.load(Ordering::Relaxed);
            if open >= self.config.max_concurrent_positions {
                debug!(mint = %pool.base_mint, open, "Max concurrent positions reached — skipping");
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
                return;
            }
        }

        // ── ATH drawdown guard ─────────────────────────────────────────────────
        if self.ath_drawdown_pct > 0.0 {
            let current = self.rpc.get_balance(&self.wallet.pubkey()).await.unwrap_or(0);
            let dd = self.ath_tracker.drawdown_pct(current);
            if dd >= self.ath_drawdown_pct {
                warn!(
                    mint = %pool.base_mint,
                    drawdown_pct = %format!("{:.1}%", dd),
                    threshold_pct = %format!("{:.1}%", self.ath_drawdown_pct),
                    "ATH drawdown limit reached — skipping buy"
                );
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
            let q = qv.ok().and_then(|r| r.ok()).and_then(|b| b.amount.parse::<u64>().ok()).unwrap_or(0);
            let b = bv.ok().and_then(|r| r.ok()).and_then(|b| b.amount.parse::<u64>().ok()).unwrap_or(0);
            (q, b)
        };
        let upfront_pool_size_sol = upfront_pool_size_lamports as f64 / 1e9;
        let upfront_score = PoolScorer::score(&pool, upfront_pool_size_lamports);

        // High-speed mode: skip filters, AI, scorer — go straight to buy. The operator
        // has explicitly opted in to extra rugs / failed buys / 429s in exchange for
        // entry latency. We still respect the listener's open_time gate above.
        let high_speed = self.high_speed_mode.load(Ordering::Relaxed);
        if high_speed {
            info!(mint = %pool.base_mint, "⚡ HIGH-SPEED — bypassing filters/AI/scorer");
        }

        // Run filters (unless snipe list mode or high-speed mode) — hard cap at 25s
        // so a hung RPC node can't stall this evaluation task and starve the pool stream.
        if !high_speed && self.snipe_list.is_none() {
            let filter_result = tokio::time::timeout(
                tokio::time::Duration::from_secs(25),
                self.filter_pipeline.execute(&pool),
            ).await;
            match filter_result {
                Ok(true) => {}
                Ok(false) => {
                    info!(mint = %pool.base_mint, "Pool rejected by filters");
                    self.write_radar_entry(&pool, upfront_pool_size_sol, false, upfront_score);
                    return;
                }
                Err(_) => {
                    warn!(mint = %pool.base_mint, "Filter pipeline timed out (25s) — skipping pool");
                    self.write_radar_entry(&pool, upfront_pool_size_sol, false, upfront_score);
                    return;
                }
            }
        }

        // AI risk assessment (if available, and not high-speed)
        if !high_speed { if let Some(ai) = &self.ai {
            // Reuse the upfront vault fetch — no extra RPC round-trip here.
            let pool_size_sol = scematica_core::token::raw_to_ui(upfront_pool_size_lamports, pool.quote_decimals);

            // UTC hour from open_time (unix timestamp)
            let open_hour = (pool.open_time % 86400 / 3600) as u8;

            // The pool has already passed every ENABLED filter at this point. When a
            // filter is disabled, the operator opted out of caring — feed the AI the
            // "safe" value rather than the disabled flag, otherwise the LLM scores it
            // as risky for things we explicitly chose not to check (e.g., we disabled
            // check_mint_renounced because pump.fun graduates renounce *after* the WS
            // notification; the AI was scoring them 30/100 with "mint not renounced"
            // as the top reason, and rejecting every buy).
            let mint_renounced = true;
            let freezable = false;
            let lp_burned = true;
            let mutable_metadata = false;

            let risk = ai.risk.score_token(
                &pool.base_mint.to_string(),
                "UNKNOWN",
                "UNKNOWN",
                pool_size_sol,
                mint_renounced,
                freezable,
                lp_burned,
                mutable_metadata,
                self.config.filters.check_socials,
                open_hour,
            ).await;

            info!(
                mint = %pool.base_mint,
                score = risk.score,
                recommendation = %risk.recommendation,
                reasoning = %risk.reasoning,
                "AI risk assessment"
            );

            if !risk.should_buy() {
                // If the AI API itself failed (rate limit, network, etc.) the token
                // already passed all on-chain filters — don't let an infrastructure
                // outage block every trade.  Only hard-skip on a genuine AI rejection.
                let ai_failed = risk.red_flags.iter()
                    .any(|f| f.contains("AI assessment failed"));
                if ai_failed {
                    warn!(
                        mint = %pool.base_mint,
                        reasoning = %risk.reasoning,
                        "AI unavailable — proceeding on on-chain filters"
                    );
                } else {
                    info!(
                        mint = %pool.base_mint,
                        score = risk.score,
                        flags = ?risk.red_flags,
                        "AI rejected token — skipping buy"
                    );
                    return;
                }
            }
        } } // close `if let Some(ai)` and the outer `if !high_speed` guard

        // ── Pool predictive scoring (skipped in high-speed mode) ───────────────
        if !high_speed && self.config.min_pool_score > 0.0 {
            // upfront_score was computed above from the same vault fetch — free.
            if upfront_score < self.config.min_pool_score {
                info!(
                    mint = %pool.base_mint,
                    score = %format!("{:.1}", upfront_score),
                    min = %format!("{:.1}", self.config.min_pool_score),
                    "Pool score too low — skipping buy"
                );
                self.write_radar_entry(&pool, upfront_pool_size_sol, false, upfront_score);
                return;
            }
            info!(
                mint = %pool.base_mint,
                score = %format!("{:.1}", upfront_score),
                "Pool scorer: passed"
            );
        }

        // Track last pool time for gas war burst detection
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.gas_war_last_pool_ms.store(now_ms, Ordering::Relaxed);

        // Write pool radar entry — pool passed all checks and is about to be bought
        // Reuse upfront values; no new RPC call needed.
        self.write_radar_entry(&pool, upfront_pool_size_sol, true, upfront_score);

        // Execute buy — pass upfront reserves so buy() can compute a real min_out
        // without another RPC round-trip.
        if let Err(e) = self.buy(&pool, upfront_pool_size_lamports, upfront_base_vault_lamports, upfront_score).await {
            error!(mint = %pool.base_mint, "buy() error: {}", e);
        }
    }

    async fn buy(&self, pool: &CachedPool, quote_reserve_lam: u64, base_reserve_lam: u64, upfront_score: f64) -> Result<()> {
        // ── Compute effective quote amount with day-weight and Kelly multipliers ──
        let mut effective_quote_amount_raw = self.quote_amount_raw;

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
                effective_quote_amount_raw =
                    (effective_quote_amount_raw as f64 * prog_mult) as u64;
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

        // NN agent gating: when epsilon < 0.3 (agent is confident), scale buy amount
        // based on the recommended action. Uses file-based IPC consistent with the
        // rest of this codebase — reads scematica-nn-stats.json (written every 5 s).
        //
        // Action → size multiplier:
        //   BuyAgg (index 2): 1.5×  — agent sees a strong signal, upsize
        //   Buy    (index 1): 1.0×  — baseline (no change)
        //   Hold   (index 0): 0.5×  — agent is uncertain, downsize
        //   SellPartial/SellAll: skip buy entirely (bearish signal)
        if let Ok(nn_raw) = std::fs::read_to_string("scematica-nn-stats.json") {
            if let Ok(nn_v) = serde_json::from_str::<serde_json::Value>(&nn_raw) {
                let ready = nn_v["ready_to_advise"].as_bool().unwrap_or(false);
                let epsilon = nn_v["epsilon"].as_f64().unwrap_or(1.0);
                if ready && epsilon < 0.3 {
                    let q_vals: Vec<f64> = nn_v["last_q_values"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
                        .unwrap_or_default();
                    if let Some(best_action) = q_vals.iter()
                        .enumerate()
                        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                        .map(|(i, _)| i)
                    {
                        let nn_mult = match best_action {
                            2 => { // BuyAgg
                                info!(mint = %pool.base_mint, epsilon, "NN: BuyAgg signal — sizing up 1.5×");
                                1.5
                            }
                            1 => 1.0, // Buy — no change
                            0 => { // Hold — downsize
                                info!(mint = %pool.base_mint, epsilon, "NN: Hold signal — sizing down 0.5×");
                                0.5
                            }
                            _ => { // SellPartial or SellAll — skip buy
                                info!(mint = %pool.base_mint, epsilon, "NN: Sell signal — skipping buy");
                                return Ok(());
                            }
                        };
                        effective_quote_amount_raw =
                            (effective_quote_amount_raw as f64 * nn_mult) as u64;
                    }
                }
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

        let wallet_pubkey = self.wallet.pubkey();
        let quote_ata = get_ata(&wallet_pubkey, &self.quote_mint);
        let base_ata = get_ata(&wallet_pubkey, &pool.base_mint);

        // Gate: ensure we have enough native SOL for quote amount + fees + ATA rent + reserve floor.
        // min_sol_reserve (config) is the absolute floor to keep in wallet; defaults to 0.02 SOL.
        // Always keep at least 6_000_000 lamports (0.006 SOL) for tx fees even if reserve is lower.
        let native_balance = self.rpc.get_balance(&wallet_pubkey).await.unwrap_or(0);
        let reserve_lam = ((self.config.min_sol_reserve * 1e9) as u64).max(6_000_000);
        let min_required = effective_quote_amount_raw + reserve_lam;
        if native_balance < min_required {
            warn!(
                mint = %pool.base_mint,
                balance_sol = native_balance as f64 / 1e9,
                required_sol = min_required as f64 / 1e9,
                "Insufficient SOL — skipping buy"
            );
            return Ok(());
        }

        // Compute buy min_out using the AMM constant-product formula + slippage.
        // Use the upfront reserves passed from on_new_pool (saved vs another RPC
        // call). Falls back to 0 (accept any) if reserves are unknown/zero.
        let buy_min_out = if quote_reserve_lam > 0 && base_reserve_lam > 0 {
            let expected = amm_out(effective_quote_amount_raw, quote_reserve_lam, base_reserve_lam);
            if expected > 0 {
                scematica_core::token::apply_slippage(expected, self.config.buy_slippage_pct)
            } else { 0 }
        } else { 0 };

        // Confirmation window: wait briefly and verify price hasn't already pumped 15%+.
        // Skipped in high-speed mode; skipped when confirmation_window_ms == 0 or quote
        // reserves are unknown (can't compute the drain percentage without them).
        if self.config.confirmation_window_ms > 0
            && !self.high_speed_mode.load(Ordering::Relaxed)
            && quote_reserve_lam > 0
        {
            tokio::time::sleep(tokio::time::Duration::from_millis(self.config.confirmation_window_ms)).await;
            if let Ok(Ok(qb)) = tokio::time::timeout(
                tokio::time::Duration::from_secs(2),
                self.rpc.get_token_account_balance(&pool.quote_vault),
            ).await {
                if let Ok(current_q) = qb.amount.parse::<u64>() {
                    // Quote vault draining = tokens bought out = price pumped.
                    // Drain >15% means early buyers already front-ran us — skip.
                    if current_q < quote_reserve_lam {
                        let drain_pct = (quote_reserve_lam - current_q) as f64
                            / quote_reserve_lam as f64 * 100.0;
                        if drain_pct > 15.0 {
                            info!(
                                mint = %pool.base_mint,
                                drain_pct = %format!("{:.1}%", drain_pct),
                                "Confirmation window: price already pumped — skipping buy"
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
        let ixs = match self.raydium_builder.build_swap(
            &pool.id,
            &wallet_pubkey,
            &self.quote_mint,
            &pool.base_mint,
            &quote_ata,
            &base_ata,
            effective_quote_amount_raw,
            buy_min_out,
        ).await {
            Ok(ixs) => ixs,
            Err(e) => {
                error!(mint = %pool.base_mint, "Failed to build swap instructions: {}", e);
                self.metrics.record_trade_failed();
                return Err(e);
            }
        };

        // One-buy-at-a-time gate: prevents two buy TRANSACTIONS from racing on the
        // same WSOL ATA simultaneously. Released as soon as the buy tx confirms (NOT
        // held for the duration of sell monitoring — see release below).
        // High-speed mode bypasses so multiple pools can snipe in parallel.
        let high_speed = self.high_speed_mode.load(Ordering::Relaxed);
        let slot = if self.config.one_token_at_a_time && !high_speed {
            match ProcessingSlot::try_acquire(&self.processing_lock) {
                Some(g) => Some(g),
                None => {
                    debug!(mint = %pool.base_mint, "one_token_at_a_time: buy tx in flight, skipping");
                    return Ok(());
                }
            }
        } else {
            None
        };

        // Count this as a real attempt only after the lock is secured —
        // pools skipped by try_acquire() above were never attempted.
        self.metrics.record_trade_attempt();
        info!(
            mint = %pool.base_mint,
            amount_sol = effective_quote_amount_raw as f64 / 1e9,
            "Executing buy"
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
            )
        );

        final_ixs.extend(ixs);
        let ixs = final_ixs;


        // Per-attempt deadline — independently of the executor's own internal cap.
        // Defense in depth: even if a custom executor (e.g. Jito) blocks forever
        // on the bundle endpoint, the buy task here can never hold the slot beyond this.
        const BUY_ATTEMPT_DEADLINE: tokio::time::Duration = tokio::time::Duration::from_secs(8);

        for attempt in 0..self.config.max_buy_retries {
            info!("Buy attempt {}/{}", attempt + 1, self.config.max_buy_retries);
            let exec_outcome = tokio::time::timeout(
                BUY_ATTEMPT_DEADLINE,
                self.executor.execute(ixs.clone(), &self.wallet, &self.rpc),
            ).await;
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

                    // Emit trade event for the dashboard
                    TradeEvent {
                        timestamp: chrono::Utc::now(),
                        kind: "BUY".into(),
                        mint: pool.base_mint.to_string(),
                        symbol: String::new(),
                        amount: scematica_core::token::raw_to_ui(effective_quote_amount_raw, self.quote_decimals),
                        pnl: 0.0,
                        status: "✓".into(),
                        signature: result.signature
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                        dex: "Raydium".into(),
                        hops: 1,
                        pnl_pct: 0.0,
                        position_age_secs: 0.0,
                    }.append_to_file(TRADES_FILE);

                    // Track open positions
                    self.open_positions.fetch_add(1, Ordering::Relaxed);

                    // Alert: buy confirmed
                    {
                        let alerts = self.alerts.clone();
                        let mint_str = pool.base_mint.to_string();
                        let amount_sol = scematica_core::token::raw_to_ui(effective_quote_amount_raw, self.quote_decimals);
                        tokio::spawn(async move {
                            alerts.send(
                                "BUY Confirmed",
                                &format!("Mint: {}...\nAmount: {:.4} SOL", &mint_str[..8], amount_sol),
                            ).await;
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
            symbol: String::new(),
            amount: scematica_core::token::raw_to_ui(effective_quote_amount_raw, self.quote_decimals),
            pnl: 0.0,
            status: "✗".into(),
            signature: String::new(),
            dex: "Raydium".into(),
            hops: 1,
            pnl_pct: 0.0,
            position_age_secs: 0.0,
        }.append_to_file(TRADES_FILE);
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

        // Use live_params so Strategy Agent adjustments take effect immediately
        let (take_profit_pct, stop_loss_pct) = {
            let p = self.live_params.read();
            (p.take_profit_pct, p.stop_loss_pct)
        };
        let take_profit_factor = 1.0 + take_profit_pct / 100.0;
        let stop_loss_factor = 1.0 - stop_loss_pct / 100.0;
        let target_profit = (self.quote_amount_raw as f64 * take_profit_factor) as u64;
        let stop_loss = (self.quote_amount_raw as f64 * stop_loss_factor) as u64;

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
                            let current_value =
                                (amount as u128 * q as u128 / b as u128) as u64;

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
                                let pnl_sol = (current_value as f64 - self.quote_amount_raw as f64)
                                    / 1_000_000_000.0;
                                self.record_trade_outcome(true, pnl_sol);
                                break;
                            }
                            if current_value <= stop_loss {
                                info!(mint = %pool.base_mint, "Stop loss triggered");
                                self.sell(&pool, &base_ata, amount).await;
                                let pnl_sol = (current_value as f64 - self.quote_amount_raw as f64)
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
        info!(mint = %pool.base_mint, amount, "Executing sell");
        self.metrics.record_trade_attempt();

        let wallet_pubkey = self.wallet.pubkey();
        let quote_ata = get_ata(&wallet_pubkey, &self.quote_mint);

        let min_out = apply_slippage(
            // rough estimate
            (amount as f64 * 0.99) as u64,
            self.config.sell_slippage_pct,
        );

        let ixs = match self.raydium_builder.build_swap(
            &pool.id,
            &wallet_pubkey,
            &pool.base_mint,
            &self.quote_mint,
            base_ata,
            &quote_ata,
            amount,
            min_out,
        ).await {
            Ok(ixs) => ixs,
            Err(e) => {
                error!("Failed to build sell instructions: {}", e);
                return;
            }
        };

        for attempt in 0..self.config.max_sell_retries {
            info!("Sell attempt {}/{}", attempt + 1, self.config.max_sell_retries);
            match self.executor.execute(ixs.clone(), &self.wallet, &self.rpc).await {
                Ok(result) if result.confirmed => {
                    info!(
                        mint = %pool.base_mint,
                        sig = ?result.signature,
                        "Sell confirmed"
                    );
                    self.metrics.record_trade_confirmed(0);

                    // Emit trade event for the dashboard
                    TradeEvent {
                        timestamp: chrono::Utc::now(),
                        kind: "SELL".into(),
                        mint: pool.base_mint.to_string(),
                        symbol: String::new(),
                        amount: scematica_core::token::raw_to_ui(amount, pool.base_decimals),
                        pnl: 0.0, // PnL calculated post-confirmation in production
                        status: "✓".into(),
                        signature: result.signature
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                        dex: "Raydium".into(),
                        hops: 1,
                        pnl_pct: 0.0,
                        position_age_secs: 0.0,
                    }.append_to_file(TRADES_FILE);
                    return;
                }
                Ok(result) => {
                    warn!(error = ?result.error, "Sell attempt failed");
                }
                Err(e) => {
                    error!("Sell error: {}", e);
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
            pnl_pct: -100.0,
            position_age_secs: 0.0,
        }.append_to_file(TRADES_FILE);
    }

    async fn on_wallet_update(&self, account: Pubkey, mint: Pubkey, amount: u64) {
        debug!(account = %account, mint = %mint, amount, "Wallet update");
    }

    /// Scan wallet for existing token positions and spawn a sell monitor for each.
    /// Called once at startup so tokens bought in a previous run can be sold.
    pub async fn scan_existing_positions(&self) {
        use solana_client::rpc_request::TokenAccountsFilter;
        use solana_sdk::program_pack::Pack;

        let wallet_pubkey = self.wallet.pubkey();
        info!("Startup scan: checking wallet for existing token positions...");

        let keyed_accounts = match self.rpc
            .get_token_accounts_by_owner(
                &wallet_pubkey,
                TokenAccountsFilter::ProgramId(spl_token::id()),
            )
            .await
        {
            Ok(a) => a,
            Err(e) => {
                warn!("Startup scan: failed to list token accounts: {}", e);
                return;
            }
        };

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

            // Unpack the raw SPL token account to get mint and balance
            let raw = match self.rpc.get_account(&ata_pk).await {
                Ok(a) => a,
                Err(_) => continue,
            };
            let token_acct = match spl_token::state::Account::unpack(&raw.data) {
                Ok(a) => a,
                Err(_) => continue,
            };

            if token_acct.amount == 0 {
                continue;
            }
            let mint = token_acct.mint;
            if skip_mints.contains(&mint) {
                continue;
            }

            info!(
                mint = %mint,
                amount = token_acct.amount,
                "Startup scan: found existing position — looking up Raydium pool"
            );

            let pool = match self.find_raydium_pool_for_mint(&mint).await {
                Some(p) => p,
                None => {
                    warn!(mint = %mint, "Startup scan: no Raydium pool found — cannot auto-sell");
                    continue;
                }
            };

            if pool.quote_mint != self.quote_mint {
                debug!(mint = %mint, "Startup scan: pool quote mint mismatch — skipping");
                continue;
            }

            info!(
                mint = %mint,
                pool = %pool.id,
                amount = token_acct.amount,
                "Startup scan: spawning sell monitor"
            );

            // Pre-existing wallet position from a prior session — we don't know
            // the real entry amount, so use the config baseline as the floor.
            // sell_mode / dump_mode handles these positions anyway; the entry
            // value here only affects the trailing-stop reference.
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
            match self.rpc
                .get_token_accounts_by_owner(
                    &wallet_pubkey,
                    TokenAccountsFilter::ProgramId(program_id),
                )
                .await
            {
                Ok(accounts) => {
                    info!("AUTO DUMP: found {} accounts under {}", accounts.len(), program_id);
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
                let amount = u64::from_le_bytes(
                    raw.data[64..72].try_into().unwrap_or([0u8; 8]),
                );
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
                monitor.sell_with_retry(&pool, &ata_pk, amount, 0.0).await;
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
        use solana_account_decoder::UiAccountEncoding;
        use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
        use solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
        use scematica_core::dex::{program_ids, raydium_v4::*};

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

        let pools = match self.rpc
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
        let base_vault =
            Pubkey::try_from(&data[BASE_VAULT_OFFSET..BASE_VAULT_OFFSET + 32]).ok()?;
        let quote_vault =
            Pubkey::try_from(&data[QUOTE_VAULT_OFFSET..QUOTE_VAULT_OFFSET + 32]).ok()?;
        let market_id =
            Pubkey::try_from(&data[MARKET_ID_OFFSET..MARKET_ID_OFFSET + 32]).ok()?;
        let open_time = u64::from_le_bytes(
            data[OPEN_TIME_OFFSET..OPEN_TIME_OFFSET + 8].try_into().ok()?,
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
}

impl SellMonitor {
    async fn monitor_and_sell(&self, pool: CachedPool, base_ata: Pubkey) {
        // Track position entry so the NN observer gets accurate holding-time signal
        // on every sell path (sell_mode trigger / partial TP / dump / TP-SL / timeout).
        let position_started = std::time::Instant::now();
        let entry_unix_secs = chrono::Utc::now().timestamp();

        // Register this position in the live registry so the dashboard sees it
        // immediately. We update the entry on every price check below and
        // remove it when the monitor exits (any branch).
        let pos_key = pool.base_mint.to_string();
        let initial_sl_lam = (self.entry_amount_raw as f64
            * (1.0 - self.live_params.read().stop_loss_pct / 100.0)) as u64;
        self.live_positions.insert(pos_key.clone(), LivePositionSnapshot {
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
        });
        // RAII guard: ensure the position is removed from the registry no matter
        // which branch of monitor_and_sell exits the function.
        struct PositionGuard {
            map: Arc<DashMap<String, LivePositionSnapshot>>,
            key: String,
        }
        impl Drop for PositionGuard {
            fn drop(&mut self) { self.map.remove(&self.key); }
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
        let fast_poll_ms: u64        = 75;
        let fast_interval    = tokio::time::Duration::from_millis(fast_poll_ms);
        let normal_floor_ms          = 250u64;
        let normal_interval  = tokio::time::Duration::from_millis(
            self.config.price_check_interval_ms.max(normal_floor_ms),
        );
        let fast_budget_ms   = fast_phase_checks * fast_poll_ms;
        let remaining_budget = self.config.price_check_duration_ms.saturating_sub(fast_budget_ms);
        let normal_checks    = if self.config.price_check_interval_ms > 0 {
            remaining_budget / self.config.price_check_interval_ms.max(normal_floor_ms)
        } else { 0 };
        let max_checks = fast_phase_checks + normal_checks;

        // Partial TP state
        let partial_tp_enabled = self.config.partial_tp_pct > 0.0 && self.config.partial_tp_trigger > 0.0;
        let partial_tp_target  = (self.entry_amount_raw as f64 * (1.0 + self.config.partial_tp_trigger / 100.0)) as u64;
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
            (
                self.config.momentum_max_escalations,
                self.config.momentum_escalation_factor,
                self.config.momentum_pullback_exit_pct,
                self.config.momentum_escalation_threshold_pct,
            )
        };
        let initial_tp_pct = self.live_params.read().take_profit_pct;
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
        let tiered_enabled = self.config.tiered_partial_tp
            && !self.config.tiered_partial_tp_levels.is_empty();
        let mut tiered_fired: Vec<bool> = vec![false; self.config.tiered_partial_tp_levels.len()];

        // Profit-lock counter: tracks consecutive checks where value > entry.
        // Once it reaches `profit_lock_checks`, the SL floor is raised to near
        // breakeven (entry × 0.98) so a sustained winner can't reverse to a loss.
        let mut profit_lock_counter: u32 = 0;
        let profit_lock_floor = (self.entry_amount_raw as f64 * 0.98) as u64;

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
                            self.sell_with_retry(&pool, &base_ata, amount, position_started.elapsed().as_secs_f64()).await;
                        }
                    }
                    break;
                }
            }

            // Sell/dump mode activated mid-monitor → sell immediately
            if self.sell_mode.load(Ordering::Relaxed) || self.dump_mode.load(Ordering::Relaxed) {
                tracing::info!(mint = %pool.base_mint, "Sell/dump mode active — forcing immediate sell");
                if let Ok(balance) = self.rpc.get_token_account_balance(&base_ata).await {
                    let amount: u64 = balance.amount.parse().unwrap_or(0);
                    if amount > 0 {
                        self.sell_with_retry(&pool, &base_ata, amount, position_started.elapsed().as_secs_f64()).await;
                    }
                }
                break;
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
            let target_profit    = (self.entry_amount_raw as f64 * (1.0 + dynamic_tp_pct / 100.0)) as u64;

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
                let start_lam   = self.session_start_lamports.load(Ordering::Relaxed);
                let daily_pnl   = *self.daily_pnl_lamports.lock();
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
            let mut stop_loss_amount = (self.entry_amount_raw as f64 * (1.0 - effective_stop_loss_pct / 100.0)) as u64;

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
            let (ata_res, qb_res, bb_res) = match tokio::time::timeout(SELL_POLL_DEADLINE, fetch_fut).await {
                Ok(t) => t,
                Err(_) => {
                    tracing::debug!(
                        mint = %pool.base_mint,
                        "Sell-monitor balance fetch timed out — skipping this iteration"
                    );
                    checks += 1;
                    if checks >= max_checks { break; }
                    let sleep_dur = if checks < fast_phase_checks { fast_interval } else { normal_interval };
                    tokio::time::sleep(sleep_dur).await;
                    continue;
                }
            };
            match ata_res {
                Ok(balance) => {
                    let amount: u64 = balance.amount.parse().unwrap_or(0);
                    if amount == 0 { break; }

                    if let (Ok(qb), Ok(bb)) = (qb_res, bb_res) {
                        let q: u64 = qb.amount.parse().unwrap_or(1);
                        let b: u64 = bb.amount.parse().unwrap_or(1);
                        // Use correct AMM formula with 0.25% fee — more accurate than naive ratio
                        let current_value = amm_out(amount, b, q);

                        // Momentum: track consecutive declines for dump detection
                        if current_value < prev_value {
                            decline_streak += 1;
                        } else {
                            decline_streak = 0;
                        }

                        // Capture entry quote vault on the first valid check
                        if entry_q == 0 { entry_q = q; }

                        // ── Whale exit: single-check vault drop ─────────────────
                        if self.config.whale_exit_vault_drop_pct > 0.0 && prev_q > 0 && q < prev_q {
                            let vault_drop_pct = (prev_q - q) as f64 / prev_q as f64 * 100.0;
                            if vault_drop_pct >= self.config.whale_exit_vault_drop_pct {
                                let pnl_lamports = current_value as i64 - self.entry_amount_raw as i64;
                                let pnl_sol = pnl_lamports as f64 / 1e9;
                                tracing::warn!(
                                    mint = %pool.base_mint,
                                    vault_drop_pct = %format!("{:.1}%", vault_drop_pct),
                                    threshold = self.config.whale_exit_vault_drop_pct,
                                    pnl_sol,
                                    "🐋 Whale exit: vault drop detected — selling immediately"
                                );
                                self.sell_with_retry(&pool, &base_ata, amount, position_started.elapsed().as_secs_f64()).await;
                                self.record_sell_outcome(pnl_lamports > 0, pnl_lamports, pnl_sol, &pool.base_mint.to_string()).await;
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
                        {
                            let exhaustion_floor =
                                (entry_q as f64 * (1.0 - self.config.volume_exhaustion_pct / 100.0)) as u64;
                            if q < exhaustion_floor {
                                let pnl_lamports = current_value as i64 - self.entry_amount_raw as i64;
                                let pnl_sol = pnl_lamports as f64 / 1e9;
                                tracing::info!(
                                    mint = %pool.base_mint,
                                    entry_q, current_q = q, exhaustion_floor,
                                    pnl_sol,
                                    "Volume exhaustion exit — locking profit before liquidity dries up"
                                );
                                self.sell_with_retry(&pool, &base_ata, amount, position_started.elapsed().as_secs_f64()).await;
                                self.record_sell_outcome(true, pnl_lamports, pnl_sol, &pool.base_mint.to_string()).await;
                                break;
                            }
                        }

                        // Update peak (used for both trailing stop AND momentum pullback exit)
                        if current_value > peak_value { peak_value = current_value; }
                        if trailing_enabled {
                            let trail = (peak_value as f64 * (1.0 - self.config.trailing_stop_loss_pct / 100.0)) as u64;
                            if trail > stop_loss_amount { stop_loss_amount = trail; }
                        }

                        // Flash-crash detector: if a SINGLE check shows a value drop
                        // ≥ flash_crash_pct from the previous check's value AND we've
                        // already had at least 3 stabilising checks (avoids false-
                        // positives on the fill price vs entry discrepancy), exit now.
                        // This fires before the 3-consecutive-decline counter can even
                        // accumulate — crucial for vertical dumps (rug/snipe target).
                        if self.config.flash_crash_pct > 0.0 && checks >= 3 && prev_value > current_value {
                            let single_drop_pct = (prev_value as f64 - current_value as f64)
                                / self.entry_amount_raw as f64 * 100.0;
                            if single_drop_pct >= self.config.flash_crash_pct {
                                let pnl_lamports = current_value as i64 - self.entry_amount_raw as i64;
                                let pnl_sol = pnl_lamports as f64 / 1e9;
                                tracing::warn!(
                                    mint = %pool.base_mint,
                                    single_drop_pct = %format!("{:.1}%", single_drop_pct),
                                    pnl_sol,
                                    "⚡ Flash-crash detected — emergency exit"
                                );
                                self.sell_with_retry(&pool, &base_ata, amount, position_started.elapsed().as_secs_f64()).await;
                                self.record_sell_outcome(pnl_lamports > 0, pnl_lamports, pnl_sol, &pool.base_mint.to_string()).await;
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
                                / self.entry_amount_raw as f64 * 100.0;
                            velocity_window.push_back(delta_pct);
                            while velocity_window.len() > momentum_window_cap {
                                velocity_window.pop_front();
                            }

                            let avg_velocity: f64 = if velocity_window.is_empty() {
                                0.0
                            } else {
                                velocity_window.iter().sum::<f64>() / velocity_window.len() as f64
                            };

                            let current_pnl_pct = (current_value as f64 - self.entry_amount_raw as f64)
                                / self.entry_amount_raw as f64 * 100.0;
                            let peak_pnl_pct    = (peak_value    as f64 - self.entry_amount_raw as f64)
                                / self.entry_amount_raw as f64 * 100.0;
                            let pullback_pct = peak_pnl_pct - current_pnl_pct;

                            // (A) TP escalation: position is at/above current TP, momentum
                            //     is still strong → raise TP rather than exit. Capped at
                            //     `mom_max_esc` so the bot still books.
                            if current_pnl_pct >= dynamic_tp_pct
                                && avg_velocity > mom_threshold
                                && escalation_count < mom_max_esc
                                && velocity_window.len() >= momentum_window_cap
                            {
                                let new_tp = dynamic_tp_pct * mom_factor;
                                tracing::info!(
                                    mint = %pool.base_mint,
                                    old_tp_pct = %format!("{:.0}%", dynamic_tp_pct),
                                    new_tp_pct = %format!("{:.0}%", new_tp),
                                    avg_velocity_pct = %format!("{:.2}%", avg_velocity),
                                    escalation = escalation_count + 1,
                                    moon_chase,
                                    "🚀 TP escalated — momentum strong, holding for bigger move"
                                );
                                dynamic_tp_pct = new_tp;
                                escalation_count += 1;
                                // Recompute target_profit on the NEXT iteration; this one
                                // still uses the old target so we don't double-fire.
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
                            if peak_pnl_pct >= self.config.momentum_min_peak_pct
                                && pullback_pct >= pullback_eff
                            {
                                let pnl_lamports = current_value as i64 - self.entry_amount_raw as i64;
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
                                self.sell_with_retry(&pool, &base_ata, amount, position_started.elapsed().as_secs_f64()).await;
                                self.record_sell_outcome(pnl_lamports > 0, pnl_lamports, pnl_sol, &pool.base_mint.to_string()).await;
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
                                && current_pnl_pct >= self.config.velocity_decay_min_pnl_pct
                            {
                                decay_window.push_back(delta_pct);
                                while decay_window.len() > decay_window_cap {
                                    decay_window.pop_front();
                                }
                                if decay_window.len() == decay_window_cap {
                                    let prev_half_avg: f64 = decay_window
                                        .iter()
                                        .take(decay_half)
                                        .sum::<f64>() / decay_half as f64;
                                    let recent_half_avg: f64 = decay_window
                                        .iter()
                                        .skip(decay_half)
                                        .sum::<f64>() / decay_half as f64;
                                    let decay_drop = prev_half_avg - recent_half_avg;
                                    // Fire when velocity has fallen meaningfully
                                    // AND the previous half was still upward
                                    // (prevents firing after a recovery from a dip).
                                    if decay_drop >= self.config.velocity_decay_drop_threshold
                                        && prev_half_avg > 0.0
                                    {
                                        let pnl_lamports = current_value as i64 - self.entry_amount_raw as i64;
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
                                        self.sell_with_retry(&pool, &base_ata, amount, position_started.elapsed().as_secs_f64()).await;
                                        self.record_sell_outcome(pnl_lamports > 0, pnl_lamports, pnl_sol, &pool.base_mint.to_string()).await;
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
                            entry.peak_value_lamports    = peak_value;
                            entry.dynamic_tp_pct         = dynamic_tp_pct;
                            entry.escalations            = escalation_count;
                            entry.last_check_unix_secs   = chrono::Utc::now().timestamp();
                            entry.current_sl_lamports    = stop_loss_amount;
                            entry.current_sl_pct         = (stop_loss_amount as f64
                                - self.entry_amount_raw as f64)
                                / self.entry_amount_raw as f64 * 100.0;
                            entry.decline_streak         = decline_streak;
                        }

                        prev_value = current_value;

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
                            let current_pnl_pct_now = (current_value as f64 - self.entry_amount_raw as f64)
                                / self.entry_amount_raw as f64 * 100.0;
                            let mut any_fired_this_check = false;
                            let mut remaining_tokens = amount;
                            for (i, (trigger_pct, sell_pct)) in
                                self.config.tiered_partial_tp_levels.iter().enumerate()
                            {
                                if tiered_fired[i] || current_pnl_pct_now < *trigger_pct { continue; }
                                let partial_amount = (remaining_tokens as f64 * sell_pct / 100.0) as u64;
                                if partial_amount == 0 { tiered_fired[i] = true; continue; }
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
                                self.sell_with_retry(&pool, &base_ata, partial_amount, position_started.elapsed().as_secs_f64()).await;
                                remaining_tokens = remaining_tokens.saturating_sub(partial_amount);
                                tiered_fired[i] = true;
                                any_fired_this_check = true;
                                // Move stop to breakeven after the FIRST tier fires.
                                if i == 0 { stop_loss_amount = self.entry_amount_raw; }
                            }
                            // Set legacy partial_tp_done so the fallback path below
                            // doesn't fire on top of the tiered ladder.
                            if any_fired_this_check { partial_tp_done = true; }
                        } else if partial_tp_enabled && !partial_tp_done && current_value >= partial_tp_target {
                            // Legacy single partial-TP path (fallback when tiered disabled)
                            let partial_amount = (amount as f64 * self.config.partial_tp_pct / 100.0) as u64;
                            if partial_amount > 0 {
                                tracing::info!(mint = %pool.base_mint, "Partial TP triggered — selling {}%", self.config.partial_tp_pct);
                                self.sell_with_retry(&pool, &base_ata, partial_amount, position_started.elapsed().as_secs_f64()).await;
                                partial_tp_done = true;
                                stop_loss_amount = self.entry_amount_raw; // move stop to breakeven
                            }
                        }

                        // Dump detection: 3 consecutive declining checks after fast phase → exit.
                        // In profit-first mode we suppress this trigger unless the position
                        // is BOTH down meaningfully AND past the rug floor — otherwise the
                        // bot bails on every dip and never books any wins.
                        let dump_eligible = if self.config.profit_first_mode {
                            let override_lam = self.wallet_target_lamports_override.load(Ordering::Relaxed);
                            let target_lam = if override_lam > 0 {
                                override_lam as i64
                            } else {
                                (self.config.wallet_target_sol * 1e9) as i64
                            };
                            let approx_wallet = self.session_start_lamports.load(Ordering::Relaxed) as i64
                                + *self.daily_pnl_lamports.lock();
                            if target_lam > 0 && approx_wallet < target_lam {
                                // Approx-wallet < target → only honor dump-detection if we're already
                                // at or past the rug-only floor. Anything shallower, hold and wait.
                                current_value <= stop_loss_amount
                            } else { true }
                        } else { true };
                        if decline_streak >= 3 && checks >= fast_phase_checks && dump_eligible {
                            let pnl_lamports = current_value as i64 - self.entry_amount_raw as i64;
                            let pnl_sol = pnl_lamports as f64 / 1e9;
                            tracing::warn!(
                                mint = %pool.base_mint,
                                decline_streak, current_value, pnl_sol,
                                "Dump momentum detected — exiting position"
                            );
                            self.sell_with_retry(&pool, &base_ata, amount, position_started.elapsed().as_secs_f64()).await;
                            self.record_sell_outcome(pnl_lamports > 0, pnl_lamports, pnl_sol, &pool.base_mint.to_string()).await;
                            break;
                        }

                        if current_value >= target_profit || current_value <= stop_loss_amount {
                            let profitable = current_value >= target_profit;
                            let pnl_lamports = current_value as i64 - self.entry_amount_raw as i64;
                            let pnl_sol = pnl_lamports as f64 / 1e9;
                            let reason = if profitable { "take profit" } else { "stop loss" };
                            tracing::info!(
                                mint = %pool.base_mint,
                                current_value, target_profit,
                                stop_loss = stop_loss_amount, pnl_sol,
                                peak_value,
                                "Sell triggered: {}", reason
                            );
                            self.sell_with_retry(&pool, &base_ata, amount, position_started.elapsed().as_secs_f64()).await;
                            self.record_sell_outcome(profitable, pnl_lamports, pnl_sol, &pool.base_mint.to_string()).await;
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
                    } else { false }
                } else { false };

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
                        self.sell_with_retry(&pool, &base_ata, amount, position_started.elapsed().as_secs_f64()).await;
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
                        self.record_sell_outcome(pnl_lam > 0, pnl_lam, pnl_sol, &pool.base_mint.to_string()).await;
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
            let was_buy_limit = std::fs::read_to_string("scematica-sell-mode.json")
                .map(|s| s.contains("buy_limit"))
                .unwrap_or(false);
            if was_buy_limit {
                let prev = self.buy_count.swap(0, Ordering::Relaxed);
                self.sell_mode.store(false, Ordering::Relaxed);
                let _ = std::fs::remove_file("scematica-sell-mode.json");
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
                &spl_token::id(), &base_ata, &wallet_pk, &wallet_pk, &[],
            ) {
                Ok(ix) => ix,
                Err(e) => { tracing::debug!(mint = %mint_str, "ATA close ix build failed: {}", e); return; }
            };
            let blockhash = match rpc.get_latest_blockhash().await {
                Ok(b) => b,
                Err(e) => { tracing::debug!(mint = %mint_str, "ATA close: blockhash fetch failed: {}", e); return; }
            };
            let msg = solana_sdk::message::Message::new_with_blockhash(
                &[close_ix], Some(&wallet_pk), &blockhash,
            );
            let tx = solana_sdk::transaction::Transaction::new(&[&*wallet], msg, blockhash);
            match rpc.send_transaction(&tx).await {
                Ok(sig) => tracing::info!(mint = %mint_str, %sig, "ATA closed — ~0.002 SOL rent reclaimed"),
                Err(e) => tracing::debug!(mint = %mint_str, "ATA close failed (may be Token-2022): {}", e),
            }
        });
    }

    async fn record_sell_outcome(&self, profitable: bool, pnl_lamports: i64, pnl_sol: f64, mint: &str) {
        // Strategy agent history
        {
            let mut history = self.trade_history.lock();
            history.push((profitable, pnl_sol));
            if history.len() > 20 { history.remove(0); }
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
                    Ok(m) => m.mint_authority
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

        // Daily PnL accumulator
        {
            let mut daily = self.daily_pnl_lamports.lock();
            *daily += pnl_lamports;
        }

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
            alerts.send(
                &format!("{} SELL Confirmed", emoji),
                &format!("Mint: {}...\nPnL: {:.4} SOL", mint_short, pnl_sol),
            ).await;
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
    async fn sell_with_retry(&self, pool: &CachedPool, base_ata: &Pubkey, amount: u64, position_age_secs: f64) {
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
            if q < 10_000 {
                tracing::warn!(
                    mint = %pool.base_mint,
                    quote_vault_lamports = q,
                    "Pool quote vault drained (sell_with_retry) — writing total-loss event"
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
                }.append_to_file(TRADES_FILE);
                return;
            }
        }

        let rounds: &[(u64, u32)] = &[
            (0, 0),   // immediate, normal slippage
            (1, 1),   // 1 s wait, 2× slippage
            (3, 2),   // 3 s wait, min_out=0
            (8, 2),   // 8 s wait, min_out=0 final attempt
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
            match self.do_sell(pool, base_ata, current_amount, position_age_secs, *sell_round).await {
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
    async fn do_sell(&self, pool: &CachedPool, base_ata: &Pubkey, amount: u64, position_age_secs: f64, sell_round: u32) -> Result<()> {
        use scematica_core::token::apply_slippage;

        // Limit concurrent sell transactions to avoid 429 RPC hammering
        let _permit = self.sell_sem.acquire().await
            .map_err(|_| anyhow::anyhow!("sell semaphore closed"))?;

        self.metrics.record_trade_attempt();
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

        // Pool drain guard: if quote vault < 10_000 lamports the Raydium on-chain program
        // rejects every swap regardless of min_out (constant-product denominator collapses).
        // Writing a total-loss event and returning Ok(()) frees the processing lock so future
        // pools can be bought. Burning 4 rounds × max_retries on a drained pool would lock
        // the bot for 20+ seconds against a pool that can never fill the swap.
        const DRAIN_THRESHOLD_LAMPORTS: u64 = 10_000;
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
            }.append_to_file(TRADES_FILE);
            return Ok(());
        }

        // Escalate slippage by retry round to avoid repeated tx rejections:
        //   round 0: normal (e.g., 2.5%)
        //   round 1: 2× (5%)  — wider window for volatile price action
        //   round 2+: min_out=0 — accept any price, priority is closing the position
        let min_out = if self.dump_mode.load(Ordering::Relaxed) || sell_round >= 2 {
            0
        } else if estimated_out > 0 {
            let effective_slippage = self.config.sell_slippage_pct * (1.0 + sell_round as f64);
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

        let swap_ixs = self.raydium_builder.build_swap(
            &pool.id,
            &wallet_pubkey,
            &pool.base_mint,
            &self.quote_mint,
            base_ata,
            &quote_ata,
            amount,
            min_out,
        ).await?;

        let mut ixs = pre_ixs;
        ixs.extend(swap_ixs);

        // After selling base → WSOL, close the WSOL ATA to unwrap back to native SOL.
        if self.quote_mint == scematica_core::types::known_tokens::WSOL_MINT {
            ixs.push(
                spl_token::instruction::close_account(
                    &spl_token::id(),
                    &quote_ata,
                    &wallet_pubkey,
                    &wallet_pubkey,
                    &[],
                )?
            );
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
            match self.executor.execute(ixs.clone(), &self.wallet, &self.rpc).await {
                Ok(result) if result.confirmed => {
                    tracing::info!(mint = %pool.base_mint, sig = ?result.signature, "Sell confirmed");
                    self.metrics.record_trade_confirmed(0);

                    // pnl_pct is the primary reward signal for the NN agent — fall back
                    // to 0 when estimated_out couldn't be computed (RPC failure path).
                    let pnl_pct = if self.entry_amount_raw > 0 && estimated_out > 0 {
                        (estimated_out as f64 - self.entry_amount_raw as f64)
                            / self.entry_amount_raw as f64 * 100.0
                    } else { 0.0 };
                    TradeEvent {
                        timestamp: chrono::Utc::now(),
                        kind: "SELL".into(),
                        mint: pool.base_mint.to_string(),
                        symbol: String::new(),
                        amount: scematica_core::token::raw_to_ui(amount, pool.base_decimals),
                        pnl: (estimated_out as f64 - self.entry_amount_raw as f64) / 1_000_000_000.0,
                        status: "✓".into(),
                        signature: result.signature.map(|s| s.to_string()).unwrap_or_default(),
                        dex: "Raydium".into(),
                        hops: 1,
                        pnl_pct,
                        position_age_secs,
                    }.append_to_file(TRADES_FILE);

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
                    let is_slippage_or_timeout =
                        err.contains("0x26")
                        || err.contains("timeout after")
                        || err.contains("slippage");
                    if is_slippage_or_timeout && !tried_zero_slippage {
                        tried_zero_slippage = true;
                        // Refresh balance: if the slippage error actually meant the tx
                        // landed but we missed the confirmation, we'd be trying to sell
                        // zero tokens. A fresh read here prevents a "no tokens to sell"
                        // fail-loop that burns remaining retries uselessly.
                        let live_amount = if let Ok(bal) = self.rpc.get_token_account_balance(base_ata).await {
                            let fresh = bal.amount.parse::<u64>().unwrap_or(0);
                            if fresh == 0 {
                                tracing::info!(mint = %pool.base_mint, "Token already fully sold (zero-slippage rebuild) — exiting");
                                return Ok(());
                            }
                            fresh
                        } else {
                            amount  // RPC failed — use original amount conservatively
                        };
                        tracing::warn!(mint = %pool.base_mint, error = %err, live_amount, "Slippage/timeout — rebuilding sell with min_out=0");
                        if let Ok(swap_rebuilt) = self.raydium_builder.build_swap(
                            &pool.id,
                            &wallet_pubkey,
                            &pool.base_mint,
                            &self.quote_mint,
                            base_ata,
                            &quote_ata,
                            live_amount,
                            0,
                        ).await {
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
                                    &spl_token::id(), &quote_ata, &wallet_pubkey, &wallet_pubkey, &[],
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
            pnl_pct: -100.0,           // total loss signal for the NN agent
            position_age_secs,
        }.append_to_file(TRADES_FILE);

        anyhow::bail!("sell exhausted {} retries for {}", self.config.max_sell_retries, pool.base_mint)
    }
}
