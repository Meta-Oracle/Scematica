use async_trait::async_trait;
use scematica_core::config::FilterConfig;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use dashmap::DashMap;
use tracing::{debug, info, warn};

use crate::cache::CachedPool;
use crate::reputation::DeployerLedger;

/// Tracks per-filter rejection counts for dashboard stats
#[derive(Debug, Default, Clone)]
pub struct FilterStats {
    inner: Arc<DashMap<String, u32>>,
    pub pools_seen: Arc<AtomicU32>,
    pub pools_passed: Arc<AtomicU32>,
}

impl FilterStats {
    pub fn record_rejection(&self, filter_name: &str) {
        *self.inner.entry(filter_name.to_string()).or_insert(0) += 1;
    }
    pub fn snapshot(&self) -> Vec<(String, u32)> {
        let mut v: Vec<_> = self.inner.iter().map(|e| (e.key().clone(), *e.value())).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }
    pub fn write_to_file(&self, path: &str) {
        let snap = serde_json::json!({
            "pools_seen": self.pools_seen.load(Ordering::Relaxed),
            "pools_passed": self.pools_passed.load(Ordering::Relaxed),
            "rejections": self.snapshot().into_iter().collect::<std::collections::HashMap<_,_>>(),
        });
        let tmp = format!("{}.tmp", path);
        if let Ok(s) = serde_json::to_string(&snap) {
            if std::fs::write(&tmp, &s).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

pub const FILTER_STATS_FILE: &str = "scematica-filter-stats.json";

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

/// Hard cap per RPC call — any node that takes longer is treated as failed.
const RPC_CALL_TIMEOUT_SECS: u64 = 5;

fn is_rate_limited(e: &str) -> bool {
    e.contains("429") || e.contains("Too Many Requests")
}

/// Fetch an account with up to `retries` attempts, backing off on 429.
/// Each attempt is capped at RPC_CALL_TIMEOUT_SECS so a hung node can't stall the pipeline.
async fn get_account_retried(
    rpc: &Arc<RpcClient>,
    pubkey: &Pubkey,
    retries: u32,
) -> Option<Account> {
    for attempt in 0..retries {
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(RPC_CALL_TIMEOUT_SECS),
            rpc.get_account(pubkey),
        ).await;
        match result {
            Ok(Ok(a)) => return Some(a),
            Ok(Err(e)) => {
                let msg = e.to_string();
                if is_rate_limited(&msg) && attempt + 1 < retries {
                    let delay = 600 * (attempt + 1) as u64;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                } else {
                    return None;
                }
            }
            Err(_) => {
                debug!("RPC get_account timed out on attempt {}/{}", attempt + 1, retries);
                return None;
            }
        }
    }
    None
}

/// Fetch a token account balance with up to `retries` attempts on 429.
/// Each attempt is capped at RPC_CALL_TIMEOUT_SECS.
async fn get_token_balance_retried(
    rpc: &Arc<RpcClient>,
    pubkey: &Pubkey,
    retries: u32,
) -> Option<u64> {
    for attempt in 0..retries {
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(RPC_CALL_TIMEOUT_SECS),
            rpc.get_token_account_balance(pubkey),
        ).await;
        match result {
            Ok(Ok(b)) => return b.amount.parse().ok(),
            Ok(Err(e)) => {
                let msg = e.to_string();
                if is_rate_limited(&msg) && attempt + 1 < retries {
                    let delay = 600 * (attempt + 1) as u64;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                } else {
                    return None;
                }
            }
            Err(_) => {
                debug!("RPC get_token_balance timed out on attempt {}/{}", attempt + 1, retries);
                return None;
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

/// Known scam/rug keywords — case-insensitive substring match
static SCAM_WORDS: &[&str] = &[
    "test", "rug", "scam", "free", "airdrop", "safe", "moon100x", "1000x",
    "elon", "trump", "biden", "shib2", "pepe2", "honeypot", "drain",
    "presale", "stealth", "fair launch", "dev wallet",
];

pub struct NameFilter;

#[async_trait]
impl PoolFilter for NameFilter {
    fn name(&self) -> &str { "NameFilter" }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        // Derive Metaplex metadata PDA: ["metadata", METADATA_PROGRAM, mint]
        const METADATA_PROGRAM: Pubkey = solana_sdk::pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");
        let seeds: &[&[u8]] = &[b"metadata", METADATA_PROGRAM.as_ref(), pool.base_mint.as_ref()];
        let (metadata_pda, _) = Pubkey::find_program_address(seeds, &METADATA_PROGRAM);

        match rpc.get_account(&metadata_pda).await {
            Ok(acct) if acct.data.len() > 100 => {
                // Metaplex metadata: name starts at byte 69 (after discriminator+update_auth+mint)
                // Layout: [1 discriminator][32 update_auth][32 mint][4+len name][4+len symbol]...
                let data = &acct.data;
                let name = read_metaplex_string(data, 65);
                let symbol = read_metaplex_string(data, 65 + 4 + name.len().min(32));
                let combined = format!("{} {}", name, symbol).to_lowercase();
                for word in SCAM_WORDS {
                    if combined.contains(word) {
                        return FilterResult::fail(format!("Suspicious name: '{}' contains '{}'", combined.trim(), word));
                    }
                }
                debug!(mint = %pool.base_mint, name = %name.trim(), symbol = %symbol.trim(), "Name filter passed");
                FilterResult::pass()
            }
            Ok(_) | Err(_) => {
                // No metadata = anonymous token — treat as suspicious but don't hard-fail
                debug!(mint = %pool.base_mint, "NameFilter: no metadata found — skipping name check");
                FilterResult::pass()
            }
        }
    }
}

fn read_metaplex_string(data: &[u8], offset: usize) -> String {
    if offset + 4 > data.len() { return String::new(); }
    let len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap_or([0;4])) as usize;
    let start = offset + 4;
    let end = (start + len).min(data.len());
    String::from_utf8_lossy(&data[start..end]).trim_end_matches('\0').to_string()
}

/// Check recent transaction volume on the pool
pub struct VolumeFilter {
    pub min_txns: u32,
}

#[async_trait]
impl PoolFilter for VolumeFilter {
    fn name(&self) -> &str { "VolumeSpike" }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
        let config = GetConfirmedSignaturesForAddress2Config {
            limit: Some(self.min_txns as usize + 1),
            ..Default::default()
        };
        match rpc.get_signatures_for_address_with_config(&pool.id, config).await {
            Ok(sigs) => {
                let count = sigs.len() as u32;
                if count >= self.min_txns {
                    FilterResult::pass()
                } else {
                    FilterResult::fail(format!("Insufficient volume: {} txns (need {})", count, self.min_txns))
                }
            }
            Err(e) => {
                warn!(mint = %pool.base_mint, "VolumeFilter RPC error: {} — skipping", e);
                FilterResult::pass() // fail-open
            }
        }
    }
}

/// Check if our buy size causes excessive price impact
pub struct LiquidityDepthFilter {
    pub quote_amount_raw: u64,
    pub max_price_impact_pct: f64,
}

#[async_trait]
impl PoolFilter for LiquidityDepthFilter {
    fn name(&self) -> &str { "LiquidityDepth" }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        let qb = match get_token_balance_retried(rpc, &pool.quote_vault, 3).await {
            Some(v) if v > 0 => v,
            _ => return FilterResult::pass(), // can't check — fail-open
        };
        // Constant product AMM: impact = amount / (reserve + amount)
        let impact_pct = (self.quote_amount_raw as f64 / (qb as f64 + self.quote_amount_raw as f64)) * 100.0;
        if impact_pct > self.max_price_impact_pct {
            FilterResult::fail(format!(
                "Price impact {:.1}% exceeds max {:.1}% (pool liquidity: {:.3} SOL)",
                impact_pct,
                self.max_price_impact_pct,
                qb as f64 / 1e9,
            ))
        } else {
            debug!(mint = %pool.base_mint, impact_pct = %format!("{:.2}%", impact_pct), "Liquidity depth OK");
            FilterResult::pass()
        }
    }
}

/// Check pool deployer/authority against the blacklist file
pub struct BlacklistFilter {
    /// Loaded set of blacklisted pubkeys
    blacklist: Arc<std::collections::HashSet<String>>,
}

impl BlacklistFilter {
    pub fn load(path: &str) -> Self {
        let set = std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        Self { blacklist: Arc::new(set) }
    }
}

#[async_trait]
impl PoolFilter for BlacklistFilter {
    fn name(&self) -> &str { "Blacklist" }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        // Check pool account owner as proxy for deployer
        match rpc.get_account(&pool.id).await {
            Ok(acct) => {
                let owner = acct.owner.to_string();
                if self.blacklist.contains(&owner) {
                    return FilterResult::fail(format!("Blacklisted account: {}", owner));
                }
                FilterResult::pass()
            }
            Err(_) => FilterResult::pass(), // fail-open
        }
    }
}

/// Reject tokens where top-10 holders own a suspicious share of supply (whale concentration rug signal).
pub struct HolderConcentrationFilter {
    pub max_top10_pct: f64,
}

#[async_trait]
impl PoolFilter for HolderConcentrationFilter {
    fn name(&self) -> &str { "HolderConcentration" }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        // Fetch the token's total supply (5s timeout)
        let supply = match tokio::time::timeout(
            tokio::time::Duration::from_secs(RPC_CALL_TIMEOUT_SECS),
            rpc.get_token_supply(&pool.base_mint),
        ).await {
            Ok(Ok(s)) => s.amount.parse::<u128>().unwrap_or(0),
            Ok(Err(e)) => {
                warn!(mint = %pool.base_mint, "HolderConcentration: supply fetch failed: {} — skipping", e);
                return FilterResult::pass();
            }
            Err(_) => {
                warn!(mint = %pool.base_mint, "HolderConcentration: supply fetch timed out — skipping");
                return FilterResult::pass();
            }
        };
        if supply == 0 {
            return FilterResult::fail("Token supply is zero");
        }

        // getTokenLargestAccounts returns up to 20 largest holders sorted by balance (5s timeout)
        let top = match tokio::time::timeout(
            tokio::time::Duration::from_secs(RPC_CALL_TIMEOUT_SECS),
            rpc.get_token_largest_accounts(&pool.base_mint),
        ).await {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => {
                warn!(mint = %pool.base_mint, "HolderConcentration: getLargestAccounts failed: {} — skipping", e);
                return FilterResult::pass();
            }
            Err(_) => {
                warn!(mint = %pool.base_mint, "HolderConcentration: getLargestAccounts timed out — skipping");
                return FilterResult::pass();
            }
        };

        let top10_amount: u128 = top.iter()
            .take(10)
            .filter_map(|a| a.amount.amount.parse::<u128>().ok())
            .sum();

        let concentration_pct = top10_amount as f64 / supply as f64 * 100.0;

        if concentration_pct > self.max_top10_pct {
            FilterResult::fail(format!(
                "Top-10 holders own {:.1}% of supply (max allowed: {:.1}%)",
                concentration_pct, self.max_top10_pct
            ))
        } else {
            debug!(
                mint = %pool.base_mint,
                concentration_pct = %format!("{:.1}%", concentration_pct),
                "Holder concentration OK"
            );
            FilterResult::pass()
        }
    }
}

/// Rejects pools where the deployer's historical rug score is too low.
pub struct DeployerReputationFilter {
    pub ledger: Arc<parking_lot::Mutex<DeployerLedger>>,
    pub min_score: f64,
}

impl DeployerReputationFilter {
    pub fn new(ledger: Arc<parking_lot::Mutex<DeployerLedger>>, min_score: f64) -> Self {
        Self { ledger, min_score }
    }
}

#[async_trait]
impl PoolFilter for DeployerReputationFilter {
    fn name(&self) -> &str { "DeployerReputation" }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        let deployer = match tokio::time::timeout(
            tokio::time::Duration::from_secs(RPC_CALL_TIMEOUT_SECS),
            rpc.get_account(&pool.id),
        ).await {
            Ok(Ok(acct)) => acct.owner.to_string(),
            Ok(Err(e)) => {
                warn!(mint = %pool.base_mint, "DeployerReputation: account fetch failed: {} — skipping", e);
                return FilterResult::pass();
            }
            Err(_) => {
                warn!(mint = %pool.base_mint, "DeployerReputation: account fetch timed out — skipping");
                return FilterResult::pass();
            }
        };
        let score = self.ledger.lock().score(&deployer);
        if score < self.min_score {
            FilterResult::fail(format!(
                "Deployer {} reputation score {:.2} < min {:.2}",
                &deployer[..8.min(deployer.len())], score, self.min_score
            ))
        } else {
            debug!(mint = %pool.base_mint, deployer = %&deployer[..8.min(deployer.len())], score, "Deployer reputation OK");
            FilterResult::pass()
        }
    }
}

// ── new filters ───────────────────────────────────────────────────────────────

/// Liquidity momentum filter.
///
/// Fetches the pool quote vault balance twice 3 seconds apart.  If the second
/// reading hasn't grown by at least `min_growth_pct`% the pool is likely being
/// drained by the deployer and is rejected.
pub struct LiquidityMomentumFilter {
    pub min_growth_pct: f64,
}

#[async_trait]
impl PoolFilter for LiquidityMomentumFilter {
    fn name(&self) -> &str { "LiquidityMomentum" }

    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        let first = match get_token_balance_retried(rpc, &pool.quote_vault, 2).await {
            Some(v) if v > 0 => v,
            _ => {
                warn!(mint = %pool.base_mint, "LiquidityMomentum: first read unavailable — skipping");
                return FilterResult::pass();
            }
        };

        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

        let second = match get_token_balance_retried(rpc, &pool.quote_vault, 2).await {
            Some(v) => v,
            None => {
                warn!(mint = %pool.base_mint, "LiquidityMomentum: second read unavailable — skipping");
                return FilterResult::pass();
            }
        };

        // Check for zero to avoid divide-by-zero
        if first == 0 {
            return FilterResult::pass();
        }

        let growth_pct = (second as f64 - first as f64) / first as f64 * 100.0;

        if growth_pct < self.min_growth_pct {
            FilterResult::fail(format!(
                "Liquidity momentum too low: {:.1}% growth (need {:.1}%); pool may be draining",
                growth_pct, self.min_growth_pct
            ))
        } else {
            debug!(
                mint = %pool.base_mint,
                growth_pct = %format!("{:.1}%", growth_pct),
                "Liquidity momentum OK"
            );
            FilterResult::pass()
        }
    }
}

/// Cross-pool correlation rug guard.
///
/// Checks the deployer reputation ledger for rugs recorded within the last 24 h.
/// Deployers who have rugged >N times recently are rejected.
pub struct CrossPoolCorrelationFilter {
    pub ledger: Arc<parking_lot::Mutex<crate::reputation::DeployerLedger>>,
    pub max_rugs_24h: u32,
}

#[async_trait]
impl PoolFilter for CrossPoolCorrelationFilter {
    fn name(&self) -> &str { "CrossPoolCorrelation" }

    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        let deployer = match tokio::time::timeout(
            tokio::time::Duration::from_secs(RPC_CALL_TIMEOUT_SECS),
            rpc.get_account(&pool.id),
        ).await {
            Ok(Ok(acct)) => acct.owner.to_string(),
            Ok(Err(e)) => {
                warn!(mint = %pool.base_mint, "CrossPoolCorrelation: account fetch failed: {} — skipping", e);
                return FilterResult::pass();
            }
            Err(_) => {
                warn!(mint = %pool.base_mint, "CrossPoolCorrelation: account fetch timed out — skipping");
                return FilterResult::pass();
            }
        };

        let rug_count = self.ledger.lock().recent_rug_count(&deployer);

        if rug_count > self.max_rugs_24h {
            FilterResult::fail(format!(
                "Deployer {} has {} recent rugs (max allowed: {})",
                &deployer[..8.min(deployer.len())], rug_count, self.max_rugs_24h
            ))
        } else {
            debug!(
                mint = %pool.base_mint,
                deployer = %&deployer[..8.min(deployer.len())],
                rug_count,
                "Cross-pool correlation OK"
            );
            FilterResult::pass()
        }
    }
}

/// Jupiter price discrepancy filter.
///
/// Queries the Jupiter Price API for the token's price and compares it to the
/// AMM pool price.  If Jupiter's price is higher than the AMM price by at least
/// `min_premium_pct`%, the pool is a buy signal (cheap relative to the broader
/// market).  Pools where the AMM is already at or above Jupiter are rejected.
pub struct JupiterDiscrepancyFilter {
    pub min_premium_pct: f64,
}

#[async_trait]
impl PoolFilter for JupiterDiscrepancyFilter {
    fn name(&self) -> &str { "JupiterDiscrepancy" }

    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        // Estimate AMM price in lamports-per-base-token using reserve ratio.
        let (quote_reserve, base_reserve) = match tokio::join!(
            get_token_balance_retried(rpc, &pool.quote_vault, 2),
            get_token_balance_retried(rpc, &pool.base_vault, 2),
        ) {
            (Some(q), Some(b)) if q > 0 && b > 0 => (q, b),
            _ => {
                warn!(mint = %pool.base_mint, "JupiterDiscrepancy: reserve fetch failed — skipping");
                return FilterResult::pass();
            }
        };

        let amm_price_lamports = quote_reserve as f64 / base_reserve as f64;

        // Fetch Jupiter price
        let jup_price_lamports = match crate::jup_oracle::JupiterOracle::get_price_lamports(&pool.base_mint).await {
            Some(p) => p,
            None => {
                warn!(mint = %pool.base_mint, "JupiterDiscrepancy: Jupiter price unavailable — skipping");
                return FilterResult::pass();
            }
        };

        let premium = crate::jup_oracle::JupiterOracle::premium_pct(amm_price_lamports, jup_price_lamports);

        if premium < self.min_premium_pct {
            FilterResult::fail(format!(
                "Jupiter premium {:.1}% < min {:.1}% (AMM={:.6} JUP={:.6} lamports/token)",
                premium, self.min_premium_pct, amm_price_lamports, jup_price_lamports
            ))
        } else {
            debug!(
                mint = %pool.base_mint,
                premium_pct = %format!("{:.1}%", premium),
                "Jupiter discrepancy buy signal detected"
            );
            FilterResult::pass()
        }
    }
}

// ── pipeline ──────────────────────────────────────────────────────────────────

pub struct FilterPipeline {
    filters: Vec<Box<dyn PoolFilter>>,
    config: FilterConfig,
    rpc: Arc<RpcClient>,
    pub stats: FilterStats,
}

impl FilterPipeline {
    pub fn new(
        config: FilterConfig,
        rpc: Arc<RpcClient>,
        quote_amount_raw: u64,
        blacklist_path: &str,
        deployer_ledger: Option<Arc<parking_lot::Mutex<DeployerLedger>>>,
    ) -> Self {
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
        if config.check_name {
            filters.push(Box::new(NameFilter));
        }
        if config.check_volume && config.min_volume_txns > 0 {
            filters.push(Box::new(VolumeFilter { min_txns: config.min_volume_txns }));
        }
        if config.check_liquidity_depth && config.max_price_impact_pct > 0.0 {
            filters.push(Box::new(LiquidityDepthFilter {
                quote_amount_raw,
                max_price_impact_pct: config.max_price_impact_pct,
            }));
        }
        if std::path::Path::new(blacklist_path).exists() {
            let bf = BlacklistFilter::load(blacklist_path);
            if !bf.blacklist.is_empty() {
                filters.push(Box::new(bf));
            }
        }
        if config.check_holder_concentration && config.max_top10_holder_pct > 0.0 {
            filters.push(Box::new(HolderConcentrationFilter {
                max_top10_pct: config.max_top10_holder_pct,
            }));
        }
        if let Some(ledger) = deployer_ledger.clone() {
            filters.push(Box::new(DeployerReputationFilter::new(ledger, 0.3)));
        }

        // ── New filters wired from config ─────────────────────────────────────
        if config.check_liquidity_momentum {
            filters.push(Box::new(LiquidityMomentumFilter {
                min_growth_pct: config.liquidity_momentum_pct,
            }));
        }
        if config.check_cross_pool_correlation {
            if let Some(ledger) = deployer_ledger {
                filters.push(Box::new(CrossPoolCorrelationFilter {
                    ledger,
                    max_rugs_24h: config.max_deployer_rugs_24h,
                }));
            }
        }
        if config.check_jupiter_discrepancy {
            filters.push(Box::new(JupiterDiscrepancyFilter {
                min_premium_pct: config.jupiter_min_premium_pct,
            }));
        }

        Self { filters, config, rpc, stats: FilterStats::default() }
    }

    pub async fn execute(&self, pool: &CachedPool) -> bool {
        self.stats.pools_seen.fetch_add(1, Ordering::Relaxed);
        if self.filters.is_empty() {
            self.stats.pools_passed.fetch_add(1, Ordering::Relaxed);
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
            if self.run_once(pool).await {
                consecutive += 1;
                debug!(mint = %pool.base_mint, "Filter pass {}/{}", consecutive, self.config.consecutive_matches);
                if consecutive >= self.config.consecutive_matches {
                    self.stats.pools_passed.fetch_add(1, Ordering::Relaxed);
                    self.stats.write_to_file(FILTER_STATS_FILE);
                    return true;
                }
            } else {
                consecutive = 0;
            }

            checks += 1;
            if checks >= max_checks {
                self.stats.write_to_file(FILTER_STATS_FILE);
                return false;
            }

            tokio::time::sleep(interval).await;
        }
    }

    async fn run_once(&self, pool: &CachedPool) -> bool {
        for filter in &self.filters {
            let result = filter.check(pool, &self.rpc).await;
            if !result.passed {
                let reason = result.reason.as_deref().unwrap_or("unknown");
                info!(mint = %pool.base_mint, filter = filter.name(), reason = %reason, "Filter rejected pool");
                self.stats.record_rejection(filter.name());
                return false;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(120)).await;
        }
        true
    }
}
