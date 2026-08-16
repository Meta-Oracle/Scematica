use async_trait::async_trait;
use dashmap::DashMap;
use scematica_core::config::FilterConfig;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
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
        let mut v: Vec<_> = self
            .inner
            .iter()
            .map(|e| (e.key().clone(), *e.value()))
            .collect();
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

/// On-chain and off-chain metadata enrichment for a token.
/// Written by SocialLinksFilter; read by sniper AI call + pool scorer.
#[derive(Debug, Clone, Default)]
pub struct TokenSocials {
    pub name: String,
    pub symbol: String,
    pub twitter: bool,
    pub telegram: bool,
    pub website: bool,
    pub discord: bool,
    /// 0–4: how many distinct social channels were found
    pub social_count: u8,
}

#[derive(Debug, Clone)]
pub struct FilterResult {
    pub passed: bool,
    pub reason: Option<String>,
}

impl FilterResult {
    pub fn pass() -> Self {
        Self {
            passed: true,
            reason: None,
        }
    }
    pub fn fail(reason: impl Into<String>) -> Self {
        Self {
            passed: false,
            reason: Some(reason.into()),
        }
    }
}

#[async_trait]
pub trait PoolFilter: Send + Sync {
    fn name(&self) -> &str;
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult;
}

/// Which filter stopped a pool, and what it said.
///
/// The pipeline used to collapse to a bare `bool` here, so the decision log recorded
/// 736 of 1959 rejections as an undifferentiated `filter_rejected` — enough to know the
/// pipeline was rejecting and not enough to know what to change. `FilterStats` had the
/// per-filter counts all along, but only as dashboard totals: they answer "how often does
/// LpBurn fire?" and never "which filter stopped *this* pool, and was its input real?".
///
/// That second question is the one that matters, because a filter rejecting on a signal
/// that is always zero looks identical in the totals to one doing its job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterRejection {
    /// The rejecting filter's `PoolFilter::name`.
    pub filter: String,
    /// That filter's own explanation, verbatim.
    pub reason: String,
}

impl FilterRejection {
    /// Render for the decision log's free-text `reason` column.
    ///
    /// `key=value;key=value` matches the convention already used by the momentum and
    /// `dq_advice` sites. The `stage` column stays `filters` — `replay.rs` keys its
    /// governed-stage matching on `stage` precisely because these reason strings are
    /// heterogeneous free text, so enriching this string cannot mislead the replay.
    pub fn as_decision_reason(&self) -> String {
        format!("filter={};reason={}", self.filter, self.reason)
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Hard cap per RPC call — any node that takes longer is treated as failed.
/// 3 s is well above the p99 for a healthy paid RPC (Helius/Triton); anything
/// slower than that is almost certainly a stalled/degraded node and we'd rather
/// fail open than hold the whole pipeline.
const RPC_CALL_TIMEOUT_SECS: u64 = 3;

/// Maximum RPC calls the filter pipeline keeps in flight at once.
///
/// This is a **safety** control, not a politeness one. Every RPC-bound filter fails open,
/// so a throttled provider does not slow the pipeline down — it silently converts it into
/// a pass-through that still reports "passed". A skipped Freezable check is how you buy a
/// token you cannot sell.
///
/// MEASURED against this deployment's keyed Helius endpoint, 2026-08-16, identical
/// `getAccountInfo` calls issued concurrently:
///
/// ```text
///   10 in flight →  10 ok,  0 rate-limited
///   25 in flight →  22 ok,  3 rate-limited
///   50 in flight →   0 ok, 50 rate-limited      ← total wipeout
/// ```
///
/// Several pools evaluate at once and each runs several RPC-bound filters, so the
/// unbounded pipeline routinely crossed 50 and lost *every* check. That is the
/// `resolution_rate=49%` in the 2026-08-16 logs, and it is why the coherence breaker was
/// halting buys: the breaker was right, the pipeline really was flying blind.
///
/// 8 sits below the measured clean ceiling with headroom for the executor's own calls,
/// which take a different path and must never be starved by filter traffic — landing a
/// sell matters more than verifying a pool we have not bought yet.
const MAX_INFLIGHT_RPC: usize = 8;

/// How long a call will queue for a permit before giving up.
///
/// Queuing is strictly better than failing open — a check that answers late still
/// answers, and a check that fails open answers nothing while looking like a pass. But it
/// cannot be unbounded, or a stalled provider backs the whole pipeline up behind it.
const PERMIT_WAIT_SECS: u64 = 3;

static RPC_INFLIGHT: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();

fn rpc_gate() -> &'static tokio::sync::Semaphore {
    RPC_INFLIGHT.get_or_init(|| tokio::sync::Semaphore::new(MAX_INFLIGHT_RPC))
}

/// Acquire an in-flight slot, or `None` if the pipeline is saturated.
async fn rpc_permit() -> Option<tokio::sync::SemaphorePermit<'static>> {
    tokio::time::timeout(
        tokio::time::Duration::from_secs(PERMIT_WAIT_SECS),
        rpc_gate().acquire(),
    )
    .await
    .ok()?
    .ok()
}

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
    // Instrumented here rather than at each fail-open site: every RPC-bound filter goes
    // through this helper, so the coherence breaker counts a new filter automatically.
    let observed = get_account_retried_inner(rpc, pubkey, retries).await;
    crate::coherence::record_check(observed.is_some());
    observed
}

async fn get_account_retried_inner(
    rpc: &Arc<RpcClient>,
    pubkey: &Pubkey,
    retries: u32,
) -> Option<Account> {
    for attempt in 0..retries {
        // Bounded concurrency: see MAX_INFLIGHT_RPC. The permit is held only for the call
        // itself, and the queue wait is deliberately outside RPC_CALL_TIMEOUT_SECS —
        // charging queue time to the call's budget would reproduce the very fail-open
        // storm the gate exists to prevent.
        let Some(permit) = rpc_permit().await else {
            debug!("RPC gate saturated — get_account not attempted");
            return None;
        };
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(RPC_CALL_TIMEOUT_SECS),
            rpc.get_account(pubkey),
        )
        .await;
        drop(permit);
        match result {
            Ok(Ok(a)) => return Some(a),
            Ok(Err(e)) => {
                let msg = e.to_string();
                if is_rate_limited(&msg) && attempt + 1 < retries {
                    // 250ms / 500ms / 1000ms — paid RPCs recover from 429 within ~hundreds of ms;
                    // the old 600ms*attempt floor was tuned for the public mainnet-beta endpoint.
                    let delay = 250u64 << attempt.min(2);
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                } else {
                    return None;
                }
            }
            Err(_) => {
                debug!(
                    "RPC get_account timed out on attempt {}/{}",
                    attempt + 1,
                    retries
                );
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
    let observed = get_token_balance_retried_inner(rpc, pubkey, retries).await;
    crate::coherence::record_check(observed.is_some());
    observed
}

async fn get_token_balance_retried_inner(
    rpc: &Arc<RpcClient>,
    pubkey: &Pubkey,
    retries: u32,
) -> Option<u64> {
    for attempt in 0..retries {
        let Some(permit) = rpc_permit().await else {
            debug!("RPC gate saturated — get_token_balance not attempted");
            return None;
        };
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(RPC_CALL_TIMEOUT_SECS),
            rpc.get_token_account_balance(pubkey),
        )
        .await;
        drop(permit);
        match result {
            Ok(Ok(b)) => return b.amount.parse().ok(),
            Ok(Err(e)) => {
                let msg = e.to_string();
                if is_rate_limited(&msg) && attempt + 1 < retries {
                    let delay = 250u64 << attempt.min(2);
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                } else {
                    return None;
                }
            }
            Err(_) => {
                debug!(
                    "RPC get_token_balance timed out on attempt {}/{}",
                    attempt + 1,
                    retries
                );
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
    fn name(&self) -> &str {
        "MintRenounced"
    }
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
    fn name(&self) -> &str {
        "NotFreezable"
    }
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
    fn name(&self) -> &str {
        "LPBurned"
    }
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
    fn name(&self) -> &str {
        "PoolSize"
    }
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
    "test",
    "rug",
    "scam",
    "free",
    "airdrop",
    "safe",
    "moon100x",
    "1000x",
    "elon",
    "trump",
    "biden",
    "shib2",
    "pepe2",
    "honeypot",
    "drain",
    "presale",
    "stealth",
    "fair launch",
    "dev wallet",
];

pub struct NameFilter;

#[async_trait]
impl PoolFilter for NameFilter {
    fn name(&self) -> &str {
        "NameFilter"
    }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        // Derive Metaplex metadata PDA: ["metadata", METADATA_PROGRAM, mint]
        const METADATA_PROGRAM: Pubkey =
            solana_sdk::pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");
        let seeds: &[&[u8]] = &[
            b"metadata",
            METADATA_PROGRAM.as_ref(),
            pool.base_mint.as_ref(),
        ];
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
                        return FilterResult::fail(format!(
                            "Suspicious name: '{}' contains '{}'",
                            combined.trim(),
                            word
                        ));
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
    if offset + 4 > data.len() {
        return String::new();
    }
    let len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4])) as usize;
    let start = offset + 4;
    let end = (start + len).min(data.len());
    String::from_utf8_lossy(&data[start..end])
        .trim_end_matches('\0')
        .to_string()
}

/// Check recent transaction volume on the pool
pub struct VolumeFilter {
    pub min_txns: u32,
}

#[async_trait]
impl PoolFilter for VolumeFilter {
    fn name(&self) -> &str {
        "VolumeSpike"
    }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
        let config = GetConfirmedSignaturesForAddress2Config {
            limit: Some(self.min_txns as usize + 1),
            ..Default::default()
        };
        match rpc
            .get_signatures_for_address_with_config(&pool.id, config)
            .await
        {
            Ok(sigs) => {
                let count = sigs.len() as u32;
                if count >= self.min_txns {
                    FilterResult::pass()
                } else {
                    FilterResult::fail(format!(
                        "Insufficient volume: {} txns (need {})",
                        count, self.min_txns
                    ))
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
    fn name(&self) -> &str {
        "LiquidityDepth"
    }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        let qb = match get_token_balance_retried(rpc, &pool.quote_vault, 3).await {
            Some(v) if v > 0 => v,
            _ => return FilterResult::pass(), // can't check — fail-open
        };
        // Constant product AMM: impact = amount / (reserve + amount)
        let impact_pct =
            (self.quote_amount_raw as f64 / (qb as f64 + self.quote_amount_raw as f64)) * 100.0;
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
        Self {
            blacklist: Arc::new(set),
        }
    }
}

#[async_trait]
impl PoolFilter for BlacklistFilter {
    fn name(&self) -> &str {
        "Blacklist"
    }
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
    fn name(&self) -> &str {
        "HolderConcentration"
    }
    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        // Fetch the token's total supply (5s timeout)
        let supply = match tokio::time::timeout(
            tokio::time::Duration::from_secs(RPC_CALL_TIMEOUT_SECS),
            rpc.get_token_supply(&pool.base_mint),
        )
        .await
        {
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
        )
        .await
        {
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

        let top10_amount: u128 = top
            .iter()
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
    fn name(&self) -> &str {
        "DeployerReputation"
    }
    async fn check(&self, pool: &CachedPool, _rpc: &Arc<RpcClient>) -> FilterResult {
        // Reputation ledger is keyed by base_mint (set at sell time). Using the
        // pool account owner always returns the Raydium program ID, never matching.
        let mint_key = pool.base_mint.to_string();
        let score = self.ledger.lock().score(&mint_key);
        if score < self.min_score {
            FilterResult::fail(format!(
                "Mint {} reputation score {:.2} < min {:.2}",
                &mint_key[..8.min(mint_key.len())],
                score,
                self.min_score
            ))
        } else {
            debug!(mint = %pool.base_mint, score, "Deployer reputation OK");
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
    fn name(&self) -> &str {
        "LiquidityMomentum"
    }

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
    fn name(&self) -> &str {
        "CrossPoolCorrelation"
    }

    async fn check(&self, pool: &CachedPool, _rpc: &Arc<RpcClient>) -> FilterResult {
        // The reputation ledger is keyed by base_mint (set at sell time). Using
        // pool.id account owner returns the Raydium AMM program ID for every pool,
        // which never matches any ledger entry. Key on mint instead.
        let mint_key = pool.base_mint.to_string();
        let rug_count = self.ledger.lock().recent_rug_count(&mint_key);

        if rug_count > self.max_rugs_24h {
            FilterResult::fail(format!(
                "Mint {} has {} recorded rugs (max allowed: {})",
                &mint_key[..8.min(mint_key.len())],
                rug_count,
                self.max_rugs_24h
            ))
        } else {
            debug!(
                mint = %pool.base_mint,
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
    fn name(&self) -> &str {
        "JupiterDiscrepancy"
    }

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
        let jup_price_lamports = match crate::jup_oracle::JupiterOracle::get_price_lamports(
            &pool.base_mint,
        )
        .await
        {
            Some(p) => p,
            None => {
                warn!(mint = %pool.base_mint, "JupiterDiscrepancy: Jupiter price unavailable — skipping");
                return FilterResult::pass();
            }
        };

        let premium =
            crate::jup_oracle::JupiterOracle::premium_pct(amm_price_lamports, jup_price_lamports);

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

/// Rejects pools whose base-mint token was created less than `min_age_hours` ago.
///
/// A freshly minted token (< 48h) from a never-seen deployer is the single
/// strongest rug predictor on Solana. We use the oldest transaction on the
/// base_mint address as a proxy for when the token was created.
pub struct DeployerWalletAgeFilter {
    pub min_age_hours: u64,
}

#[async_trait]
impl PoolFilter for DeployerWalletAgeFilter {
    fn name(&self) -> &str {
        "DeployerWalletAge"
    }

    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        if self.min_age_hours == 0 {
            return FilterResult::pass();
        }

        use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
        use solana_sdk::commitment_config::CommitmentConfig;

        // Fetch up to 10 signatures for the base mint, newest first.
        // The last (oldest) entry gives us the mint creation blockTime.
        let config = GetConfirmedSignaturesForAddress2Config {
            limit: Some(10),
            commitment: Some(CommitmentConfig::confirmed()),
            ..Default::default()
        };

        let sigs = match tokio::time::timeout(
            tokio::time::Duration::from_secs(RPC_CALL_TIMEOUT_SECS),
            rpc.get_signatures_for_address_with_config(&pool.base_mint, config),
        )
        .await
        {
            Ok(Ok(s)) if !s.is_empty() => s,
            Ok(Ok(_)) => {
                debug!(mint = %pool.base_mint, "DeployerWalletAge: no signatures found — skipping");
                return FilterResult::pass();
            }
            Ok(Err(e)) => {
                warn!(mint = %pool.base_mint, "DeployerWalletAge: sig fetch failed: {} — skipping", e);
                return FilterResult::pass();
            }
            Err(_) => {
                warn!(mint = %pool.base_mint, "DeployerWalletAge: sig fetch timed out — skipping");
                return FilterResult::pass();
            }
        };

        // Oldest signature is the last in the list (they're ordered newest-first)
        let oldest = sigs.last().unwrap();
        let creation_block_time = match oldest.block_time {
            Some(t) => t,
            None => return FilterResult::pass(), // block_time unavailable — fail-open
        };

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let age_hours = (now_secs - creation_block_time).max(0) as u64 / 3600;

        if age_hours < self.min_age_hours {
            FilterResult::fail(format!(
                "Token only {} hours old (min: {} hours) — likely fresh rug wallet",
                age_hours, self.min_age_hours
            ))
        } else {
            debug!(
                mint = %pool.base_mint,
                age_hours,
                "Deployer wallet age OK"
            );
            FilterResult::pass()
        }
    }
}

/// Fetch off-chain token metadata JSON from the Metaplex URI.
/// Hard 1.5 s wall-clock cap so a slow CDN can't stall the filter pipeline.
async fn fetch_token_uri_metadata(uri: &str) -> Option<serde_json::Value> {
    if !uri.starts_with("http") {
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .user_agent("scematica-sniper/1.0")
        .build()
        .ok()?;
    let resp = tokio::time::timeout(
        tokio::time::Duration::from_millis(1800),
        client.get(uri).send(),
    )
    .await
    .ok()?
    .ok()?;
    tokio::time::timeout(
        tokio::time::Duration::from_millis(1500),
        resp.json::<serde_json::Value>(),
    )
    .await
    .ok()?
    .ok()
}

/// Enriches pool metadata with on-chain name/symbol and off-chain social links.
///
/// Always runs so that `FilterPipeline::metadata` is populated for the AI call
/// and pool scorer even when `require_socials = false`. When `require_socials`
/// is true, rejects pools that have zero social presence.
pub struct SocialLinksFilter {
    pub metadata_cache: Arc<DashMap<String, TokenSocials>>,
    /// If true, reject pools with no social links. If false, only enrich.
    pub require_socials: bool,
}

#[async_trait]
impl PoolFilter for SocialLinksFilter {
    fn name(&self) -> &str {
        "SocialLinks"
    }

    async fn check(&self, pool: &CachedPool, rpc: &Arc<RpcClient>) -> FilterResult {
        const METADATA_PROGRAM: Pubkey =
            solana_sdk::pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");
        let seeds: &[&[u8]] = &[
            b"metadata",
            METADATA_PROGRAM.as_ref(),
            pool.base_mint.as_ref(),
        ];
        let (metadata_pda, _) = Pubkey::find_program_address(seeds, &METADATA_PROGRAM);

        let acct = match tokio::time::timeout(
            tokio::time::Duration::from_secs(RPC_CALL_TIMEOUT_SECS),
            rpc.get_account(&metadata_pda),
        )
        .await
        {
            Ok(Ok(a)) if a.data.len() > 120 => a,
            _ => {
                debug!(mint = %pool.base_mint, "SocialLinks: no metadata account — skipping enrichment");
                self.metadata_cache
                    .insert(pool.base_mint.to_string(), TokenSocials::default());
                return FilterResult::pass();
            }
        };

        let data = &acct.data;
        let name = read_metaplex_string(data, 65);
        let symbol_offset = 65 + 4 + name.len().min(32);
        let symbol = read_metaplex_string(data, symbol_offset);
        let uri_offset = symbol_offset + 4 + symbol.len().min(10);
        let uri_raw = read_metaplex_string(data, uri_offset);
        let uri = uri_raw.trim().to_string();

        let mut socials = TokenSocials {
            name: name.trim().to_string(),
            symbol: symbol.trim().to_string(),
            ..TokenSocials::default()
        };

        // Off-chain URI fetch — pump.fun stores twitter/telegram/website in JSON
        if !uri.is_empty() {
            if let Some(meta) = fetch_token_uri_metadata(&uri).await {
                let check = |key: &str| {
                    meta.get(key)
                        .and_then(|v| v.as_str())
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false)
                        || meta
                            .get("extensions")
                            .and_then(|e| e.get(key))
                            .and_then(|v| v.as_str())
                            .map(|s| !s.trim().is_empty())
                            .unwrap_or(false)
                };
                socials.twitter = check("twitter");
                socials.telegram = check("telegram");
                socials.website = check("website");
                socials.discord = check("discord");
                // pump.fun also uses "createdOn" and root-level socials
                if !socials.twitter {
                    socials.twitter = check("twitter_url");
                }
                if !socials.telegram {
                    socials.telegram = check("telegram_url");
                }
            }
        }

        socials.social_count = [
            socials.twitter,
            socials.telegram,
            socials.website,
            socials.discord,
        ]
        .iter()
        .filter(|&&b| b)
        .count() as u8;

        tracing::debug!(
            mint = %pool.base_mint,
            name = %socials.name,
            symbol = %socials.symbol,
            social_count = socials.social_count,
            twitter = socials.twitter,
            telegram = socials.telegram,
            website = socials.website,
            "SocialLinks metadata enriched"
        );

        self.metadata_cache
            .insert(pool.base_mint.to_string(), socials.clone());

        if self.require_socials && socials.social_count == 0 {
            FilterResult::fail(format!(
                "Zero social links — '{}' ({}) is likely anonymous rug",
                socials.name, socials.symbol,
            ))
        } else {
            FilterResult::pass()
        }
    }
}

// ── pipeline ──────────────────────────────────────────────────────────────────

pub struct FilterPipeline {
    filters: Vec<Box<dyn PoolFilter>>,
    rpc: Arc<RpcClient>,
    pub stats: FilterStats,
    /// Pool-id → (passed, timestamp) cache to avoid repeating RPC calls on duplicate events.
    result_cache: Arc<DashMap<String, (Option<FilterRejection>, std::time::Instant)>>,
    cache_ttl_secs: u64,
    /// mint → enriched metadata (name, symbol, social links). Written by SocialLinksFilter,
    /// read by sniper AI call and pool scorer for quantitative signal enrichment.
    pub metadata: Arc<DashMap<String, TokenSocials>>,
}

impl FilterPipeline {
    pub fn new(
        config: FilterConfig,
        rpc: Arc<RpcClient>,
        quote_amount_raw: u64,
        blacklist_path: &str,
        deployer_ledger: Option<Arc<parking_lot::Mutex<DeployerLedger>>>,
    ) -> Self {
        // ── Filter ordering: cheapest first so expensive RPC calls only run on
        // pools that pass the cheap guards. Each filter's RPC cost is noted below.
        let mut filters: Vec<Box<dyn PoolFilter>> = vec![];

        // [1 RPC] In-memory blacklist check — nearly free after initial load
        if std::path::Path::new(blacklist_path).exists() {
            let bf = BlacklistFilter::load(blacklist_path);
            if !bf.blacklist.is_empty() {
                filters.push(Box::new(bf));
            }
        }
        // [1 RPC] Freeze authority check — single getAccountInfo on mint
        if config.check_freezable {
            filters.push(Box::new(FreezableFilter));
        }
        // [1 RPC] Mint renounce check — same account data as FreezableFilter
        if config.check_mint_renounced {
            filters.push(Box::new(MintRenounceFilter));
        }
        // [1 RPC] Base vault balance check — quick reject for empty/rugged pools
        if config.check_burned {
            filters.push(Box::new(BurnFilter));
        }
        // [1 RPC] Pool size range check
        if config.min_pool_size > 0.0 || config.max_pool_size > 0.0 {
            filters.push(Box::new(PoolSizeFilter {
                min_size_lamports: (config.min_pool_size * 1e9) as u64,
                max_size_lamports: (config.max_pool_size * 1e9) as u64,
            }));
        }
        // [1 RPC] Price impact from our buy size — single quote vault read
        if config.check_liquidity_depth && config.max_price_impact_pct > 0.0 {
            filters.push(Box::new(LiquidityDepthFilter {
                quote_amount_raw,
                max_price_impact_pct: config.max_price_impact_pct,
            }));
        }
        // [1 RPC + parse] Scam word check — metadata account fetch + string scan
        if config.check_name {
            filters.push(Box::new(NameFilter));
        }
        // [1 RPC] Recent tx volume check
        if config.check_volume && config.min_volume_txns > 0 {
            filters.push(Box::new(VolumeFilter {
                min_txns: config.min_volume_txns,
            }));
        }
        // [1 RPC + in-memory lookup] Deployer rug history via CrossPoolCorrelation
        // DeployerReputationFilter is DISABLED — `account.owner` always returns the
        // Raydium AMM V4 program ID (675kPX9M…), not the actual deployer wallet.
        // CrossPoolCorrelationFilter uses the same field but the ledger data is keyed
        // by program ID, which is still useful as a per-program-version guard.
        if config.check_cross_pool_correlation {
            if let Some(ref ledger) = deployer_ledger {
                filters.push(Box::new(CrossPoolCorrelationFilter {
                    ledger: Arc::clone(ledger),
                    max_rugs_24h: config.max_deployer_rugs_24h,
                }));
            }
        }
        // [1 RPC] Deployer wallet / token age via oldest mint signature
        if config.check_deployer_wallet_age && config.deployer_min_age_hours > 0 {
            filters.push(Box::new(DeployerWalletAgeFilter {
                min_age_hours: config.deployer_min_age_hours,
            }));
        }
        // [2 RPC] Top-10 holder concentration — two RPC calls (supply + largest accounts)
        if config.check_holder_concentration && config.max_top10_holder_pct > 0.0 {
            filters.push(Box::new(HolderConcentrationFilter {
                max_top10_pct: config.max_top10_holder_pct,
            }));
        }
        // [2 RPC + 3s wait] Liquidity momentum — most expensive filter, runs last
        if config.check_liquidity_momentum {
            filters.push(Box::new(LiquidityMomentumFilter {
                min_growth_pct: config.liquidity_momentum_pct,
            }));
        }
        // [2 RPC + HTTP] Jupiter discrepancy — external HTTP call, run last
        if config.check_jupiter_discrepancy {
            filters.push(Box::new(JupiterDiscrepancyFilter {
                min_premium_pct: config.jupiter_min_premium_pct,
            }));
        }

        // Shared metadata cache — populated by SocialLinksFilter, read by sniper AI + scorer.
        let metadata: Arc<DashMap<String, TokenSocials>> = Arc::new(DashMap::new());

        // SocialLinksFilter always runs for metadata enrichment (name/symbol/socials).
        // It only rejects when require_socials = true (check_socials in config).
        filters.push(Box::new(SocialLinksFilter {
            metadata_cache: Arc::clone(&metadata),
            require_socials: config.check_socials,
        }));

        let cache_ttl_secs = config.filter_cache_ttl_secs.max(5);
        Self {
            filters,
            rpc,
            stats: FilterStats::default(),
            result_cache: Arc::new(DashMap::new()),
            cache_ttl_secs,
            metadata,
        }
    }

    /// Run the pipeline. `None` means the pool passed; `Some` names the filter that
    /// stopped it.
    pub async fn execute(&self, pool: &CachedPool) -> Option<FilterRejection> {
        self.stats.pools_seen.fetch_add(1, Ordering::Relaxed);

        // TTL cache: skip redundant RPC calls when the same pool fires multiple events
        let cache_key = pool.id.to_string();
        if let Some(entry) = self.result_cache.get(&cache_key) {
            let (rejection, ts) = entry.value();
            if ts.elapsed().as_secs() < self.cache_ttl_secs {
                let rejection = rejection.clone();
                debug!(
                    mint = %pool.base_mint,
                    cached = rejection.is_none(),
                    "Filter result served from cache"
                );
                if rejection.is_none() {
                    self.stats.pools_passed.fetch_add(1, Ordering::Relaxed);
                }
                return rejection;
            }
        }

        let rejection = if self.filters.is_empty() {
            None
        } else {
            self.run_once(pool).await
        };

        // Cache the result
        self.result_cache
            .insert(cache_key, (rejection.clone(), std::time::Instant::now()));

        // Periodically evict stale cache entries (every ~100 evaluations on average)
        if self.stats.pools_seen.load(Ordering::Relaxed) % 100 == 0 {
            let ttl = self.cache_ttl_secs;
            self.result_cache
                .retain(|_, (_, ts)| ts.elapsed().as_secs() < ttl);
        }

        if rejection.is_none() {
            self.stats.pools_passed.fetch_add(1, Ordering::Relaxed);
        }
        self.stats.write_to_file(FILTER_STATS_FILE);
        rejection
    }

    async fn run_once(&self, pool: &CachedPool) -> Option<FilterRejection> {
        // Run every filter concurrently — independent RPC reads with no side effects.
        // For a typical 10-filter config this replaces ~10*(RPC_RTT) sequential
        // latency with a single ~RPC_RTT round-trip in the common case.
        let futures = self.filters.iter().map(|filter| async move {
            let name = filter.name();
            let result = filter.check(pool, &self.rpc).await;
            (name, result)
        });
        let results = futures::future::join_all(futures).await;

        // Surface the first failure (deterministic order = filter declaration order).
        for (name, result) in results {
            if !result.passed {
                let reason = result.reason.as_deref().unwrap_or("unknown");
                info!(mint = %pool.base_mint, filter = name, reason = %reason, "Filter rejected pool");
                self.stats.record_rejection(name);
                return Some(FilterRejection {
                    filter: name.to_string(),
                    reason: reason.to_string(),
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The decision log's `reason` column is parsed by analysis tooling and by anyone
    /// reading `scematica-pool-decisions.jsonl`, so the shape is pinned here rather than
    /// left to whatever `format!` happens to say.
    #[test]
    fn rejection_renders_filter_and_reason() {
        let r = FilterRejection {
            filter: "LpBurnFilter".to_string(),
            reason: "Pool vault is empty (possibly rugged)".to_string(),
        };
        assert_eq!(
            r.as_decision_reason(),
            "filter=LpBurnFilter;reason=Pool vault is empty (possibly rugged)"
        );
    }

    /// `stage` stays `filters` and only `reason` gains detail. `replay.rs` matches
    /// governed stages on `stage` precisely because reasons are heterogeneous free text;
    /// if that ever inverts, this comment is the thing that was wrong.
    #[test]
    fn rejection_reason_is_prefixed_so_it_cannot_be_confused_with_a_bare_stage() {
        let r = FilterRejection { filter: "X".into(), reason: "y".into() };
        assert!(r.as_decision_reason().starts_with("filter="));
        assert_ne!(r.as_decision_reason(), "filter_rejected");
    }
}
