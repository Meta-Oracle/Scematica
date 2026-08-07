use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::debug;

#[derive(Debug, Clone)]
struct BoostCacheEntry {
    is_boosted: bool,
    total_amount_usd: f64,
    checked_at: Instant,
}

/// Per-mint cache for DexScreener boost lookups.
///
/// A token with a non-zero `boostAmount` has paid DexScreener for advertising,
/// which is a strong positive signal: the team has spent real money and DexScreener
/// visitors are actively being directed to the token.  Results are cached for 5
/// minutes so hot pools don't hammer the API.
#[derive(Default)]
pub struct DexScreenerCache {
    inner: Arc<DashMap<String, BoostCacheEntry>>,
}

impl DexScreenerCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `Some(usd_total)` if the token has an active boost, `None` otherwise.
    /// Network errors and missing data both return `None` (fail-open).
    pub async fn check_boost(&self, mint: &str) -> Option<f64> {
        const TTL: Duration = Duration::from_secs(300); // 5-minute cache

        if let Some(entry) = self.inner.get(mint) {
            if entry.checked_at.elapsed() < TTL {
                return if entry.is_boosted { Some(entry.total_amount_usd) } else { None };
            }
        }

        let result = Self::fetch_boost_http(mint).await;
        self.inner.insert(mint.to_string(), BoostCacheEntry {
            is_boosted: result.is_some(),
            total_amount_usd: result.unwrap_or(0.0),
            checked_at: Instant::now(),
        });
        result
    }

    async fn fetch_boost_http(mint: &str) -> Option<f64> {
        let url = format!("https://api.dexscreener.com/latest/dex/tokens/{}", mint);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(1500))
            .user_agent("scematica-sniper/1.6")
            .build()
            .ok()?;

        let resp = tokio::time::timeout(
            Duration::from_millis(1800),
            client.get(&url).send(),
        ).await.ok()?.ok()?;

        let json: serde_json::Value = tokio::time::timeout(
            Duration::from_millis(1500),
            resp.json(),
        ).await.ok()?.ok()?;

        let pairs = json.get("pairs")?.as_array()?;
        let total: f64 = pairs.iter()
            .filter_map(|p| p.get("boostAmount")?.as_f64())
            .sum();

        if total > 0.0 {
            debug!(mint = %mint, boost_usd = total, "DexScreener boost confirmed");
            Some(total)
        } else {
            None
        }
    }
}
