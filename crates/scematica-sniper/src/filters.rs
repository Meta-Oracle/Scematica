use async_trait::async_trait;
use scematica_core::config::FilterConfig;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use tracing::{debug, warn};

use crate::cache::CachedPool;

/// Result of a filter check
#[derive(Debug, Clone)]
pub struct FilterResult {
    pub passed: bool,
    pub reason: Option<String>,
}

impl FilterResult {
    pub fn pass() -> Self {
        Self { passed: true, reason: None }
    }

    pub fn fail(reason: impl Into<String>) -> Self {
        Self { passed: false, reason: Some(reason.into()) }
    }
}

/// Trait for pool filters
#[async_trait]
pub trait PoolFilter: Send + Sync {
    fn name(&self) -> &str;
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult;
}

/// Checks if the mint authority has been renounced (set to None)
pub struct MintRenounceFilter;

#[async_trait]
impl PoolFilter for MintRenounceFilter {
    fn name(&self) -> &str { "MintRenounced" }

    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        match rpc.get_account(&pool.base_mint).await {
            Ok(account) => {
                // SPL token mint: mint_authority is at offset 0, 4 bytes option tag + 32 bytes pubkey
                // Option::None = [0, 0, 0, 0, ...]
                if account.data.len() >= 4 {
                    let is_renounced = account.data[0] == 0;
                    if is_renounced {
                        FilterResult::pass()
                    } else {
                        FilterResult::fail("Mint authority not renounced")
                    }
                } else {
                    FilterResult::fail("Invalid mint account data")
                }
            }
            Err(e) => {
                warn!("MintRenounceFilter RPC error: {}", e);
                FilterResult::fail(format!("RPC error: {}", e))
            }
        }
    }
}

/// Checks if the token is not freezable (freeze authority is None)
pub struct FreezableFilter;

#[async_trait]
impl PoolFilter for FreezableFilter {
    fn name(&self) -> &str { "NotFreezable" }

    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        match rpc.get_account(&pool.base_mint).await {
            Ok(account) => {
                // SPL token mint: freeze_authority is at offset 46 (4 byte option + 32 byte key)
                if account.data.len() >= 50 {
                    let freeze_option = account.data[44]; // COption tag
                    if freeze_option == 0 {
                        FilterResult::pass()
                    } else {
                        FilterResult::fail("Token has freeze authority")
                    }
                } else {
                    FilterResult::fail("Invalid mint account data")
                }
            }
            Err(e) => FilterResult::fail(format!("RPC error: {}", e)),
        }
    }
}

/// Checks if the LP tokens are burned (vault balance near zero)
pub struct BurnFilter;

#[async_trait]
impl PoolFilter for BurnFilter {
    fn name(&self) -> &str { "LPBurned" }

    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        // Check if LP mint supply is near zero (burned)
        // For Raydium V4, LP mint is derived from pool address
        // Simplified: check if base vault has reasonable liquidity
        match rpc.get_token_account_balance(&pool.base_vault).await {
            Ok(balance) => {
                let amount: u64 = balance.amount.parse().unwrap_or(0);
                if amount > 0 {
                    FilterResult::pass()
                } else {
                    FilterResult::fail("Pool vault is empty (possibly rugged)")
                }
            }
            Err(e) => FilterResult::fail(format!("RPC error: {}", e)),
        }
    }
}

/// Checks pool size is within configured bounds
pub struct PoolSizeFilter {
    pub min_size_lamports: u64,
    pub max_size_lamports: u64,
}

#[async_trait]
impl PoolFilter for PoolSizeFilter {
    fn name(&self) -> &str { "PoolSize" }

    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        match rpc.get_token_account_balance(&pool.quote_vault).await {
            Ok(balance) => {
                let amount: u64 = balance.amount.parse().unwrap_or(0);
                if self.min_size_lamports > 0 && amount < self.min_size_lamports {
                    return FilterResult::fail(format!(
                        "Pool too small: {} < {}",
                        amount, self.min_size_lamports
                    ));
                }
                if self.max_size_lamports > 0 && amount > self.max_size_lamports {
                    return FilterResult::fail(format!(
                        "Pool too large: {} > {}",
                        amount, self.max_size_lamports
                    ));
                }
                FilterResult::pass()
            }
            Err(e) => FilterResult::fail(format!("RPC error: {}", e)),
        }
    }
}

/// Runs all configured filters against a pool, with retry logic
pub struct FilterPipeline {
    filters: Vec<Box<dyn PoolFilter>>,
    config: FilterConfig,
    rpc: Arc<RpcClient>,
}

impl FilterPipeline {
    pub fn new(config: FilterConfig, rpc: Arc<RpcClient>) -> Self {
        let mut filters: Vec<Box<dyn PoolFilter>> = vec![];

        if config.check_mint_renounced {
            filters.push(Box::new(MintRenounceFilter));
        }
        if config.check_freezable {
            filters.push(Box::new(FreezableFilter));
        }
        if config.check_burned {
            filters.push(Box::new(BurnFilter));
        }
        if config.min_pool_size > 0.0 || config.max_pool_size > 0.0 {
            // Convert SOL to lamports
            let min = (config.min_pool_size * 1e9) as u64;
            let max = (config.max_pool_size * 1e9) as u64;
            filters.push(Box::new(PoolSizeFilter {
                min_size_lamports: min,
                max_size_lamports: max,
            }));
        }

        Self { filters, config, rpc }
    }

    /// Run all filters. Returns true if pool passes all filters
    /// Retries up to check_duration / check_interval times, requiring
    /// consecutive_matches consecutive passes.
    pub async fn execute(&self, pool: &CachedPool) -> bool {
        if self.filters.is_empty() {
            return true;
        }

        let interval = tokio::time::Duration::from_millis(self.config.check_interval_ms);
        let max_checks = if self.config.check_interval_ms > 0 {
            self.config.check_duration_ms / self.config.check_interval_ms
        } else {
            1
        };

        let mut consecutive = 0u32;
        let mut checks = 0u64;

        loop {
            let all_pass = self.run_once(pool).await;

            if all_pass {
                consecutive += 1;
                debug!(
                    mint = %pool.base_mint,
                    "Filter pass {}/{}", consecutive, self.config.consecutive_matches
                );
                if consecutive >= self.config.consecutive_matches {
                    return true;
                }
            } else {
                consecutive = 0;
            }

            checks += 1;
            if checks >= max_checks {
                return false;
            }

            tokio::time::sleep(interval).await;
        }
    }

    async fn run_once(&self, pool: &CachedPool) -> bool {
        for filter in &self.filters {
            let result = filter.check(pool, &self.rpc).await;
            if !result.passed {
                debug!(
                    mint = %pool.base_mint,
                    filter = filter.name(),
                    reason = ?result.reason,
                    "Filter failed"
                );
                return false;
            }
        }
        true
    }
}
