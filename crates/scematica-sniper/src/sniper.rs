use parking_lot::{Mutex, RwLock};
use spl_associated_token_account;
use std::sync::atomic::{AtomicBool, Ordering};
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
    cache::{CachedPool, MarketCache, PoolCache, SnipeListCache},
    executor::{DefaultExecutor, JitoExecutor, TxExecutor},
    filters::FilterPipeline,
    listener::ListenerEvent,
};
use scematica_core::metrics::{TradeEvent, TRADES_FILE};
use scematica_core::metrics::{StrategySnapshot, STRATEGY_FILE};

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

        let filter_pipeline = FilterPipeline::new(
            config.filters.clone(),
            rpc.clone(),
            quote_amount_raw,
            &config.blacklist_path,
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
            sell_sem: Arc::new(tokio::sync::Semaphore::new(1)),
            buy_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            open_positions: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            consecutive_losses: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            cooldown_until_ms: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            daily_pnl_lamports: Arc::new(parking_lot::Mutex::new(0i64)),
            session_start_lamports: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            alerts,
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
            live.market_regime = adjustment.market_regime.clone();

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

        // Skip if already processed
        if self.pool_cache.contains(&pool.id.to_string()) {
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

        // Skip all buys when sell mode or dump mode is active
        if self.sell_mode.load(Ordering::Relaxed) || self.dump_mode.load(Ordering::Relaxed) {
            debug!(mint = %pool.base_mint, "Sell/dump mode active — skipping buy");
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

        // Cooldown after consecutive losses
        if self.config.cooldown_after_losses > 0 {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let until = self.cooldown_until_ms.load(Ordering::Relaxed);
            if until > now_ms {
                let remaining = (until - now_ms) / 1000;
                debug!(mint = %pool.base_mint, remaining_secs = remaining, "Cooldown active — skipping buy");
                return;
            }
        }

        // Daily loss limit
        if self.config.daily_loss_limit_sol > 0.0 {
            let loss_lamports = -*self.daily_pnl_lamports.lock();
            let loss_sol = loss_lamports as f64 / 1e9;
            if loss_sol >= self.config.daily_loss_limit_sol {
                warn!(mint = %pool.base_mint, loss_sol, "Daily loss limit reached — skipping buy");
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

        // Apply buy delay
        if self.config.buy_delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(self.config.buy_delay_ms)).await;
        }

        // Run filters (unless snipe list mode)
        if self.snipe_list.is_none() {
            let passes = self.filter_pipeline.execute(&pool).await;
            if !passes {
                info!(mint = %pool.base_mint, "Pool rejected by filters");
                return;
            }
        }

        // AI risk assessment (if available)
        if let Some(ai) = &self.ai {
            let pool_size_sol = scematica_core::token::raw_to_ui(
                // rough estimate from quote vault — 0 if unavailable
                self.rpc.get_token_account_balance(&pool.quote_vault).await
                    .ok()
                    .and_then(|b| b.amount.parse::<u64>().ok())
                    .unwrap_or(0),
                pool.quote_decimals,
            );

            // UTC hour from open_time (unix timestamp)
            let open_hour = (pool.open_time % 86400 / 3600) as u8;

            // Pool already passed all enabled filters at this point, so:
            // - mint_renounced = true  (MintRenounceFilter passed or was disabled)
            // - freezable = false      (FreezableFilter passed or was disabled)
            // - lp_burned = true       (BurnFilter passed or was disabled)
            // Pass the known-good state so the AI isn't told the token is risky.
            let mint_renounced = self.config.filters.check_mint_renounced; // enabled → verified
            let freezable = false; // FreezableFilter already confirmed not freezable
            let lp_burned = self.config.filters.check_burned;
            let mutable_metadata = !self.config.filters.check_mutable; // mutable only if we didn't check

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
        }

        // Execute buy
        if let Err(e) = self.buy(&pool).await {
            error!(mint = %pool.base_mint, "buy() error: {}", e);
        }
    }

    async fn buy(&self, pool: &CachedPool) -> Result<()> {
        info!(
            mint = %pool.base_mint,
            amount = self.config.quote_amount,
            quote = %self.config.quote_mint,
            "Executing buy"
        );

        self.metrics.record_trade_attempt();

        let wallet_pubkey = self.wallet.pubkey();
        let quote_ata = get_ata(&wallet_pubkey, &self.quote_mint);
        let base_ata = get_ata(&wallet_pubkey, &pool.base_mint);

        // Gate: ensure we have enough native SOL for quote amount + fees + ATA rent.
        // 6_000_000 lamports (0.006 SOL) covers tx fee, compute budget, and 2× ATA rent.
        let native_balance = self.rpc.get_balance(&wallet_pubkey).await.unwrap_or(0);
        let min_required = self.quote_amount_raw + 6_000_000;
        if native_balance < min_required {
            warn!(
                mint = %pool.base_mint,
                balance_sol = native_balance as f64 / 1e9,
                required_sol = min_required as f64 / 1e9,
                "Insufficient SOL — skipping buy"
            );
            return Ok(());
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
            self.quote_amount_raw,
            0,
        ).await {
            Ok(ixs) => ixs,
            Err(e) => {
                error!(mint = %pool.base_mint, "Failed to build swap instructions: {}", e);
                self.metrics.record_trade_failed();
                return Err(e);
            }
        };

        // Atomically claim the processing slot — skip buy if another position owns it.
        // Using compare_exchange so the check+set is race-free (unlike the old Mutex<bool> pattern).
        if self.config.one_token_at_a_time {
            if self.processing_lock
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                debug!(mint = %pool.base_mint, "one_token_at_a_time: slot taken, skipping buy");
                return Ok(());
            }
        }

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
                self.quote_amount_raw,
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


        for attempt in 0..self.config.max_buy_retries {
            info!("Buy attempt {}/{}", attempt + 1, self.config.max_buy_retries);
            match self.executor.execute(ixs.clone(), &self.wallet, &self.rpc).await {
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
                        amount: scematica_core::token::raw_to_ui(self.quote_amount_raw, self.quote_decimals),
                        pnl: 0.0,
                        status: "✓".into(),
                        signature: result.signature
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                        dex: "Raydium".into(),
                        hops: 1,
                    }.append_to_file(TRADES_FILE);

                    // Track open positions
                    self.open_positions.fetch_add(1, Ordering::Relaxed);

                    // Alert: buy confirmed
                    {
                        let alerts = self.alerts.clone();
                        let mint_str = pool.base_mint.to_string();
                        let amount_sol = scematica_core::token::raw_to_ui(self.quote_amount_raw, self.quote_decimals);
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

                    // Schedule auto-sell — the monitor task owns the lock release.
                    // If auto_sell is disabled we release the lock here instead.
                    if self.config.auto_sell {
                        let pool_clone = pool.clone();
                        let monitor = self.clone_for_sell();
                        tokio::spawn(async move {
                            monitor.monitor_and_sell(pool_clone, base_ata).await;
                        });
                    } else if self.config.one_token_at_a_time {
                        self.processing_lock.store(false, Ordering::Relaxed);
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
            amount: scematica_core::token::raw_to_ui(self.quote_amount_raw, self.quote_decimals),
            pnl: 0.0,
            status: "✗".into(),
            signature: String::new(),
            dex: "Raydium".into(),
            hops: 1,
        }.append_to_file(TRADES_FILE);
        if self.config.one_token_at_a_time {
            self.processing_lock.store(false, Ordering::Relaxed);
        }

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

        if self.config.one_token_at_a_time {
            self.processing_lock.store(false, Ordering::Relaxed);
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

            let monitor = self.clone_for_sell();
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
            let monitor = self.clone_for_sell();
            tokio::spawn(async move {
                monitor.sell_with_retry(&pool, &ata_pk, amount).await;
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

    /// Clone the parts needed for the sell monitor task
    fn clone_for_sell(&self) -> SellMonitor {
        SellMonitor {
            config: self.config.clone(),
            wallet: self.wallet.clone(),
            rpc: self.rpc.clone(),
            executor: self.executor.clone(),
            metrics: self.metrics.clone(),
            processing_lock: self.processing_lock.clone(),
            quote_mint: self.quote_mint,
            quote_amount_raw: self.quote_amount_raw,
            raydium_builder: self.raydium_builder.clone(),
            live_params: self.live_params.clone(),
            trade_history: self.trade_history.clone(),
            sell_mode: self.sell_mode.clone(),
            dump_mode: self.dump_mode.clone(),
            sell_sem: self.sell_sem.clone(),
            open_positions: self.open_positions.clone(),
            consecutive_losses: self.consecutive_losses.clone(),
            cooldown_until_ms: self.cooldown_until_ms.clone(),
            daily_pnl_lamports: self.daily_pnl_lamports.clone(),
            alerts: self.alerts.clone(),
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
    processing_lock: Arc<std::sync::atomic::AtomicBool>,
    quote_mint: Pubkey,
    quote_amount_raw: u64,
    raydium_builder: Arc<dyn SwapInstructionBuilder>,
    live_params: Arc<RwLock<LiveParams>>,
    trade_history: Arc<Mutex<Vec<(bool, f64)>>>,
    sell_mode: Arc<AtomicBool>,
    dump_mode: Arc<AtomicBool>,
    sell_sem: Arc<tokio::sync::Semaphore>,
    open_positions: Arc<std::sync::atomic::AtomicU32>,
    consecutive_losses: Arc<std::sync::atomic::AtomicU32>,
    cooldown_until_ms: Arc<std::sync::atomic::AtomicU64>,
    daily_pnl_lamports: Arc<parking_lot::Mutex<i64>>,
    alerts: Arc<AlertManager>,
}

impl SellMonitor {
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

        // Use live_params so Strategy Agent adjustments apply to the sell monitor too
        let (take_profit_pct, stop_loss_pct) = {
            let p = self.live_params.read();
            (p.take_profit_pct, p.stop_loss_pct)
        };
        let take_profit_factor = 1.0 + take_profit_pct / 100.0;
        let stop_loss_factor  = 1.0 - stop_loss_pct / 100.0;
        let target_profit     = (self.quote_amount_raw as f64 * take_profit_factor) as u64;
        let mut stop_loss_amount = (self.quote_amount_raw as f64 * stop_loss_factor) as u64;

        // Partial TP state
        let partial_tp_enabled = self.config.partial_tp_pct > 0.0 && self.config.partial_tp_trigger > 0.0;
        let partial_tp_target  = (self.quote_amount_raw as f64 * (1.0 + self.config.partial_tp_trigger / 100.0)) as u64;
        let mut partial_tp_done = false;

        // Trailing stop state
        let trailing_enabled = self.config.trailing_stop_loss_pct > 0.0;
        let mut peak_value: u64 = self.quote_amount_raw; // initialise at buy price

        let mut checks = 0u64;
        loop {
            // Sell/dump mode activated mid-monitor → sell immediately
            if self.sell_mode.load(Ordering::Relaxed) || self.dump_mode.load(Ordering::Relaxed) {
                tracing::info!(mint = %pool.base_mint, "Sell/dump mode active — forcing immediate sell");
                if let Ok(balance) = self.rpc.get_token_account_balance(&base_ata).await {
                    let amount: u64 = balance.amount.parse().unwrap_or(0);
                    if amount > 0 {
                        self.sell_with_retry(&pool, &base_ata, amount).await;
                    }
                }
                break;
            }

            match self.rpc.get_token_account_balance(&base_ata).await {
                Ok(balance) => {
                    let amount: u64 = balance.amount.parse().unwrap_or(0);
                    if amount == 0 { break; }

                    if let (Ok(qb), Ok(bb)) = (
                        self.rpc.get_token_account_balance(&pool.quote_vault).await,
                        self.rpc.get_token_account_balance(&pool.base_vault).await,
                    ) {
                        let q: u64 = qb.amount.parse().unwrap_or(1);
                        let b: u64 = bb.amount.parse().unwrap_or(1);
                        let current_value = (amount as u128 * q as u128 / b as u128) as u64;

                        // Update peak and trailing stop
                        if trailing_enabled {
                            if current_value > peak_value { peak_value = current_value; }
                            let trail = (peak_value as f64 * (1.0 - self.config.trailing_stop_loss_pct / 100.0)) as u64;
                            if trail > stop_loss_amount { stop_loss_amount = trail; }
                        }

                        // Partial TP: sell half, move stop to breakeven
                        if partial_tp_enabled && !partial_tp_done && current_value >= partial_tp_target {
                            let partial_amount = (amount as f64 * self.config.partial_tp_pct / 100.0) as u64;
                            if partial_amount > 0 {
                                tracing::info!(mint = %pool.base_mint, "Partial TP triggered — selling {}%", self.config.partial_tp_pct);
                                self.sell_with_retry(&pool, &base_ata, partial_amount).await;
                                partial_tp_done = true;
                                // Move stop to breakeven
                                stop_loss_amount = self.quote_amount_raw;
                            }
                        }

                        if current_value >= target_profit || current_value <= stop_loss_amount {
                            let profitable = current_value >= target_profit;
                            let pnl_lamports = current_value as i64 - self.quote_amount_raw as i64;
                            let pnl_sol = pnl_lamports as f64 / 1e9;
                            let reason = if profitable { "take profit" } else { "stop loss" };
                            tracing::info!(
                                mint = %pool.base_mint,
                                current_value, target_profit,
                                stop_loss = stop_loss_amount, pnl_sol,
                                peak_value,
                                "Sell triggered: {}", reason
                            );
                            self.sell_with_retry(&pool, &base_ata, amount).await;
                            self.record_sell_outcome(profitable, pnl_lamports, pnl_sol, &pool.base_mint.to_string()).await;
                            break;
                        }
                    }
                }
                Err(_) => {}
            }

            checks += 1;
            if checks >= max_checks {
                tracing::info!(mint = %pool.base_mint, "Price check window expired — force selling");
                if let Ok(balance) = self.rpc.get_token_account_balance(&base_ata).await {
                    let amount: u64 = balance.amount.parse().unwrap_or(0);
                    if amount > 0 {
                        self.sell_with_retry(&pool, &base_ata, amount).await;
                        self.record_sell_outcome(false, -(self.quote_amount_raw as i64 / 10), -0.001, &pool.base_mint.to_string()).await;
                    }
                }
                break;
            }
            tokio::time::sleep(interval).await;
        }

        // Position closed — decrement counter
        self.open_positions.fetch_sub(1, Ordering::Relaxed);
        if self.config.one_token_at_a_time {
            self.processing_lock.store(false, Ordering::Relaxed);
        }
    }

    async fn record_sell_outcome(&self, profitable: bool, pnl_lamports: i64, pnl_sol: f64, mint: &str) {
        // Strategy agent history
        {
            let mut history = self.trade_history.lock();
            history.push((profitable, pnl_sol));
            if history.len() > 20 { history.remove(0); }
        }

        // Daily PnL accumulator
        {
            let mut daily = self.daily_pnl_lamports.lock();
            *daily += pnl_lamports;
        }

        // Consecutive loss / win tracking + cooldown
        if profitable {
            self.consecutive_losses.store(0, Ordering::Relaxed);
        } else {
            let losses = self.consecutive_losses.fetch_add(1, Ordering::Relaxed) + 1;
            if self.config.cooldown_after_losses > 0 && losses >= self.config.cooldown_after_losses {
                let cooldown_ms = self.config.cooldown_minutes as u64 * 60_000;
                let until_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64 + cooldown_ms;
                self.cooldown_until_ms.store(until_ms, Ordering::Relaxed);
                tracing::warn!(
                    "Cooldown activated after {} consecutive losses — pausing buys for {} min",
                    losses, self.config.cooldown_minutes
                );
                let alerts = self.alerts.clone();
                let mins = self.config.cooldown_minutes;
                tokio::spawn(async move {
                    alerts.send("Cooldown Activated", &format!("{} consecutive losses — pausing {}min", losses, mins)).await;
                });
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

    /// Persistent sell wrapper: retries `do_sell` up to 3 rounds with a 30 s pause.
    /// Refreshes the token balance before each round so stale amounts don't block later rounds.
    async fn sell_with_retry(&self, pool: &CachedPool, base_ata: &Pubkey, amount: u64) {
        // Dump mode: retry every 5 s. Normal mode: 15 s between rounds (was 30 s).
        let retry_delay_secs: u64 = if self.dump_mode.load(Ordering::Relaxed) { 5 } else { 15 };
        let mut current_amount = amount;
        for round in 0..3u32 {
            // Refresh balance — token may have been partially or fully sold already
            if let Ok(bal) = self.rpc.get_token_account_balance(base_ata).await {
                let fresh = bal.amount.parse::<u64>().unwrap_or(0);
                if fresh == 0 {
                    tracing::info!(mint = %pool.base_mint, "Token fully sold");
                    return;
                }
                current_amount = fresh;
            }
            match self.do_sell(pool, base_ata, current_amount).await {
                Ok(()) => return,
                Err(e) => {
                    let is_last = round >= 2;
                    let next_action = if is_last {
                        "giving up".to_string()
                    } else {
                        format!("retrying in {}s", retry_delay_secs)
                    };
                    tracing::error!(
                        mint = %pool.base_mint,
                        round = round + 1,
                        "Sell round failed: {} — {}",
                        e,
                        next_action
                    );
                    if !is_last {
                        tokio::time::sleep(tokio::time::Duration::from_secs(retry_delay_secs)).await;
                    }
                }
            }
        }
    }

    async fn do_sell(&self, pool: &CachedPool, base_ata: &Pubkey, amount: u64) -> Result<()> {
        use scematica_core::token::apply_slippage;

        // Limit concurrent sell transactions to avoid 429 RPC hammering
        let _permit = self.sell_sem.acquire().await
            .map_err(|_| anyhow::anyhow!("sell semaphore closed"))?;

        self.metrics.record_trade_attempt();
        let wallet_pubkey = self.wallet.pubkey();
        let quote_ata = get_ata(&wallet_pubkey, &self.quote_mint);

        // Estimate current quote output from pool reserves so min_out is in lamports.
        // Falls back to 0 (no minimum) if reserve fetch fails — still better than using
        // base token raw amount as min_out, which causes every sell to fail slippage checks.
        let estimated_out = if let (Ok(qb), Ok(bb)) = (
            self.rpc.get_token_account_balance(&pool.quote_vault).await,
            self.rpc.get_token_account_balance(&pool.base_vault).await,
        ) {
            let q: u64 = qb.amount.parse().unwrap_or(1);
            let b: u64 = bb.amount.parse().unwrap_or(1);
            (amount as u128 * q as u128 / b as u128) as u64
        } else {
            0
        };
        // Dump mode: accept any output — skip slippage check entirely
        let min_out = if self.dump_mode.load(Ordering::Relaxed) {
            0
        } else if estimated_out > 0 {
            apply_slippage(estimated_out, self.config.sell_slippage_pct)
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

                    TradeEvent {
                        timestamp: chrono::Utc::now(),
                        kind: "SELL".into(),
                        mint: pool.base_mint.to_string(),
                        symbol: String::new(),
                        amount: scematica_core::token::raw_to_ui(amount, pool.base_decimals),
                        pnl: (estimated_out as f64 - self.quote_amount_raw as f64) / 1_000_000_000.0,
                        status: "✓".into(),
                        signature: result.signature.map(|s| s.to_string()).unwrap_or_default(),
                        dex: "Raydium".into(),
                        hops: 1,
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
                    // 0x26 = Raydium slippage check; rebuild with min_out=0 without consuming a retry
                    if err.contains("0x26") && !tried_zero_slippage {
                        tried_zero_slippage = true;
                        tracing::warn!(mint = %pool.base_mint, "0x26 slippage — rebuilding sell with min_out=0");
                        if let Ok(swap_rebuilt) = self.raydium_builder.build_swap(
                            &pool.id,
                            &wallet_pubkey,
                            &pool.base_mint,
                            &self.quote_mint,
                            base_ata,
                            &quote_ata,
                            amount,
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
        }.append_to_file(TRADES_FILE);

        anyhow::bail!("sell exhausted {} retries for {}", self.config.max_sell_retries, pool.base_mint)
    }
}
