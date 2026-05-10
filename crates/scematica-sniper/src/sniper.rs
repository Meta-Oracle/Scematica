use anyhow::Result;
use parking_lot::Mutex;
use scematica_core::{
    config::SniperConfig,
    metrics::BotMetrics,
    token::{apply_slippage, get_ata, resolve_mint, ui_to_raw},
    types::known_tokens,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
};
use spl_associated_token_account::instruction::create_associated_token_account_idempotent;
use spl_token::instruction as token_ix;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::{
    cache::{CachedPool, MarketCache, PoolCache, SnipeListCache},
    executor::{DefaultExecutor, JitoExecutor, TxExecutor},
    filters::FilterPipeline,
    listener::ListenerEvent,
};

/// Core sniper bot: receives pool events and executes buy/sell
pub struct Sniper {
    config: SniperConfig,
    wallet: Arc<Keypair>,
    rpc: Arc<RpcClient>,
    pool_cache: PoolCache,
    market_cache: MarketCache,
    snipe_list: Option<SnipeListCache>,
    filter_pipeline: FilterPipeline,
    executor: Arc<dyn TxExecutor>,
    metrics: Arc<BotMetrics>,
    /// Mutex to enforce one-token-at-a-time
    processing_lock: Arc<Mutex<bool>>,
    quote_mint: Pubkey,
    quote_decimals: u8,
    quote_amount_raw: u64,
}

impl Sniper {
    pub fn new(
        config: SniperConfig,
        wallet: Arc<Keypair>,
        rpc: Arc<RpcClient>,
        metrics: Arc<BotMetrics>,
    ) -> Self {
        let quote_mint = resolve_mint(&config.quote_mint)
            .unwrap_or(known_tokens::WSOL_MINT);
        let quote_decimals = if config.quote_mint.to_uppercase() == "USDC" { 6 } else { 9 };
        let quote_amount_raw = ui_to_raw(config.quote_amount, quote_decimals);

        let filter_pipeline = FilterPipeline::new(config.filters.clone(), rpc.clone());

        let executor: Arc<dyn TxExecutor> = match config.quote_mint.as_str() {
            _ if std::env::var("EXECUTOR").unwrap_or_default() == "jito" => {
                Arc::new(JitoExecutor::new(
                    std::env::var("JITO_URL")
                        .unwrap_or_else(|_| "https://mainnet.block-engine.jito.wtf".into()),
                    0.006,
                ))
            }
            _ => Arc::new(DefaultExecutor::new(200_000, 100_000, true, 3)),
        };

        let snipe_list = if config.use_snipe_list {
            let sl = SnipeListCache::new(&config.snipe_list_path);
            let _ = sl.load();
            Some(sl)
        } else {
            None
        };

        Self {
            config,
            wallet,
            rpc,
            pool_cache: PoolCache::new(),
            market_cache: MarketCache::new(),
            snipe_list,
            filter_pipeline,
            executor,
            metrics,
            processing_lock: Arc::new(Mutex::new(false)),
            quote_mint,
            quote_decimals,
            quote_amount_raw,
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

        // One-token-at-a-time check
        if self.config.one_token_at_a_time {
            let locked = self.processing_lock.lock();
            if *locked {
                debug!(mint = %pool.base_mint, "Skipping: already processing a token");
                return;
            }
        }

        info!(mint = %pool.base_mint, pool = %pool.id, "New pool detected — evaluating");

        // Cache the pool
        self.pool_cache.save(&pool.id.to_string(), pool.clone());

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

        // Execute buy
        self.buy(&pool).await;
    }

    async fn buy(&self, pool: &CachedPool) {
        info!(
            mint = %pool.base_mint,
            amount = self.config.quote_amount,
            quote = %self.config.quote_mint,
            "Executing buy"
        );

        self.metrics.record_trade_attempt();

        // Lock if one-at-a-time
        if self.config.one_token_at_a_time {
            *self.processing_lock.lock() = true;
        }

        let wallet_pubkey = self.wallet.pubkey();
        let quote_ata = get_ata(&wallet_pubkey, &self.quote_mint);
        let base_ata = get_ata(&wallet_pubkey, &pool.base_mint);

        // Build swap instructions
        let ixs = self.build_raydium_swap_ixs(
            pool,
            &quote_ata,
            &base_ata,
            self.quote_amount_raw,
            0, // min_out: 0 for now, slippage applied in swap
            true,
        );

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

                    // Schedule auto-sell if enabled
                    if self.config.auto_sell {
                        let pool_clone = pool.clone();
                        let monitor = self.clone_for_sell();
                        tokio::spawn(async move {
                            monitor.monitor_and_sell(pool_clone, base_ata).await;
                        });
                    }
                    return;
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
        if self.config.one_token_at_a_time {
            *self.processing_lock.lock() = false;
        }
    }

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

        let take_profit_factor = 1.0 + self.config.take_profit_pct / 100.0;
        let stop_loss_factor = 1.0 - self.config.stop_loss_pct / 100.0;
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
                                break;
                            }
                            if current_value <= stop_loss {
                                info!(mint = %pool.base_mint, "Stop loss triggered");
                                self.sell(&pool, &base_ata, amount).await;
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
            *self.processing_lock.lock() = false;
        }
    }

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

        let ixs = self.build_raydium_swap_ixs(
            pool,
            base_ata,
            &quote_ata,
            amount,
            min_out,
            false,
        );

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
    }

    async fn on_wallet_update(&self, account: Pubkey, mint: Pubkey, amount: u64) {
        // Triggered when a token account in our wallet changes
        // Used to detect received tokens and trigger sell logic
        debug!(account = %account, mint = %mint, amount, "Wallet update");
    }

    /// Build Raydium V4 swap instructions
    /// This is a simplified version — full implementation requires
    /// the Raydium SDK accounts (market, open orders, etc.)
    fn build_raydium_swap_ixs(
        &self,
        pool: &CachedPool,
        ata_in: &Pubkey,
        ata_out: &Pubkey,
        amount_in: u64,
        min_amount_out: u64,
        is_buy: bool,
    ) -> Vec<Instruction> {
        let wallet_pubkey = self.wallet.pubkey();
        let mut ixs = vec![];

        // Create output ATA if needed (idempotent)
        let out_mint = if is_buy { pool.base_mint } else { self.quote_mint };
        ixs.push(create_associated_token_account_idempotent(
            &wallet_pubkey,
            &wallet_pubkey,
            &out_mint,
            &spl_token::id(),
        ));

        // NOTE: Full Raydium V4 swap instruction requires:
        // amm_id, amm_authority, amm_open_orders, amm_target_orders,
        // pool_coin_token_account, pool_pc_token_account,
        // serum_program_id, serum_market, serum_bids, serum_asks,
        // serum_event_queue, serum_coin_vault, serum_pc_vault, serum_vault_signer
        // This is built in the scematica-executor crate with full account resolution.
        // Placeholder instruction added here for compilation.
        tracing::debug!(
            pool = %pool.id,
            amount_in,
            min_amount_out,
            is_buy,
            "Raydium swap ix (full impl in scematica-executor)"
        );

        ixs
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
    processing_lock: Arc<Mutex<bool>>,
    quote_mint: Pubkey,
    quote_amount_raw: u64,
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

        let take_profit_factor = 1.0 + self.config.take_profit_pct / 100.0;
        let stop_loss_factor = 1.0 - self.config.stop_loss_pct / 100.0;
        let target_profit = (self.quote_amount_raw as f64 * take_profit_factor) as u64;
        let stop_loss_amount = (self.quote_amount_raw as f64 * stop_loss_factor) as u64;

        let mut checks = 0u64;
        loop {
            match self.rpc.get_token_account_balance(&base_ata).await {
                Ok(balance) => {
                    let amount: u64 = balance.amount.parse().unwrap_or(0);
                    if amount == 0 {
                        break;
                    }

                    if let (Ok(qb), Ok(bb)) = (
                        self.rpc.get_token_account_balance(&pool.quote_vault).await,
                        self.rpc.get_token_account_balance(&pool.base_vault).await,
                    ) {
                        let q: u64 = qb.amount.parse().unwrap_or(1);
                        let b: u64 = bb.amount.parse().unwrap_or(1);
                        let current_value = (amount as u128 * q as u128 / b as u128) as u64;

                        if current_value >= target_profit || current_value <= stop_loss_amount {
                            self.do_sell(&pool, &base_ata, amount).await;
                            break;
                        }
                    }
                }
                Err(_) => {}
            }

            checks += 1;
            if checks >= max_checks {
                if let Ok(balance) = self.rpc.get_token_account_balance(&base_ata).await {
                    let amount: u64 = balance.amount.parse().unwrap_or(0);
                    if amount > 0 {
                        self.do_sell(&pool, &base_ata, amount).await;
                    }
                }
                break;
            }
            tokio::time::sleep(interval).await;
        }

        if self.config.one_token_at_a_time {
            *self.processing_lock.lock() = false;
        }
    }

    async fn do_sell(&self, pool: &CachedPool, base_ata: &Pubkey, amount: u64) {
        use scematica_core::token::apply_slippage;
        use spl_associated_token_account::instruction::create_associated_token_account_idempotent;

        self.metrics.record_trade_attempt();
        let wallet_pubkey = self.wallet.pubkey();
        let quote_ata = get_ata(&wallet_pubkey, &self.quote_mint);

        let min_out = apply_slippage(
            (amount as f64 * 0.99) as u64,
            self.config.sell_slippage_pct,
        );

        let mut ixs = vec![create_associated_token_account_idempotent(
            &wallet_pubkey,
            &wallet_pubkey,
            &self.quote_mint,
            &spl_token::id(),
        )];

        tracing::debug!(pool = %pool.id, amount, min_out, "Sell ix (full impl in scematica-executor)");

        for attempt in 0..self.config.max_sell_retries {
            match self.executor.execute(ixs.clone(), &self.wallet, &self.rpc).await {
                Ok(result) if result.confirmed => {
                    tracing::info!(mint = %pool.base_mint, sig = ?result.signature, "Sell confirmed");
                    self.metrics.record_trade_confirmed(0);
                    return;
                }
                Ok(result) => {
                    tracing::warn!(error = ?result.error, "Sell attempt {} failed", attempt + 1);
                }
                Err(e) => {
                    tracing::error!("Sell error: {}", e);
                }
            }
        }
        self.metrics.record_trade_failed();
    }
}
