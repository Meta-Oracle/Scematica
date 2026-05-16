use crate::cache::CachedPool;

/// Predictive pool score in the range 0.0–100.0.
///
/// Higher score = more likely to pump.
/// The score is composed from observable on-chain signals available at pool
/// creation time without additional RPC calls beyond what the listener already
/// provides.
pub struct PoolScorer;

impl PoolScorer {
    /// Score a pool 0..100 based on observable signals.
    ///
    /// Inputs:
    /// - `pool`: the newly detected pool (from the listener or pool cache)
    /// - `pool_size_lamports`: quote vault balance at the time of detection
    ///   (pass 0 if unavailable — treated as neutral)
    pub fn score(pool: &CachedPool, pool_size_lamports: u64) -> f64 {
        let mut score: f64 = 50.0;

        // ── Pool age ──────────────────────────────────────────────────────────
        // open_time == 0 means unknown — leave score neutral
        if pool.open_time > 0 {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let age_secs = now_secs.saturating_sub(pool.open_time);

            if age_secs <= 30 {
                // Very fresh — maximum bonus
                score += 20.0;
            } else if age_secs <= 120 {
                // Recent — moderate bonus
                score += 10.0;
            } else if age_secs > 300 {
                // Stale — early pump is likely over
                score -= 20.0;
            }
            // 120–300 s: neutral (no change)
        }

        // ── Pool size ─────────────────────────────────────────────────────────
        if pool_size_lamports > 0 {
            const SOL: u64 = 1_000_000_000; // 1 SOL in lamports

            let sol_amount = pool_size_lamports / SOL;

            if pool_size_lamports >= 5 * SOL && sol_amount <= 50 {
                // Sweet spot: 5–50 SOL — enough liquidity without dev already pumping
                score += 15.0;
            } else if pool_size_lamports > 500 * SOL {
                // Whale-backed large pool
                score += 10.0;
            } else if pool_size_lamports < 5 * SOL {
                // Too thin — high rug risk
                score -= 15.0;
            }
            // 50–500 SOL: neutral
        }

        // Clamp to valid range
        score.max(0.0).min(100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    fn make_pool(open_time: u64) -> CachedPool {
        CachedPool {
            id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            base_vault: Pubkey::new_unique(),
            quote_vault: Pubkey::new_unique(),
            market_id: Pubkey::new_unique(),
            open_time,
            base_decimals: 9,
            quote_decimals: 9,
        }
    }

    #[test]
    fn test_unknown_open_time() {
        let pool = make_pool(0);
        // 10 SOL
        let score = PoolScorer::score(&pool, 10 * 1_000_000_000);
        // 50 + 15 (good size) = 65
        assert!((score - 65.0).abs() < 0.1, "score={}", score);
    }

    #[test]
    fn test_fresh_good_size() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let pool = make_pool(now - 10); // 10s old
        // 10 SOL
        let score = PoolScorer::score(&pool, 10 * 1_000_000_000);
        // 50 + 20 (fresh) + 15 (good size) = 85
        assert!((score - 85.0).abs() < 0.1, "score={}", score);
    }

    #[test]
    fn test_stale_thin() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let pool = make_pool(now - 400); // 400s old
        // 1 SOL — too thin
        let score = PoolScorer::score(&pool, 1_000_000_000);
        // 50 - 20 (stale) - 15 (thin) = 15
        assert!((score - 15.0).abs() < 0.1, "score={}", score);
    }
}
