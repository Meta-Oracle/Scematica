use solana_sdk::pubkey::Pubkey;
use tracing::{debug, warn};

/// Jupiter Price API v6 oracle.
///
/// Queries the Jupiter Price API to get a reliable reference price for a token
/// and compares it against the AMM pool price to detect arbitrage opportunities.
pub struct JupiterOracle;

impl JupiterOracle {
    /// Query the Jupiter Price API v6 for `mint` denominated in WSOL.
    ///
    /// Returns the price in lamports-per-token-unit (as f64), or `None` on any
    /// failure (network error, rate-limit, unparseable response).
    ///
    /// Uses: `https://price.jup.ag/v6/price?ids=MINT&vsToken=So11111111111111111111111111111111111111112`
    pub async fn get_price_lamports(mint: &Pubkey) -> Option<f64> {
        // WSOL mint address
        const WSOL: &str = "So11111111111111111111111111111111111111112";

        let url = format!(
            "https://price.jup.ag/v6/price?ids={}&vsToken={}",
            mint, WSOL
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .ok()?;

        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("JupiterOracle: request failed for {}: {}", mint, e);
                return None;
            }
        };

        if !resp.status().is_success() {
            warn!("JupiterOracle: non-200 response {} for {}", resp.status(), mint);
            return None;
        }

        let json: serde_json::Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                warn!("JupiterOracle: JSON parse failed for {}: {}", mint, e);
                return None;
            }
        };

        // Response schema:
        // { "data": { "<MINT>": { "id": "...", "price": 0.000001234 } } }
        let price_ui = json["data"][mint.to_string()]["price"].as_f64()?;

        if price_ui <= 0.0 {
            return None;
        }

        // Convert UI price (SOL per token) → lamports per token
        let price_lamports = price_ui * 1_000_000_000.0;
        debug!(mint = %mint, price_lamports, "JupiterOracle: got price");
        Some(price_lamports)
    }

    /// Calculate the % premium of the Jupiter price vs the AMM pool price.
    ///
    /// A positive premium means Jupiter is more expensive than the AMM →
    /// the pool is cheap relative to the broader market = a buy signal.
    ///
    /// Returns 0.0 if either price is zero.
    pub fn premium_pct(amm_lamports: f64, jup_lamports: f64) -> f64 {
        if amm_lamports <= 0.0 || jup_lamports <= 0.0 {
            return 0.0;
        }
        (jup_lamports - amm_lamports) / amm_lamports * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_premium_pct() {
        // Jupiter 10% more expensive than AMM
        let pct = JupiterOracle::premium_pct(1000.0, 1100.0);
        assert!((pct - 10.0).abs() < 0.01, "pct={}", pct);
    }

    #[test]
    fn test_negative_premium() {
        // Jupiter cheaper than AMM (unusual) → negative premium
        let pct = JupiterOracle::premium_pct(1000.0, 900.0);
        assert!((pct + 10.0).abs() < 0.01, "pct={}", pct);
    }

    #[test]
    fn test_zero_inputs() {
        assert_eq!(JupiterOracle::premium_pct(0.0, 100.0), 0.0);
        assert_eq!(JupiterOracle::premium_pct(100.0, 0.0), 0.0);
    }
}
