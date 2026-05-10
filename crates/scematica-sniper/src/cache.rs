use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

/// Cached pool state (Raydium V4 layout decoded)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPool {
    pub id: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub market_id: Pubkey,
    pub open_time: u64,
    pub base_decimals: u8,
    pub quote_decimals: u8,
}

/// Cached market state (OpenBook V3)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedMarket {
    pub id: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub request_queue: Pubkey,
    pub event_queue: Pubkey,
    pub bids: Pubkey,
    pub asks: Pubkey,
}

/// Thread-safe pool cache
#[derive(Debug, Clone, Default)]
pub struct PoolCache {
    inner: Arc<DashMap<String, CachedPool>>,
}

impl PoolCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn save(&self, pool_id: &str, pool: CachedPool) {
        self.inner.insert(pool_id.to_string(), pool);
    }

    pub fn get(&self, pool_id: &str) -> Option<CachedPool> {
        self.inner.get(pool_id).map(|v| v.clone())
    }

    pub fn contains(&self, pool_id: &str) -> bool {
        self.inner.contains_key(pool_id)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

/// Thread-safe market cache
#[derive(Debug, Clone, Default)]
pub struct MarketCache {
    inner: Arc<DashMap<String, CachedMarket>>,
}

impl MarketCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn save(&self, market_id: &str, market: CachedMarket) {
        self.inner.insert(market_id.to_string(), market);
    }

    pub fn get(&self, market_id: &str) -> Option<CachedMarket> {
        self.inner.get(market_id).map(|v| v.clone())
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

/// Snipe list: set of token mints to watch for
#[derive(Debug, Clone, Default)]
pub struct SnipeListCache {
    inner: Arc<DashMap<String, bool>>,
    path: String,
}

impl SnipeListCache {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
            path: path.into(),
        }
    }

    /// Load snipe list from file
    pub fn load(&self) -> anyhow::Result<()> {
        let content = std::fs::read_to_string(&self.path)?;
        self.inner.clear();
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                self.inner.insert(trimmed.to_string(), true);
            }
        }
        tracing::info!("Loaded {} tokens from snipe list", self.inner.len());
        Ok(())
    }

    pub fn is_listed(&self, mint: &str) -> bool {
        self.inner.contains_key(mint)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
}
