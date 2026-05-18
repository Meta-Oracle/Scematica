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
    /// - `pool_size_lamports`: quote vault (SOL side) balance at detection time
    ///   (pass 0 if unavailable — treated as neutral)
    /// - `base_vault_lamports`: token vault (base side) raw balance at detection time
    ///   (pass 0 if unavailable — skips momentum scoring)
    /// - `detected_at_secs`: Unix timestamp when this pool was first observed.
    ///   Pump.fun always sets `open_time=0` meaning "open immediately"; passing
    ///   the detection timestamp here lets the scorer apply the freshness bonus
    ///   for pools that have no explicit open_time. Pass 0 to skip.
    pub fn score(pool: &CachedPool, pool_size_lamports: u64, base_vault_lamports: u64, detected_at_secs: u64) -> f64 {
        let mut score: f64 = 50.0;

        // ── Pool age ──────────────────────────────────────────────────────────
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Pump.fun sets open_time=0 meaning "open immediately". When the pool
        // was detected recently (within 60 s), treat detection time as effective
        // open_time so the ultra-fresh freshness bonus applies.
        let effective_open_time = if pool.open_time > 0 {
            pool.open_time
        } else if detected_at_secs > 0 && detected_at_secs >= now_secs.saturating_sub(60) {
            detected_at_secs
        } else {
            0
        };

        if effective_open_time > 0 {
            // Future open_time means clock skew or a fake timestamp — treat as
            // suspicious instead of falling through to "ultra fresh".
            //
            // v0.8.0: bands tightened ~30% across the board (operator request).
            if effective_open_time > now_secs + 30 {
                score -= 32.0;
            } else {
                let age_secs = now_secs.saturating_sub(effective_open_time);
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
                    // Pump is statistically over for pools > 7 min old
                    score -= 38.0;
                }
            }
        }

        // ── Pool size ─────────────────────────────────────────────────────────
        if pool_size_lamports > 0 {
            const SOL: u64 = 1_000_000_000; // 1 SOL in lamports

            // v1.5.0: sweet spot extended from 6.5–22 → 6.5–28 SOL based on live
            // data showing winning pools cluster at 18–28 SOL.
            // Large pools (>100 SOL) are penalised — they are either graduated
            // tokens whose initial pump is over, or heavily traded pools where
            // sniper edge is diluted. Neutral zone (70–100 SOL) still applies
            // as a buffer for borderline cases.
            if pool_size_lamports < SOL {
                score -= 33.0;
            } else if pool_size_lamports < 4 * SOL {
                score -= 16.0;
            } else if pool_size_lamports < 13 * SOL / 2 { // 6.5 SOL
                score -= 6.0;
            } else if pool_size_lamports <= 28 * SOL {    // sweet spot: was 22 SOL
                score += 18.0;
            } else if pool_size_lamports <= 70 * SOL {
                score += 8.0;                              // was +6
            } else if pool_size_lamports <= 100 * SOL {
                // Neutral — slightly large but can still be a fresh launch
            } else if pool_size_lamports <= 400 * SOL {
                score -= 8.0;   // likely established pool, pump may be over
            } else {
                score -= 20.0;  // whale-backed but late entry — unfavourable odds
            }
        }

        // ── Momentum / buy-pressure ──────────────────────────────────────────────
        // Two signals: SOL accumulation velocity and buy-pressure ratio.
        //
        // 1. Velocity = quote_vault_SOL / age_seconds.
        //    High SOL/sec inflow means buyers piled in fast — runner candidate.
        //    Only meaningful when we know the real pool.open_time (not a
        //    detection-time fallback), because pool_size / 0-1 s is not a
        //    velocity signal — it's just the pool's SOL divided by nothing.
        //
        // 2. Buy pressure = quote_vault / base_vault (in SOL-equivalent terms).
        //    Raydium AMM: price = quote / base (raw). As buyers buy tokens the SOL
        //    side grows and the token side shrinks, so quote/base rises above the
        //    launch ratio. A ratio >> 1 SOL-per-raw-token indicates accumulated
        //    buying.
        if pool_size_lamports > 0 && pool.open_time > 0 {
            if pool.open_time <= now_secs {
                let age_secs = now_secs.saturating_sub(pool.open_time).max(1);
                let quote_sol = pool_size_lamports as f64 / 1_000_000_000.0;
                let velocity = quote_sol / age_secs as f64; // SOL per second

                // Velocity bonus — catches runners accumulating fast
                let vel_bonus: f64 = if velocity >= 15.0 {
                    22.0  // exceptional: 15+ SOL/s — crowd piling in
                } else if velocity >= 5.0 {
                    14.0  // strong: 5–15 SOL/s
                } else if velocity >= 1.5 {
                    7.0   // moderate: 1.5–5 SOL/s
                } else if velocity >= 0.4 {
                    2.0   // mild inflow
                } else {
                    0.0   // slow — no momentum signal
                };
                score += vel_bonus;

                // Buy-pressure ratio bonus — quote/base skew above neutral
                // Only useful when base vault is known and non-zero.
                if base_vault_lamports > 0 {
                    let ratio = pool_size_lamports as f64 / base_vault_lamports as f64;
                    let pressure_bonus: f64 = if ratio >= 0.5 {
                        12.0  // heavily bought up — strong momentum confirmation
                    } else if ratio >= 0.05 {
                        6.0
                    } else if ratio >= 0.005 {
                        2.0
                    } else {
                        0.0
                    };
                    score += pressure_bonus;
                }
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
        // 10 SOL — sweet spot (6.5–28 SOL), no base vault, no detected_at
        let score = PoolScorer::score(&pool, 10 * 1_000_000_000, 0, 0);
        // 50 + 18 (sweet spot) = 68
        assert!((score - 68.0).abs() < 0.1, "score={}", score);
    }

    #[test]
    fn test_pumpfun_fresh_detection() {
        let pool = make_pool(0); // pump.fun style: open_time=0
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 20 SOL sweet spot, detected right now → ultra-fresh bonus applies
        let score = PoolScorer::score(&pool, 20 * 1_000_000_000, 0, now);
        // 50 + 30 (ultra-fresh via detected_at fallback) + 18 (sweet spot) = 98
        assert!(score >= 95.0, "score={}", score);
    }

    #[test]
    fn test_pumpfun_stale_detected_at() {
        let pool = make_pool(0);
        // detected_at=0 → no fallback → no age bonus
        let score = PoolScorer::score(&pool, 20 * 1_000_000_000, 0, 0);
        // 50 + 18 (sweet spot) = 68
        assert!((score - 68.0).abs() < 0.1, "score={}", score);
    }

    #[test]
    fn test_fresh_good_size() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let pool = make_pool(now - 5); // 5s old → ultra-fresh (≤7), real open_time
        // 10 SOL, no base vault; velocity = 10/5 = 2 SOL/s → +2
        let score = PoolScorer::score(&pool, 10 * 1_000_000_000, 0, 0);
        // 50 + 30 (ultra-fresh) + 18 (sweet spot) + 2 (velocity) = 100 (clamped)
        assert!(score >= 98.0, "score={}", score);
    }

    #[test]
    fn test_stale_thin() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 300s → stale 210–420 band (-20).
        let pool = make_pool(now - 300);
        // 1.5 SOL → thin 1–4 band (-16), no base vault
        let score = PoolScorer::score(&pool, 1_500_000_000, 0, 0);
        // 50 - 20 - 16 + velocity(1.5/300=0.005 SOL/s → 0) = 14
        assert!((score - 14.0).abs() < 0.1, "score={}", score);
    }

    #[test]
    fn test_momentum_runner() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 15s old, 20 SOL — velocity = 20/15 = 1.33 SOL/s → mild (+2)
        // With high buy-pressure: quote=20e9, base=1e9 → ratio=20 → +12
        let pool = make_pool(now - 15);
        let score = PoolScorer::score(&pool, 20 * 1_000_000_000, 1_000_000_000, 0);
        // 50 + 20 (age 7–21) + 18 (sweet spot) + 2 (velocity) + 12 (pressure) = 102 → 100
        assert!(score >= 99.0, "score={}", score);
    }

    #[test]
    fn test_large_pool_penalty() {
        let pool = make_pool(0);
        // 200 SOL — above 100 SOL threshold, gets -8 penalty
        let score = PoolScorer::score(&pool, 200 * 1_000_000_000, 0, 0);
        // 50 - 8 = 42
        assert!((score - 42.0).abs() < 0.1, "score={}", score);
    }
}
