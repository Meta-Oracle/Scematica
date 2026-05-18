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

    /// Find any cached pool whose base_mint matches. Used by auto_dump to avoid
    /// a getProgramAccounts RPC call for tokens bought during this run.
    pub fn find_by_base_mint(&self, base_mint: &solana_sdk::pubkey::Pubkey) -> Option<CachedPool> {
        self.inner
            .iter()
            .find(|e| e.value().base_mint == *base_mint)
            .map(|e| e.value().clone())
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Persist all cached pools to disk so sell lookups survive restarts.
    /// Caps at MAX_POOL_CACHE_ENTRIES to prevent unbounded growth during
    /// long multi-day sessions (2k+ entries → multi-MB JSON, slow atomic writes).
    pub fn persist_to_file(&self, path: &str) {
        const MAX_POOL_CACHE_ENTRIES: usize = 1000;
        let mut entries: Vec<(String, CachedPool)> = self.inner
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();
        let original_len = entries.len();
        if original_len > MAX_POOL_CACHE_ENTRIES {
            // Keep the last MAX_POOL_CACHE_ENTRIES — most recent inserts are
            // the actively-monitored positions the sell path needs.
            entries.truncate(MAX_POOL_CACHE_ENTRIES);
            tracing::debug!("Pool cache: trimmed {} → {} entries for persist", original_len, MAX_POOL_CACHE_ENTRIES);
        }
        let map: std::collections::HashMap<String, CachedPool> =
            entries.into_iter().collect();
        if let Ok(json) = serde_json::to_string(&map) {
            let tmp = format!("{}.tmp", path);
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    /// Load cached pools from disk. Merges with any existing in-memory entries.
    pub fn load_from_file(&self, path: &str) {
        let Ok(data) = std::fs::read_to_string(path) else { return };
        let Ok(map) = serde_json::from_str::<std::collections::HashMap<String, CachedPool>>(&data) else { return };
        let count = map.len();
        for (k, v) in map {
            self.inner.insert(k, v);
        }
        tracing::info!("Pool cache: loaded {} entries from {}", count, path);
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
