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

            // Future open_time means clock skew or a fake timestamp — treat as
            // suspicious instead of falling through to "ultra fresh".
            //
            // v0.8.0: bands tightened ~30% across the board (operator request).
            // Strictest variant: cutoffs shrink, penalties for stale pools grow,
            // bonuses for fresh pools concentrate on the very-newest window.
            if pool.open_time > now_secs + 30 {
                score -= 32.0;
            } else {
                let age_secs = now_secs.saturating_sub(pool.open_time);
                if age_secs <= 7 {
                    // Ultra-fresh: maximum first-mover advantage
                    score += 30.0;
                } else if age_secs <= 21 {
                    score += 20.0;
                } else if age_secs <= 42 {
                    score += 10.0;
                } else if age_secs <= 84 {
                    score += 3.0;
                } else if age_secs <= 210 {
                    // No bonus, no penalty
                } else if age_secs <= 420 {
                    score -= 20.0;
                } else {
                    // Pump is statistically over for pools > 7 min old (was 10)
                    score -= 38.0;
                }
            }
        }

        // ── Pool size ─────────────────────────────────────────────────────────
        if pool_size_lamports > 0 {
            const SOL: u64 = 1_000_000_000; // 1 SOL in lamports

            // v0.8.0: sweet spot narrowed from 5–30 SOL to ~6.5–22 SOL (30% tighter).
            // Penalties on thin/oversized pools amplified to push the score
            // distribution further from 50 (neutral) on both tails — gives the
            // `min_pool_score` gate a sharper cutoff to work with.
            if pool_size_lamports < SOL {
                score -= 33.0;
            } else if pool_size_lamports < 4 * SOL {
                score -= 16.0;
            } else if pool_size_lamports < 13 * SOL / 2 { // 6.5 SOL
                score -= 6.0;
            } else if pool_size_lamports <= 22 * SOL {
                score += 18.0;
            } else if pool_size_lamports <= 70 * SOL {
                score += 6.0;
            } else if pool_size_lamports <= 500 * SOL {
                // Neutral — likely a graduated pool with diminished upside
            } else {
                // Whale-backed but late entry — minimal bonus only
                score += 3.0;
            }
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
        // 10 SOL — sweet spot (6.5–22 SOL)
        let score = PoolScorer::score(&pool, 10 * 1_000_000_000);
        // 50 + 18 (sweet spot) = 68
        assert!((score - 68.0).abs() < 0.1, "score={}", score);
    }

    #[test]
    fn test_fresh_good_size() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let pool = make_pool(now - 5); // 5s old → ultra-fresh (≤7)
        // 10 SOL
        let score = PoolScorer::score(&pool, 10 * 1_000_000_000);
        // 50 + 30 (ultra-fresh) + 18 (sweet spot) = 98
        assert!((score - 98.0).abs() < 0.1, "score={}", score);
    }

    #[test]
    fn test_stale_thin() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 300s → stale 210–420 band (-20).
        let pool = make_pool(now - 300);
        // 1.5 SOL → thin 1–4 band (-16)
        let score = PoolScorer::score(&pool, 1_500_000_000);
        // 50 - 20 - 16 = 14
        assert!((score - 14.0).abs() < 0.1, "score={}", score);
    }
}
