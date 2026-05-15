use async_trait::async_trait;
use scematica_core::config::FilterConfig;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tracing::{debug, warn};

use crate::cache::CachedPool;

#[derive(Debug, Clone)]
pub struct FilterResult {
    pub passed: bool,
    pub reason: Option<String>,
}

impl FilterResult {
    pub fn pass() -> Self { Self { passed: true, reason: None } }
    pub fn fail(reason: impl Into<String>) -> Self { Self { passed: false, reason: Some(reason.into()) } }
}

#[async_trait]
pub trait PoolFilter: Send + Sync {
    fn name(&self) -> &str;
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult;
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn is_rate_limited(e: &str) -> bool {
    e.contains("429") || e.contains("Too Many Requests")
}

/// Fetch an account with up to `retries` attempts, backing off on 429.
async fn get_account_retried(
    rpc: &Arc<RpcClient>,
    pubkey: &Pubkey,
    retries: u32,
) -> Option<Account> {
    for attempt in 0..retries {
        match rpc.get_account(pubkey).await {
            Ok(a) => return Some(a),
            Err(e) => {
                let msg = e.to_string();
                if is_rate_limited(&msg) && attempt + 1 < retries {
                    let delay = 600 * (attempt + 1) as u64;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                } else {
                    return None;
                }
            }
        }
    }
    None
}

/// Fetch a token account balance with up to `retries` attempts on 429.
async fn get_token_balance_retried(
    rpc: &Arc<RpcClient>,
    pubkey: &Pubkey,
    retries: u32,
) -> Option<u64> {
    for attempt in 0..retries {
        match rpc.get_token_account_balance(pubkey).await {
            Ok(b) => return b.amount.parse().ok(),
            Err(e) => {
                let msg = e.to_string();
                if is_rate_limited(&msg) && attempt + 1 < retries {
                    let delay = 600 * (attempt + 1) as u64;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                } else {
                    return None;
                }
            }
        }
    }
    None
}

// ── individual filters ────────────────────────────────────────────────────────

pub struct MintRenounceFilter;

#[async_trait]
impl PoolFilter for MintRenounceFilter {
    fn name(&self) -> &str { "MintRenounced" }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        match get_account_retried(rpc, &pool.base_mint, 3).await {
            Some(account) if account.data.len() >= 4 => {
                if account.data[0] == 0 {
                    FilterResult::pass()
                } else {
                    FilterResult::fail("Mint authority not renounced")
                }
            }
            Some(_) => FilterResult::fail("Invalid mint account data"),
            None => {
                warn!(mint = %pool.base_mint, "MintRenounceFilter: RPC unavailable after retries — skipping filter");
                FilterResult::pass() // fail-open: don't drop pool due to RPC issues
            }
        }
    }
}

pub struct FreezableFilter;

#[async_trait]
impl PoolFilter for FreezableFilter {
    fn name(&self) -> &str { "NotFreezable" }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        match get_account_retried(rpc, &pool.base_mint, 3).await {
            Some(account) if account.data.len() >= 82 => {
                // Mint layout: [0..4]=mint_auth option, [4..36]=mint_auth key,
                // [36..44]=supply, [44]=decimals, [45]=initialized, [46..50]=freeze_auth option
                if account.data[46] == 0 {
                    FilterResult::pass()
                } else {
                    FilterResult::fail("Token has freeze authority")
                }
            }
            Some(_) => FilterResult::fail("Invalid mint account data"),
            None => {
                warn!(mint = %pool.base_mint, "FreezableFilter: RPC unavailable — skipping filter");
                FilterResult::pass()
            }
        }
    }
}

pub struct BurnFilter;

#[async_trait]
impl PoolFilter for BurnFilter {
    fn name(&self) -> &str { "LPBurned" }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        match get_token_balance_retried(rpc, &pool.base_vault, 3).await {
            Some(amount) if amount > 0 => FilterResult::pass(),
            Some(_) => FilterResult::fail("Pool vault is empty (possibly rugged)"),
            None => {
                warn!(mint = %pool.base_mint, "BurnFilter: RPC unavailable — skipping filter");
                FilterResult::pass()
            }
        }
    }
}

pub struct PoolSizeFilter {
    pub min_size_lamports: u64,
    pub max_size_lamports: u64,
}

#[async_trait]
impl PoolFilter for PoolSizeFilter {
    fn name(&self) -> &str { "PoolSize" }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        match get_token_balance_retried(rpc, &pool.quote_vault, 3).await {
            Some(amount) => {
                if self.min_size_lamports > 0 && amount < self.min_size_lamports {
                    return FilterResult::fail(format!("Pool too small: {}", amount));
                }
                if self.max_size_lamports > 0 && amount > self.max_size_lamports {
                    return FilterResult::fail(format!("Pool too large: {}", amount));
                }
                FilterResult::pass()
            }
            None => {
                warn!(mint = %pool.base_mint, "PoolSizeFilter: RPC unavailable — skipping filter");
                FilterResult::pass()
            }
        }
    }
}

// ── pipeline ──────────────────────────────────────────────────────────────────

pub struct FilterPipeline {
    filters: Vec<Box<dyn PoolFilter>>,
    config: FilterConfig,
    rpc: Arc<RpcClient>,
}

impl FilterPipeline {
    pub fn new(config: FilterConfig, rpc: Arc<RpcClient>) -> Self {
        let mut filters: Vec<Box<dyn PoolFilter>> = vec![];
        if config.check_mint_renounced { filters.push(Box::new(MintRenounceFilter)); }
        if config.check_freezable      { filters.push(Box::new(FreezableFilter)); }
        if config.check_burned         { filters.push(Box::new(BurnFilter)); }
        if config.min_pool_size > 0.0 || config.max_pool_size > 0.0 {
            filters.push(Box::new(PoolSizeFilter {
                min_size_lamports: (config.min_pool_size * 1e9) as u64,
                max_size_lamports: (config.max_pool_size * 1e9) as u64,
            }));
        }
        Self { filters, config, rpc }
    }

    pub async fn execute(&self, pool: &CachedPool) -> bool {
        if self.filters.is_empty() { return true; }

        let interval = tokio::time::Duration::from_millis(self.config.check_interval_ms);
        let max_checks = if self.config.check_interval_ms > 0 {
            self.config.check_duration_ms / self.config.check_interval_ms
        } else {
            1
        };

        let mut consecutive = 0u32;
        let mut checks = 0u64;

        loop {
            if self.run_once(pool).await {
                consecutive += 1;
                debug!(mint = %pool.base_mint, "Filter pass {}/{}", consecutive, self.config.consecutive_matches);
                if consecutive >= self.config.consecutive_matches {
                    return true;
                }
            } else {
                consecutive = 0;
            }

            checks += 1;
            if checks >= max_checks { return false; }

            tokio::time::sleep(interval).await;
        }
    }

    async fn run_once(&self, pool: &CachedPool) -> bool {
        for filter in &self.filters {
            let result = filter.check(pool, &self.rpc).await;
            if !result.passed {
                debug!(mint = %pool.base_mint, filter = filter.name(), reason = ?result.reason, "Filter failed");
                return false;
            }
            // Small gap between sequential filter calls to avoid burst
            tokio::time::sleep(tokio::time::Duration::from_millis(120)).await;
        }
        true
    }
}
